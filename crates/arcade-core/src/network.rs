//! Local and remote network diagnostics. Network targets are user supplied,
//! validated as data, and never passed through a shell.

use crate::{
    process::{self, ProcessSpec},
    provider,
    tool_kit::{check_cancelled, option_bool, option_str, single_value, success},
};
use arcade_contract::{ToolManifest, ToolRequest, ToolResult, ToolValue};
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    fs,
    io::{Read, Write},
    net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use uuid::Uuid;

const NETWORK_TIMEOUT: Duration = Duration::from_secs(20);
const NETWORK_OUTPUT_LIMIT: usize = 2 * 1024 * 1024;
const HTTP_INSPECTION_TIMEOUT: Duration = Duration::from_secs(90);

pub fn execute(
    manifest: &ToolManifest,
    request: &ToolRequest,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    check_cancelled(cancelled)?;
    let input = single_value(request, 4096)?.trim();
    let (value, mime, message) = match manifest.id.as_str() {
        "arcade.network.site-check" => {
            let (value, summary) = site_check(input, request, cancelled)?;
            (value, "structured/site-check", Some(summary))
        }
        "arcade.network.rdap" => (rdap(input, cancelled)?, "structured/registration", None),
        "arcade.network.ip" => (ip_inspect(request, cancelled)?, "structured/ip-info", None),
        _ => {
            return Err(format!(
                "No network executor is registered for {}",
                manifest.id
            ));
        }
    };
    Ok(success(
        manifest,
        vec![ToolValue::text(value.to_string(), mime)],
        message,
        vec![],
    ))
}

/// "Is it down?": DNS, HTTP (with redirects and timing), ping, and the TLS
/// certificate, checked in parallel. Each part reports its own error so one
/// failing probe (ping is often blocked) never hides the others.
fn site_check(
    input: &str,
    request: &ToolRequest,
    cancelled: &AtomicBool,
) -> Result<(Value, String), String> {
    let url = if input.contains("://") {
        validate_http_url(input)?
    } else {
        validate_http_url(&format!("https://{input}"))?
    };
    let parsed = url::Url::parse(&url).map_err(|error| format!("Invalid URL: {error}"))?;
    let host = parsed
        .host_str()
        .ok_or("Enter a website or host name")?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_owned();
    let is_ip = host.parse::<IpAddr>().is_ok();
    let https = parsed.scheme() == "https";
    let port = parsed.port_or_known_default().unwrap_or(443);
    let records: &[&str] = match option_str(request, "record", "basic") {
        "basic" => &["A", "AAAA", "CNAME"],
        "all" => &["A", "AAAA", "CNAME", "MX", "NS", "TXT"],
        "MX" => &["MX"],
        "TXT" => &["TXT"],
        "NS" => &["NS"],
        other => return Err(format!("Unknown DNS record choice: {other}")),
    };
    let want_ping = option_bool(request, "ping", true);
    let want_certificate = option_bool(request, "certificate", true) && https;
    let as_value = |result: Result<Value, String>| {
        result.unwrap_or_else(|error| json!({ "error": bounded_text(&error, 600) }))
    };
    let (dns, http, ping, certificate) = std::thread::scope(|scope| {
        let dns = (!is_ip).then(|| scope.spawn(|| dns_records(&host, records, cancelled)));
        let ping = want_ping.then(|| scope.spawn(|| ping_host(&host, 3, cancelled)));
        let certificate =
            want_certificate.then(|| scope.spawn(|| certificate_info(&host, port, cancelled)));
        let http = http_check(&url, "HEAD", cancelled);
        let join = |handle: std::thread::ScopedJoinHandle<'_, Result<Value, String>>| {
            handle
                .join()
                .unwrap_or_else(|_| Err("This check stopped unexpectedly".into()))
        };
        (dns.map(join), http, ping.map(join), certificate.map(join))
    });
    check_cancelled(cancelled)?;
    let resolves = dns.as_ref().is_none_or(|result| {
        result.as_ref().is_ok_and(|value| {
            value["records"].as_array().is_some_and(|records| {
                records
                    .iter()
                    .any(|record| matches!(record["type"].as_str(), Some("A" | "AAAA")))
            })
        })
    });
    let (verdict, summary) = match &http {
        Ok(response) => {
            let status = response["status"].as_u64().unwrap_or(0) as u16;
            let seconds = response["elapsedSeconds"].as_f64().unwrap_or(0.0);
            let phrase = format!("{status} {}", status_reason(status))
                .trim()
                .to_owned();
            if status >= 500 {
                ("down", format!("Down — the server answered {phrase}"))
            } else if status >= 400 {
                (
                    "problem",
                    format!("Reachable, but the page answered {phrase} ({seconds:.2} s)"),
                )
            } else {
                ("up", format!("Up — {phrase} in {seconds:.2} s"))
            }
        }
        Err(_) if !resolves => (
            "down",
            "Down — the domain name does not resolve to an address".to_owned(),
        ),
        Err(error) => (
            "down",
            format!("Down — could not connect: {}", bounded_text(error, 200)),
        ),
    };
    let mut http_value = as_value(http);
    if let Some(object) = http_value.as_object_mut() {
        if !option_bool(request, "headers", false) {
            object.remove("headers");
        }
        if !option_bool(request, "redirects", true) {
            object.remove("redirects");
        }
    }
    let value = json!({
        "target": redact_url(&url),
        "host": host,
        "verdict": verdict,
        "summary": summary,
        "http": http_value,
        "dns": dns.map(as_value),
        "ping": ping.map(as_value),
        "certificate": certificate.map(as_value),
        "note": "Checked from this computer. If the site is up for others but down here, the problem is likely your network, DNS, or a blocker.",
    });
    Ok((value, summary))
}

fn status_reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        410 => "Gone",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        520..=530 => "CDN Error",
        _ => "",
    }
}

fn validate_host(raw: &str) -> Result<String, String> {
    let raw = raw.trim().trim_start_matches('[').trim_end_matches(']');
    if raw.is_empty()
        || raw.len() > 253
        || raw.chars().any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err("Enter a valid host name or IP address".into());
    }
    if let Ok(ip) = raw.parse::<IpAddr>() {
        return Ok(ip.to_string());
    }
    let domain = raw.trim_end_matches('.');
    if domain.is_empty()
        || !domain.is_ascii()
        || domain.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
    {
        return Err("Enter a valid ASCII host name or IP address".into());
    }
    Ok(domain.to_ascii_lowercase())
}

pub(crate) fn validate_http_url(raw: &str) -> Result<String, String> {
    let url = raw.trim();
    if url.is_empty() || url.len() > 16_384 || url.chars().any(char::is_control) {
        return Err("Enter an HTTP or HTTPS URL".into());
    }
    let (scheme, rest) = url
        .split_once("://")
        .ok_or("URL must begin with http:// or https://")?;
    if !matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https") {
        return Err("Only HTTP and HTTPS URLs are supported".into());
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.is_empty() || authority.contains('@') {
        return Err("URL must contain a host and cannot include embedded credentials".into());
    }
    let host_port = if authority.starts_with('[') {
        let end = authority.find(']').ok_or("Invalid IPv6 URL host")?;
        let host = &authority[1..end];
        let suffix = &authority[end + 1..];
        if !suffix.is_empty() && !suffix.starts_with(':') {
            return Err("Invalid URL authority".into());
        }
        (host, suffix.strip_prefix(':'))
    } else {
        let mut parts = authority.rsplitn(2, ':');
        let last = parts.next().unwrap_or_default();
        let before = parts.next();
        if let Some(host) = before.filter(|_| last.bytes().all(|b| b.is_ascii_digit())) {
            (host, Some(last))
        } else {
            (authority, None)
        }
    };
    validate_host(host_port.0)?;
    if let Some(port) = host_port.1 {
        let parsed = port.parse::<u16>().map_err(|_| "Invalid URL port")?;
        if parsed == 0 {
            return Err("URL port must be between 1 and 65535".into());
        }
    }
    Ok(url.to_owned())
}

fn ping_host(host: &str, count: u8, cancelled: &AtomicBool) -> Result<Value, String> {
    let target = validate_host(host)?;
    #[cfg(windows)]
    let (name, args) = (
        "ping",
        vec![
            "-n".to_string(),
            count.to_string(),
            "-w".into(),
            "1500".into(),
            target.clone(),
        ],
    );
    #[cfg(not(windows))]
    let (name, args) = (
        "ping",
        vec![
            "-c".to_string(),
            count.to_string(),
            "-W".into(),
            "2".into(),
            target.clone(),
        ],
    );
    let path =
        provider::find_system_executable(name).ok_or("A system ping utility was not found")?;
    let probe = probe_command(&path, &version_probe_args(), "ping", cancelled)?;
    let output = process::run(
        &ProcessSpec {
            executable: path,
            args: args.iter().map(OsString::from).collect(),
            current_dir: None,
            timeout: Duration::from_secs(30),
            output_limit: 256 * 1024,
        },
        cancelled,
    )
    .map_err(|error| error.to_string())?;
    let raw = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if raw.trim().is_empty() {
        return Err("Ping returned no diagnostic output".into());
    }
    let transmitted = parse_number_before(&raw, "packets transmitted");
    let received = parse_number_before(&raw, "received");
    let loss = raw.lines().find_map(|line| {
        let lower = line.to_ascii_lowercase();
        lower.find('%').and_then(|index| {
            lower[..index]
                .split_whitespace()
                .last()?
                .parse::<f64>()
                .ok()
        })
    });
    let average_ms = raw.lines().find_map(|line| {
        // "rtt min/avg/max/mdev = 9.1/10.2/11.3/0.5 ms" or "Average = 10ms"
        if let Some((_, values)) = line.split_once(" = ").filter(|_| line.contains("min/avg")) {
            return values.split('/').nth(1)?.trim().parse::<f64>().ok();
        }
        let (_, value) = line.split_once("Average = ")?;
        value.trim().trim_end_matches("ms").parse::<f64>().ok()
    });
    Ok(json!({
        "host": target,
        "provider": provider_label(&probe),
        "success": output.status.success(),
        "transmitted": transmitted.unwrap_or(count as u64),
        "received": received,
        "packetLossPercent": loss,
        "averageMs": average_ms,
        "raw": bounded_text(raw.trim(), 8000),
    }))
}

fn version_probe_args() -> Vec<&'static str> {
    #[cfg(windows)]
    {
        vec!["/?"]
    }
    #[cfg(target_os = "macos")]
    {
        vec!["-h"]
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        vec!["-V"]
    }
}

fn probe_command(
    path: &Path,
    args: &[&str],
    marker: &str,
    cancelled: &AtomicBool,
) -> Result<String, String> {
    let output = process::run(
        &ProcessSpec {
            executable: path.to_path_buf(),
            args: args.iter().map(OsString::from).collect(),
            current_dir: None,
            timeout: Duration::from_secs(3),
            output_limit: 128 * 1024,
        },
        cancelled,
    )
    .map_err(|error| error.to_string())?;
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !combined.to_ascii_lowercase().contains(marker) {
        return Err(format!(
            "The detected executable did not identify itself as {marker}"
        ));
    }
    Ok(combined.lines().next().unwrap_or(marker).trim().to_owned())
}

fn provider_label(version: &str) -> Value {
    json!({ "versionProbe": version })
}

fn parse_number_before(text: &str, marker: &str) -> Option<u64> {
    let lower = text.to_ascii_lowercase();
    let index = lower.find(marker)?;
    lower[..index].split_whitespace().last()?.parse().ok()
}

/// Query each record type with the system resolver and merge the answers.
fn dns_records(host: &str, types: &[&str], cancelled: &AtomicBool) -> Result<Value, String> {
    let name = validate_host(host)?;
    let resolver = default_resolver()?;
    let mut records = Vec::new();
    for record in types {
        let qtype = match *record {
            "A" => 1,
            "NS" => 2,
            "CNAME" => 5,
            "MX" => 15,
            "TXT" => 16,
            "AAAA" => 28,
            other => return Err(format!("Unsupported DNS record type {other}")),
        };
        let (answers, _) = dns_query(&name, qtype, resolver, cancelled)?;
        records.extend(answers);
    }
    let mut seen = std::collections::HashSet::new();
    records.retain(|record| seen.insert(record.to_string()));
    Ok(json!({ "query": name, "resolver": resolver.to_string(), "records": records }))
}

fn default_resolver() -> Result<IpAddr, String> {
    #[cfg(unix)]
    if let Ok(contents) = std::fs::read_to_string("/etc/resolv.conf") {
        for line in contents.lines() {
            let mut words = line.split_whitespace();
            if words.next() == Some("nameserver") {
                if let Some(address) = words.next().and_then(|value| value.parse().ok()) {
                    return Ok(address);
                }
            }
        }
    }
    #[cfg(windows)]
    {
        return Ok("1.1.1.1".parse().expect("static resolver IP"));
    }
    Err("Could not find a configured DNS resolver; enter a resolver IP address".into())
}

fn dns_query(
    name: &str,
    qtype: u16,
    resolver: IpAddr,
    cancelled: &AtomicBool,
) -> Result<(Vec<Value>, &'static str), String> {
    let mut packet = Vec::with_capacity(512);
    let id = Uuid::new_v4().as_bytes()[..2].to_vec();
    packet.extend_from_slice(&id);
    packet.extend_from_slice(&[0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    for label in name.trim_end_matches('.').split('.') {
        if label.is_empty() || label.len() > 63 {
            return Err("Invalid DNS query name".into());
        }
        packet.push(label.len() as u8);
        packet.extend_from_slice(label.as_bytes());
    }
    packet.push(0);
    packet.extend_from_slice(&qtype.to_be_bytes());
    packet.extend_from_slice(&1u16.to_be_bytes());
    let bind: SocketAddr = if resolver.is_ipv4() {
        "0.0.0.0:0".parse().unwrap()
    } else {
        "[::]:0".parse().unwrap()
    };
    let socket =
        UdpSocket::bind(bind).map_err(|error| format!("Cannot open DNS socket: {error}"))?;
    socket
        .connect(SocketAddr::new(resolver, 53))
        .map_err(|error| format!("Cannot reach DNS resolver: {error}"))?;
    socket
        .set_read_timeout(Some(Duration::from_millis(400)))
        .map_err(|error| error.to_string())?;
    socket
        .send(&packet)
        .map_err(|error| format!("DNS request failed: {error}"))?;
    let start = std::time::Instant::now();
    let mut response = [0u8; 4096];
    let size = loop {
        if cancelled.load(Ordering::Relaxed) {
            return Err("DNS lookup cancelled".into());
        }
        match socket.recv(&mut response) {
            Ok(count) => break count,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) && start.elapsed() < Duration::from_secs(4) =>
            {
                continue;
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return Err("DNS resolver timed out".into());
            }
            Err(error) => return Err(format!("DNS response failed: {error}")),
        }
    };
    let message = &response[..size];
    if dns_response_is_truncated(message, &id)? {
        let response = dns_tcp_query(resolver, &packet, &id, cancelled)?;
        return Ok((parse_dns_response(&response, &id, cancelled)?, "tcp"));
    }
    Ok((parse_dns_response(message, &id, cancelled)?, "udp"))
}

fn dns_response_is_truncated(message: &[u8], query_id: &[u8]) -> Result<bool, String> {
    if message.len() < 4 || &message[..2] != query_id {
        return Err("Invalid DNS response".into());
    }
    let flags = u16::from_be_bytes([message[2], message[3]]);
    if flags & 0x8000 == 0 {
        return Err("DNS server returned a non-response packet".into());
    }
    Ok(flags & 0x0200 != 0)
}

fn dns_tcp_query(
    resolver: IpAddr,
    packet: &[u8],
    query_id: &[u8],
    cancelled: &AtomicBool,
) -> Result<Vec<u8>, String> {
    let start = std::time::Instant::now();
    let mut stream =
        TcpStream::connect_timeout(&SocketAddr::new(resolver, 53), Duration::from_secs(4))
            .map_err(|error| format!("DNS TCP retry could not connect: {error}"))?;
    stream
        .set_read_timeout(Some(Duration::from_millis(400)))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_millis(400)))
        .map_err(|error| error.to_string())?;
    let length = u16::try_from(packet.len()).map_err(|_| "DNS query is too large")?;
    stream
        .write_all(&length.to_be_bytes())
        .and_then(|_| stream.write_all(packet))
        .map_err(|error| format!("DNS TCP retry failed to send the query: {error}"))?;
    let mut frame_length = [0u8; 2];
    read_dns_tcp_exact(&mut stream, &mut frame_length, start, cancelled)?;
    let length = u16::from_be_bytes(frame_length) as usize;
    if !(12..=65535).contains(&length) {
        return Err("DNS TCP retry returned an invalid response length".into());
    }
    let mut response = vec![0u8; length];
    read_dns_tcp_exact(&mut stream, &mut response, start, cancelled)?;
    let _ = dns_response_is_truncated(&response, query_id)?;
    Ok(response)
}

fn read_dns_tcp_exact(
    stream: &mut TcpStream,
    buffer: &mut [u8],
    start: std::time::Instant,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    let mut read = 0;
    while read < buffer.len() {
        if cancelled.load(Ordering::Relaxed) {
            return Err("DNS lookup cancelled".into());
        }
        if start.elapsed() >= Duration::from_secs(4) {
            return Err("DNS TCP retry timed out".into());
        }
        match stream.read(&mut buffer[read..]) {
            Ok(0) => return Err("DNS TCP retry returned an incomplete response".into()),
            Ok(count) => read += count,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(error) => return Err(format!("DNS TCP retry failed: {error}")),
        }
    }
    Ok(())
}

fn parse_dns_response(
    message: &[u8],
    query_id: &[u8],
    cancelled: &AtomicBool,
) -> Result<Vec<Value>, String> {
    if message.len() < 12 || &message[..2] != query_id {
        return Err("Invalid DNS response".into());
    }
    let flags = u16::from_be_bytes([message[2], message[3]]);
    if flags & 0x8000 == 0 {
        return Err("DNS server returned a non-response packet".into());
    }
    if flags & 0x0200 != 0 {
        return Err("DNS response is truncated".into());
    }
    let rcode = flags & 0x000f;
    if rcode != 0 {
        return Err(match rcode {
            3 => "No such domain (the name does not exist in DNS)".into(),
            2 => "The DNS server failed to answer (SERVFAIL)".into(),
            5 => "The DNS server refused the query".into(),
            _ => format!("DNS query failed with response code {rcode}"),
        });
    }
    let questions = u16::from_be_bytes([message[4], message[5]]) as usize;
    let answers = u16::from_be_bytes([message[6], message[7]]) as usize;
    let mut cursor = 12;
    for _ in 0..questions {
        let _ = dns_name(message, &mut cursor)?;
        cursor = cursor
            .checked_add(4)
            .filter(|end| *end <= message.len())
            .ok_or("Invalid DNS question")?;
    }
    let mut records = Vec::new();
    for _ in 0..answers.min(128) {
        if cancelled.load(Ordering::Relaxed) {
            return Err("DNS lookup cancelled".into());
        }
        let owner = dns_name(message, &mut cursor)?;
        if cursor + 10 > message.len() {
            return Err("Truncated DNS record".into());
        }
        let kind = u16::from_be_bytes([message[cursor], message[cursor + 1]]);
        let class = u16::from_be_bytes([message[cursor + 2], message[cursor + 3]]);
        let ttl = u32::from_be_bytes(message[cursor + 4..cursor + 8].try_into().unwrap());
        let length = u16::from_be_bytes([message[cursor + 8], message[cursor + 9]]) as usize;
        cursor += 10;
        let end = cursor
            .checked_add(length)
            .filter(|end| *end <= message.len())
            .ok_or("Truncated DNS record data")?;
        let value = match kind {
            1 if length == 4 => {
                IpAddr::from(<[u8; 4]>::try_from(&message[cursor..end]).unwrap()).to_string()
            }
            28 if length == 16 => {
                IpAddr::from(<[u8; 16]>::try_from(&message[cursor..end]).unwrap()).to_string()
            }
            2 | 5 | 12 => {
                let mut pos = cursor;
                dns_name(message, &mut pos)?
            }
            15 if length >= 3 => {
                let priority = u16::from_be_bytes([message[cursor], message[cursor + 1]]);
                let mut pos = cursor + 2;
                format!("{priority} {}", dns_name(message, &mut pos)?)
            }
            16 => parse_dns_txt(&message[cursor..end]),
            33 if length >= 7 => {
                let priority = u16::from_be_bytes([message[cursor], message[cursor + 1]]);
                let weight = u16::from_be_bytes([message[cursor + 2], message[cursor + 3]]);
                let port = u16::from_be_bytes([message[cursor + 4], message[cursor + 5]]);
                let mut pos = cursor + 6;
                format!(
                    "priority={priority} weight={weight} port={port} target={}",
                    dns_name(message, &mut pos)?
                )
            }
            6 => parse_dns_soa(message, cursor, end)?,
            _ => hex(&message[cursor..end]),
        };
        records.push(json!({ "name": owner, "type": dns_type_name(kind), "class": class, "ttl": ttl, "value": value }));
        cursor = end;
    }
    Ok(records)
}

fn dns_name(message: &[u8], cursor: &mut usize) -> Result<String, String> {
    let mut labels = Vec::new();
    let mut position = *cursor;
    let mut resume = None;
    let mut visited = std::collections::HashSet::new();
    for _ in 0..128 {
        let length = *message.get(position).ok_or("Truncated DNS name")?;
        if length & 0xc0 == 0xc0 {
            let second = *message.get(position + 1).ok_or("Truncated DNS pointer")?;
            let target = (((length & 0x3f) as usize) << 8) | second as usize;
            if target >= message.len() || !visited.insert(target) {
                return Err("Invalid DNS compression pointer".into());
            }
            if resume.is_none() {
                resume = Some(position + 2);
            }
            position = target;
            continue;
        }
        position += 1;
        if length == 0 {
            *cursor = resume.unwrap_or(position);
            return Ok(labels.join("."));
        }
        let length = length as usize;
        let end = position
            .checked_add(length)
            .filter(|end| *end <= message.len())
            .ok_or("Truncated DNS label")?;
        let label =
            std::str::from_utf8(&message[position..end]).map_err(|_| "DNS label is not UTF-8")?;
        if label.bytes().any(|byte| byte.is_ascii_control()) {
            return Err("Invalid DNS label".into());
        }
        labels.push(label.to_owned());
        position = end;
    }
    Err("DNS name is too deeply compressed".into())
}

fn parse_dns_txt(bytes: &[u8]) -> String {
    let mut cursor = 0;
    let mut parts = Vec::new();
    while cursor < bytes.len() {
        let length = bytes[cursor] as usize;
        cursor += 1;
        if cursor + length > bytes.len() {
            return hex(bytes);
        }
        parts.push(String::from_utf8_lossy(&bytes[cursor..cursor + length]).into_owned());
        cursor += length;
    }
    parts.join("")
}

fn parse_dns_soa(message: &[u8], start: usize, end: usize) -> Result<String, String> {
    let mut cursor = start;
    let mname = dns_name(message, &mut cursor)?;
    let rname = dns_name(message, &mut cursor)?;
    if cursor + 20 > end {
        return Err("Truncated SOA record".into());
    }
    let numbers = message[cursor..cursor + 20]
        .chunks_exact(4)
        .map(|bytes| u32::from_be_bytes(bytes.try_into().unwrap()))
        .collect::<Vec<_>>();
    Ok(format!(
        "mname={mname} rname={rname} serial={} refresh={} retry={} expire={} minimum={}",
        numbers[0], numbers[1], numbers[2], numbers[3], numbers[4]
    ))
}

fn dns_type_name(kind: u16) -> &'static str {
    match kind {
        1 => "A",
        2 => "NS",
        5 => "CNAME",
        6 => "SOA",
        12 => "PTR",
        15 => "MX",
        16 => "TXT",
        28 => "AAAA",
        33 => "SRV",
        _ => "UNKNOWN",
    }
}

fn http_check(
    url: &str,
    requested_method: &'static str,
    cancelled: &AtomicBool,
) -> Result<Value, String> {
    let url = validate_http_url(url)?;
    let curl = curl_provider()?;
    let body_stage =
        tempfile::tempdir().map_err(|error| format!("Could not stage HTTP response: {error}"))?;
    let body_path = body_stage.path().join("response.body");
    let inspection_start = std::time::Instant::now();
    let mut inspection = inspect_http_chain(
        &curl,
        &url,
        requested_method,
        &body_path,
        HTTP_INSPECTION_TIMEOUT,
        cancelled,
    )?;
    let fallback_from_head =
        requested_method == "HEAD" && matches!(inspection.response.status, 405 | 501);
    if fallback_from_head {
        let _ = fs::remove_file(&body_path);
        let remaining = HTTP_INSPECTION_TIMEOUT
            .checked_sub(inspection_start.elapsed())
            .ok_or("HTTP inspection timed out before the bounded GET retry")?;
        if remaining.is_zero() {
            return Err("HTTP inspection timed out before the bounded GET retry".into());
        }
        inspection = inspect_http_chain(&curl, &url, "GET", &body_path, remaining, cancelled)?;
    }
    let headers = parse_safe_headers(
        &inspection.response.header_text,
        &inspection.response.effective_url,
    );
    Ok(json!({
        "requestedUrl": redact_url(&url),
        "status": inspection.response.status,
        "effectiveUrl": redact_url(&inspection.response.effective_url),
        "elapsedSeconds": inspection.elapsed_seconds,
        "remoteAddress": inspection.response.remote_address,
        "redirects": inspection.redirects,
        "headers": headers,
        "provider": { "name": "curl", "version": curl.version },
        "method": inspection.response.method,
        "headOnly": inspection.response.method == "HEAD",
        "bodyBytesInspected": inspection.body_bytes,
        "bodyLimitBytes": 1_048_576,
        "fallbackFromHead": fallback_from_head,
    }))
}

struct HttpInspectionResponse {
    method: &'static str,
    status: u16,
    effective_url: String,
    elapsed_seconds: Option<f64>,
    remote_address: String,
    header_text: String,
}

struct HttpInspectionChain {
    response: HttpInspectionResponse,
    redirects: Vec<Value>,
    elapsed_seconds: f64,
    body_bytes: u64,
}

struct PinnedRedirectTarget {
    host: String,
    port: u16,
    address: IpAddr,
}

fn inspect_http_chain(
    curl: &provider::ProviderInfo,
    starting_url: &str,
    method: &'static str,
    body_path: &Path,
    timeout: Duration,
    cancelled: &AtomicBool,
) -> Result<HttpInspectionChain, String> {
    let start = std::time::Instant::now();
    let mut current_url = starting_url.to_owned();
    let mut redirect_pin = None;
    let mut redirects = Vec::new();
    let mut elapsed_seconds = 0.0;
    let mut body_bytes = 0u64;
    for hop in 0..=10 {
        let remaining = timeout
            .checked_sub(start.elapsed())
            .ok_or("HTTP inspection timed out while following redirects")?;
        if remaining.is_zero() {
            return Err("HTTP inspection timed out while following redirects".into());
        }
        let response = run_http_request(
            curl,
            &current_url,
            method,
            body_path,
            remaining.min(NETWORK_TIMEOUT),
            redirect_pin.as_ref(),
            cancelled,
        )?;
        let hop_elapsed = response.elapsed_seconds.unwrap_or(0.0);
        elapsed_seconds += hop_elapsed;
        body_bytes = body_bytes.saturating_add(
            fs::metadata(body_path)
                .map(|metadata| metadata.len())
                .unwrap_or(0),
        );
        let location = if (300..400).contains(&response.status) {
            response_location(&response.header_text).filter(|value| !value.is_empty())
        } else {
            None
        };
        let next_url = if let Some(location) = location.as_deref() {
            Some(resolve_url(&current_url, location).ok_or_else(|| {
                "The server redirected to a URL that is not a valid HTTP(S) address".to_owned()
            })?)
        } else {
            None
        };
        redirects.push(json!({
            "url": redact_url(&current_url),
            "status": response.status,
            "location": next_url.as_deref().map(redact_url),
            "elapsedSeconds": hop_elapsed,
        }));
        if let Some(next_url) = next_url {
            if hop == 10 {
                return Err("The URL redirects more than 10 times".into());
            }
            redirect_pin = Some(resolve_public_redirect_target(&next_url)?);
            current_url = next_url;
        } else {
            return Ok(HttpInspectionChain {
                response,
                redirects,
                elapsed_seconds,
                body_bytes,
            });
        }
    }
    Err("The URL redirects more than 10 times".into())
}

fn response_location(raw: &str) -> Option<String> {
    let final_block = raw
        .split("\r\n\r\n")
        .filter(|block| block.starts_with("HTTP/"))
        .last()?;
    final_block.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("location")
            .then(|| value.trim().to_owned())
    })
}

fn resolve_public_redirect_target(raw_url: &str) -> Result<PinnedRedirectTarget, String> {
    let url = url::Url::parse(raw_url).map_err(|_| "Invalid HTTP redirect URL")?;
    let host = url
        .host_str()
        .ok_or("HTTP redirect has no destination host")?
        .to_owned();
    let port = url
        .port_or_known_default()
        .ok_or("HTTP redirect has an invalid destination port")?;
    let addresses = if let Ok(address) = host.parse::<IpAddr>() {
        vec![address]
    } else {
        (host.as_str(), port)
            .to_socket_addrs()
            .map_err(|_| "Could not resolve the HTTP redirect destination")?
            .map(|address| address.ip())
            .take(64)
            .collect::<Vec<_>>()
    };
    if addresses.is_empty() {
        return Err("The HTTP redirect destination has no DNS addresses".into());
    }
    if addresses.iter().any(|address| !is_public_address(*address)) {
        return Err(
            "The HTTP redirect points to a local, private, or reserved network address".into(),
        );
    }
    Ok(PinnedRedirectTarget {
        host,
        port,
        // Pin the connection to the checked address so a second DNS lookup
        // cannot change the target between validation and the request.
        address: addresses[0],
    })
}

fn is_public_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            let octets = address.octets();
            !(octets[0] == 0
                || octets[0] == 10
                || octets[0] == 127
                || (octets[0] == 100 && (64..=127).contains(&octets[1]))
                || (octets[0] == 169 && octets[1] == 254)
                || (octets[0] == 172 && (16..=31).contains(&octets[1]))
                || (octets[0] == 192 && octets[1] == 0)
                || (octets[0] == 192 && octets[1] == 2)
                || (octets[0] == 192 && octets[1] == 168)
                || (octets[0] == 198 && (octets[1] == 18 || octets[1] == 19))
                || (octets[0] == 198 && octets[1] == 51 && octets[2] == 100)
                || (octets[0] == 203 && octets[1] == 0 && octets[2] == 113)
                || octets[0] >= 224)
        }
        IpAddr::V6(address) => {
            if let Some(mapped) = address.to_ipv4_mapped() {
                return is_public_address(IpAddr::V4(mapped));
            }
            let segments = address.segments();
            !(address.is_unspecified()
                || address.is_loopback()
                || address.is_multicast()
                || (segments[0] & 0xfe00) == 0xfc00 // unique local
                || (segments[0] & 0xffc0) == 0xfe80 // link local
                || (segments[0] == 0x2001 && segments[1] == 0x0db8) // documentation
                || (segments[0] == 0x2001 && segments[1] == 0x0000) // Teredo
                || segments[0] == 0x2002) // 6to4 embeds IPv4
        }
    }
}

fn run_http_request(
    curl: &provider::ProviderInfo,
    url: &str,
    method: &'static str,
    body_path: &Path,
    timeout: Duration,
    redirect_pin: Option<&PinnedRedirectTarget>,
    cancelled: &AtomicBool,
) -> Result<HttpInspectionResponse, String> {
    let _ = fs::remove_file(body_path);
    let mut args = curl_common_args();
    args.extend([
        "--max-time".into(),
        format!("{:.3}", timeout.as_secs_f64()).into(),
    ]);
    if let Some(target) = redirect_pin {
        args.extend(["--noproxy".into(), "*".into()]);
        if target.host.parse::<IpAddr>().is_err() {
            let address = match target.address {
                IpAddr::V4(address) => address.to_string(),
                IpAddr::V6(address) => format!("[{address}]"),
            };
            args.extend([
                "--resolve".into(),
                format!("{}:{}:{}", target.host, target.port, address).into(),
            ]);
        }
    }
    if method == "HEAD" {
        args.push("--head".into());
    } else {
        // Inspect only a one-byte range. If the server ignores Range, curl
        // refuses to write a response larger than the hard 1 MiB cap.
        args.extend([
            "--range".into(),
            "0-0".into(),
            "--max-filesize".into(),
            "1048576".into(),
        ]);
    }
    args.extend([
        "--dump-header".into(),
        "-".into(),
        "--output".into(),
        body_path.as_os_str().to_owned(),
        "--write-out".into(),
        "\nARCADE_META:%{http_code}\t%{url_effective}\t%{time_total}\t%{remote_ip}\n".into(),
        "--".into(),
        url.into(),
    ]);
    let output = process::run(
        &ProcessSpec {
            executable: curl.executable_path.clone(),
            args,
            current_dir: None,
            timeout,
            output_limit: 1024 * 1024,
        },
        cancelled,
    )
    .map_err(|error| error.to_string())?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if method == "GET" && stderr.to_ascii_lowercase().contains("maximum file size") {
            return Err("The server ignored the byte-range request and the HTTP response exceeded the 1 MiB inspection limit".into());
        }
        let detail = process_error(&output.stderr, "HTTP request failed");
        return Err(detail.replace(url, &redact_url(url)));
    }
    let marker = "\nARCADE_META:";
    let (header_text, metadata) = stdout
        .rsplit_once(marker)
        .ok_or("curl did not return HTTP metadata")?;
    let fields = metadata.trim().split('\t').collect::<Vec<_>>();
    if fields.len() != 4 {
        return Err("curl returned incomplete HTTP metadata".into());
    }
    Ok(HttpInspectionResponse {
        method,
        status: fields[0].parse::<u16>().unwrap_or(0),
        effective_url: fields[1].to_owned(),
        elapsed_seconds: fields[2].parse::<f64>().ok(),
        remote_address: fields[3].to_owned(),
        header_text: header_text.to_owned(),
    })
}

fn parse_safe_headers(raw: &str, base_url: &str) -> Vec<Value> {
    let final_block = raw
        .split("\r\n\r\n")
        .filter(|block| block.starts_with("HTTP/"))
        .last()
        .unwrap_or(raw);
    final_block
        .lines()
        .skip(1)
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            let name = name.trim().to_ascii_lowercase();
            let value = if matches!(
                name.as_str(),
                "set-cookie" | "www-authenticate" | "proxy-authenticate"
            ) {
                "[redacted]".into()
            } else if name == "location" {
                resolve_url(base_url, value.trim())
                    .map(|url| redact_url(&url))
                    .unwrap_or_else(|| "[redacted invalid redirect target]".into())
            } else {
                bounded_text(value.trim(), 2048)
            };
            Some(json!({ "name": name, "value": value }))
        })
        .take(128)
        .collect()
}

pub(crate) fn curl_common_args() -> Vec<OsString> {
    [
        "-q",
        "--silent",
        "--show-error",
        "--proto",
        "=http,https",
        "--proto-redir",
        "=http,https",
        "--connect-timeout",
        "15",
        "--max-time",
        "60",
        "--user-agent",
        "ArcadeBox/0.1",
    ]
    .into_iter()
    .map(OsString::from)
    .collect()
}

pub(crate) fn curl_provider() -> Result<provider::ProviderInfo, String> {
    provider::discover_curl()
        .into_iter()
        .find(|provider| provider.compatible)
        .ok_or_else(|| "A compatible system curl with HTTPS support was not found".into())
}

pub(crate) fn process_error(stderr: &[u8], fallback: &str) -> String {
    let text = String::from_utf8_lossy(stderr).trim().to_owned();
    if text.is_empty() {
        fallback.into()
    } else {
        bounded_text(&text, 4000)
    }
}

fn rdap(input: &str, cancelled: &AtomicBool) -> Result<Value, String> {
    let query = validate_host(input)?;
    let endpoint = if query.parse::<IpAddr>().is_ok() {
        format!("https://rdap.org/ip/{}", encode_component(&query))
    } else {
        format!("https://rdap.org/domain/{}", encode_component(&query))
    };
    let curl = curl_provider()?;
    let mut args = curl_common_args();
    args.extend([
        "--location".into(),
        "--max-redirs".into(),
        "5".into(),
        "--fail".into(),
        "--header".into(),
        "Accept: application/rdap+json, application/json".into(),
        "--".into(),
        endpoint.clone().into(),
    ]);
    let output = process::run(
        &ProcessSpec {
            executable: curl.executable_path,
            args,
            current_dir: None,
            timeout: NETWORK_TIMEOUT,
            output_limit: NETWORK_OUTPUT_LIMIT,
        },
        cancelled,
    )
    .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(process_error(&output.stderr, "RDAP lookup failed"));
    }
    let mut body: Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("RDAP returned invalid JSON: {error}"))?;
    redact_rdap(&mut body);
    Ok(json!({ "query": query, "provider": "rdap.org", "endpoint": endpoint, "record": body }))
}

fn redact_rdap(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, value) in map.iter_mut() {
                if matches!(
                    key.to_ascii_lowercase().as_str(),
                    "email" | "tel" | "telephone" | "phone"
                ) {
                    *value = Value::String("[redacted]".into());
                } else {
                    redact_rdap(value);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(redact_rdap),
        _ => {}
    }
}

fn ip_inspect(request: &ToolRequest, cancelled: &AtomicBool) -> Result<Value, String> {
    let include_public = request
        .options
        .get("includePublic")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let route_probe = UdpSocket::bind("0.0.0.0:0").map_err(|error| error.to_string())?;
    let _ = route_probe.connect("1.1.1.1:53");
    let local = route_probe
        .local_addr()
        .ok()
        .map(|address| address.ip().to_string());
    let networks = sysinfo::Networks::new_with_refreshed_list();
    let mut interfaces = networks
        .iter()
        .filter(|(name, _)| name.as_str() != "lo" && !name.starts_with("lo0"))
        .filter_map(|(name, data)| {
            let addresses = data
                .ip_networks()
                .iter()
                .map(|network| format!("{}/{}", network.addr, network.prefix))
                .collect::<Vec<_>>();
            if addresses.is_empty() {
                return None;
            }
            let primary = local.as_deref().is_some_and(|local| {
                data.ip_networks()
                    .iter()
                    .any(|network| network.addr.to_string() == local)
            });
            let mac = data.mac_address().to_string();
            Some(json!({
                "interface": name,
                "addresses": addresses.join(", "),
                "mac": if mac == "00:00:00:00:00:00" { Value::Null } else { json!(mac) },
                "primary": primary,
            }))
        })
        .collect::<Vec<_>>();
    interfaces.sort_by_key(|item| !item["primary"].as_bool().unwrap_or(false));
    let mut result = json!({
        "headline": local.clone().map_or("Not connected".to_owned(), |ip| format!("Local IP {ip}")),
        "localAddress": local,
        "interfaces": interfaces,
    });
    if include_public {
        let curl = curl_provider()?;
        let mut args = curl_common_args();
        args.extend([
            "--max-time".into(),
            "10".into(),
            "--fail".into(),
            "--".into(),
            "https://api.ipify.org".into(),
        ]);
        let output = process::run(
            &ProcessSpec {
                executable: curl.executable_path,
                args,
                current_dir: None,
                timeout: Duration::from_secs(12),
                output_limit: 128,
            },
            cancelled,
        )
        .map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err(process_error(
                &output.stderr,
                "Could not check the externally visible IP",
            ));
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let public: IpAddr = text
            .trim()
            .parse()
            .map_err(|_| "Public IP service returned an invalid address")?;
        result["headline"] = json!(format!("Public IP {public}"));
        result["publicAddress"] = json!(public.to_string());
        result["publicProvider"] = json!("api.ipify.org");
    }
    Ok(result)
}

fn certificate_info(host: &str, port: u16, cancelled: &AtomicBool) -> Result<Value, String> {
    let host = validate_host(host)?;
    let openssl = provider::find_system_executable("openssl")
        .ok_or("Install OpenSSL to inspect TLS certificates on this system")?;
    let version = run_provider_probe(&openssl, &["version"], "OpenSSL", cancelled)?;
    let mut args = vec![
        OsString::from("s_client"),
        OsString::from("-showcerts"),
        OsString::from("-connect"),
        OsString::from(format_authority(&host, port)),
    ];
    if host.parse::<IpAddr>().is_err() {
        args.push(OsString::from("-servername"));
        args.push(OsString::from(host.clone()));
    }
    let output = process::run(
        &ProcessSpec {
            executable: openssl.clone(),
            args,
            current_dir: None,
            timeout: Duration::from_secs(15),
            output_limit: 1024 * 1024,
        },
        cancelled,
    )
    .map_err(|error| error.to_string())?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let pem = stdout
        .find("-----BEGIN CERTIFICATE-----")
        .and_then(|start| {
            stdout[start..]
                .find("-----END CERTIFICATE-----")
                .map(|end| &stdout[start..start + end + "-----END CERTIFICATE-----".len()])
        })
        .ok_or_else(|| {
            let details = bounded_text(
                &format!("{}{}", stdout, String::from_utf8_lossy(&output.stderr)),
                1500,
            );
            format!("Could not retrieve a TLS certificate from {host}:{port}. {details}")
        })?;
    let args = [
        "x509",
        "-noout",
        "-subject",
        "-issuer",
        "-dates",
        "-ext",
        "subjectAltName",
        "-fingerprint",
        "-sha256",
    ];
    let parsed = process::run_with_input(
        &ProcessSpec {
            executable: openssl,
            args: args.iter().map(OsString::from).collect(),
            current_dir: None,
            timeout: Duration::from_secs(5),
            output_limit: 64 * 1024,
        },
        cancelled,
        pem.as_bytes(),
    )
    .map_err(|error| error.to_string())?;
    if !parsed.status.success() {
        return Err(process_error(
            &parsed.stderr,
            "OpenSSL could not parse the server certificate",
        ));
    }
    let details = String::from_utf8_lossy(&parsed.stdout);
    let mut fields = serde_json::Map::new();
    for line in details.lines() {
        if let Some((key, value)) = line.split_once('=') {
            fields.insert(
                key.to_ascii_lowercase().replace(' ', "_"),
                json!(bounded_text(value.trim(), 4096)),
            );
        }
    }
    let days_left = fields
        .get("notafter")
        .and_then(Value::as_str)
        .and_then(|date| {
            chrono::NaiveDateTime::parse_from_str(
                date.trim_end_matches(" GMT"),
                "%b %e %H:%M:%S %Y",
            )
            .ok()
        })
        .map(|expires| (expires.and_utc() - chrono::Utc::now()).num_days());
    Ok(json!({
        "host": host,
        "port": port,
        "provider": version,
        "valid": days_left.is_some_and(|days| days >= 0),
        "daysLeft": days_left,
        "certificate": fields,
        "chainCountAvailable": stdout.matches("-----BEGIN CERTIFICATE-----").count(),
    }))
}

fn run_provider_probe(
    path: &Path,
    args: &[&str],
    expected: &str,
    cancelled: &AtomicBool,
) -> Result<String, String> {
    let output = process::run(
        &ProcessSpec {
            executable: path.to_path_buf(),
            args: args.iter().map(OsString::from).collect(),
            current_dir: None,
            timeout: Duration::from_secs(3),
            output_limit: 64 * 1024,
        },
        cancelled,
    )
    .map_err(|error| error.to_string())?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !text
        .to_ascii_lowercase()
        .contains(&expected.to_ascii_lowercase())
    {
        return Err(format!(
            "Detected executable did not identify as {expected}"
        ));
    }
    Ok(text.lines().next().unwrap_or_default().trim().to_owned())
}

fn format_authority(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

pub(crate) fn resolve_url(base: &str, target: &str) -> Option<String> {
    if target.starts_with("http://") || target.starts_with("https://") {
        return validate_http_url(target).ok();
    }
    let (_, rest) = base.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let scheme = base.split_once("://")?.0;
    let origin = format!("{scheme}://{authority}");
    if target.starts_with("//") {
        return validate_http_url(&format!("{scheme}:{target}")).ok();
    }
    if target.is_empty() {
        return Some(base.to_owned());
    }
    if target.contains(':')
        && target
            .split(['/', '?', '#'])
            .next()
            .is_some_and(|segment| segment.contains(':'))
    {
        // Avoid treating javascript:, data:, mailto:, or other schemes as a
        // relative path in a redirect chain.
        if target
            .find(':')
            .is_some_and(|colon| !target[..colon].contains('.'))
        {
            return None;
        }
    }
    let base_path = rest
        .strip_prefix(authority)
        .unwrap_or_default()
        .split(['?', '#'])
        .next()
        .unwrap_or_default();
    let (target_path, suffix) = split_url_suffix(target);
    let combined = if target_path.starts_with('/') {
        target_path.to_owned()
    } else {
        let parent = base_path
            .rsplit_once('/')
            .map(|(prefix, _)| prefix)
            .unwrap_or("");
        format!("{parent}/{target_path}")
    };
    let normalized = normalize_url_path(&combined)?;
    validate_http_url(&format!("{origin}{normalized}{suffix}")).ok()
}

pub(crate) fn redact_url(raw: &str) -> String {
    let Some((before_query, query_and_fragment)) = raw.split_once('?') else {
        return raw.to_owned();
    };
    let (query, fragment) = query_and_fragment
        .split_once('#')
        .map(|(query, fragment)| (query, Some(fragment)))
        .unwrap_or((query_and_fragment, None));
    let redacted = query
        .split('&')
        .map(|item| {
            let (key, value) = item.split_once('=').unwrap_or((item, ""));
            let lower = key.to_ascii_lowercase();
            let sensitive = [
                "token",
                "access_token",
                "refresh_token",
                "id_token",
                "auth",
                "authorization",
                "secret",
                "password",
                "passwd",
                "key",
                "api_key",
                "apikey",
                "signature",
                "sig",
                "session",
                "sessionid",
                "credential",
                "code",
            ]
            .iter()
            .any(|part| lower == *part || lower.ends_with(part));
            if sensitive && !value.is_empty() {
                format!("{key}=[redacted]")
            } else {
                item.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("&");
    match fragment {
        Some(fragment) => format!("{before_query}?{redacted}#{fragment}"),
        None => format!("{before_query}?{redacted}"),
    }
}

fn split_url_suffix(value: &str) -> (&str, &str) {
    let boundary = value.find(['?', '#']).unwrap_or(value.len());
    value.split_at(boundary)
}

fn normalize_url_path(path: &str) -> Option<String> {
    let trailing_slash = path.ends_with('/');
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            value => segments.push(value),
        }
    }
    let mut normalized = format!("/{}", segments.join("/"));
    if trailing_slash && !normalized.ends_with('/') {
        normalized.push('/');
    }
    Some(normalized)
}

fn encode_component(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0xf) as usize] as char);
    }
    output
}

fn bounded_text(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_owned();
    }
    let mut end = max;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &value[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsafe_url_protocols_credentials_and_invalid_hosts() {
        assert!(validate_http_url("file:///etc/passwd").is_err());
        assert!(validate_http_url("https://user:pass@example.com").is_err());
        assert!(validate_http_url("https://good.example/\nInjected: yes").is_err());
        assert!(validate_host("bad host").is_err());
    }

    #[test]
    fn dns_name_reader_handles_compression_without_cycles() {
        let mut packet = vec![0u8; 12];
        packet.extend_from_slice(&[
            3, b'w', b'w', b'w', 7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 3, b'c', b'o', b'm',
            0,
        ]);
        packet.extend_from_slice(&[0xc0, 0x0c]);
        let mut cursor = 29;
        assert_eq!(dns_name(&packet, &mut cursor).unwrap(), "www.example.com");
        assert_eq!(cursor, 31);
        let cyclic = [0xc0, 0x00];
        let mut cursor = 0;
        assert!(dns_name(&cyclic, &mut cursor).is_err());
    }

    #[test]
    fn dns_truncation_bit_selects_tcp_retry_path() {
        let mut response = [0u8; 12];
        response[..4].copy_from_slice(&[0x12, 0x34, 0x82, 0x00]);
        assert!(dns_response_is_truncated(&response, &[0x12, 0x34]).unwrap());
        assert!(!dns_response_is_truncated(&[0x12, 0x34, 0x80, 0x00], &[0x12, 0x34]).unwrap());
        assert!(
            parse_dns_response(&response, &[0x12, 0x34], &AtomicBool::new(false))
                .unwrap_err()
                .contains("truncated")
        );
    }

    #[test]
    fn per_hop_location_accepts_only_valid_http_destinations() {
        let raw = "HTTP/1.1 302 Found\r\nLocation: ../next?token=hidden\r\n\r\n";
        let target = response_location(raw).unwrap();
        let resolved = resolve_url("https://example.org/a/page", &target).unwrap();
        assert_eq!(
            redact_url(&resolved),
            "https://example.org/next?token=[redacted]"
        );
        assert!(resolve_url("https://example.org/a", "javascript:alert(1)").is_none());
    }

    #[test]
    fn redirect_targets_reject_private_addresses_and_pin_public_ones() {
        for address in [
            "127.0.0.1".parse().unwrap(),
            "10.1.2.3".parse().unwrap(),
            "169.254.169.254".parse().unwrap(),
            "::1".parse().unwrap(),
            "fc00::1".parse().unwrap(),
            "fe80::1".parse().unwrap(),
        ] {
            assert!(!is_public_address(address));
        }
        for address in [
            "8.8.8.8".parse().unwrap(),
            "2606:4700:4700::1111".parse().unwrap(),
        ] {
            assert!(is_public_address(address));
        }
        assert!(resolve_public_redirect_target("http://127.0.0.1/").is_err());
    }

    #[test]
    fn redirect_resolution_stays_on_explicit_http_protocols() {
        assert_eq!(
            resolve_url("https://example.org/a/page", "../next"),
            Some("https://example.org/next".into())
        );
        assert_eq!(
            resolve_url("https://example.org/a", "//other.example/path"),
            Some("https://other.example/path".into())
        );
        assert!(validate_http_url("javascript:alert(1)").is_err());
    }
}
