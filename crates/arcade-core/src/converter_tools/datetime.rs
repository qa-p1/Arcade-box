//! Date and time tools: time-zone conversion, date differences, adding or
//! subtracting time, and Unix timestamp conversion. Inputs accept ISO dates,
//! date-times, `now`, `today`, and (for timestamps) plain numbers.

use crate::tool_kit::{option_f64, option_str};
use arcade_contract::ToolRequest;
use chrono::{
    DateTime, Datelike, Duration, FixedOffset, Local, LocalResult, Months, NaiveDate,
    NaiveDateTime, SecondsFormat, TimeZone, Utc, Weekday,
};
use chrono_tz::Tz;
use serde_json::{Value, json};

/// A parsed moment and whether the user typed only a date.
struct Moment {
    at: DateTime<FixedOffset>,
    date_only: bool,
}

/// The time zone named by a control: `local` (the default) or an IANA name.
#[derive(Clone, Copy)]
enum Zone {
    Local,
    Named(Tz),
}

impl Zone {
    fn parse(name: &str) -> Result<Self, String> {
        let name = name.trim();
        if name.is_empty() || name.eq_ignore_ascii_case("local") {
            return Ok(Self::Local);
        }
        name.parse::<Tz>().map(Self::Named).map_err(|_| {
            format!("Unknown time zone `{name}`. Use an IANA name such as Asia/Kolkata, or `local`")
        })
    }

    fn now(self) -> DateTime<FixedOffset> {
        self.at(Utc::now())
    }

    fn at(self, instant: DateTime<Utc>) -> DateTime<FixedOffset> {
        match self {
            Self::Local => instant.with_timezone(&Local).fixed_offset(),
            Self::Named(zone) => instant.with_timezone(&zone).fixed_offset(),
        }
    }

    fn localize(self, naive: NaiveDateTime) -> Result<DateTime<FixedOffset>, String> {
        match self {
            Self::Local => {
                single(Local.from_local_datetime(&naive)).map(|value| value.fixed_offset())
            }
            Self::Named(zone) => {
                single(zone.from_local_datetime(&naive)).map(|value| value.fixed_offset())
            }
        }
    }

    fn label(self) -> String {
        match self {
            Self::Local => "local time".into(),
            Self::Named(zone) => zone.name().into(),
        }
    }
}

fn single<T: TimeZone>(result: LocalResult<DateTime<T>>) -> Result<DateTime<T>, String> {
    match result {
        LocalResult::Single(value) => Ok(value),
        LocalResult::Ambiguous(_, _) => Err("That local time happens twice because of a daylight-saving change; add a UTC offset such as +01:00".into()),
        LocalResult::None => Err("That local time does not exist because of a daylight-saving change".into()),
    }
}

fn parse_moment(text: &str, zone: Zone) -> Result<Moment, String> {
    let text = text.trim();
    match text.to_ascii_lowercase().as_str() {
        "" | "now" => {
            return Ok(Moment {
                at: zone.now(),
                date_only: false,
            });
        }
        "today" => {
            let date = zone.now().date_naive();
            return Ok(Moment {
                at: zone.localize(midnight(date))?,
                date_only: true,
            });
        }
        _ => {}
    }
    if let Ok(value) = DateTime::parse_from_rfc3339(text) {
        return Ok(Moment {
            at: value,
            date_only: false,
        });
    }
    for format in [
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M",
    ] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(text, format) {
            return Ok(Moment {
                at: zone.localize(naive)?,
                date_only: false,
            });
        }
    }
    for format in [
        "%Y-%m-%d",
        "%d %b %Y",
        "%d %B %Y",
        "%b %d %Y",
        "%B %d %Y",
        "%b %d, %Y",
        "%B %d, %Y",
    ] {
        if let Ok(date) = NaiveDate::parse_from_str(text, format) {
            return Ok(Moment {
                at: zone.localize(midnight(date))?,
                date_only: true,
            });
        }
    }
    Err(format!(
        "Could not read `{text}` as a date. Use a form like 2026-10-02, 2026-10-02 14:30, 2 Oct 2026, now, or today"
    ))
}

fn midnight(date: NaiveDate) -> NaiveDateTime {
    date.and_hms_opt(0, 0, 0).expect("midnight is valid")
}

fn readable(moment: &DateTime<FixedOffset>, date_only: bool) -> String {
    if date_only {
        moment.format("%A, %-d %B %Y").to_string()
    } else {
        moment
            .format("%A, %-d %B %Y, %H:%M:%S (UTC%:z)")
            .to_string()
    }
}

fn plural(count: i64, unit: &str) -> String {
    format!("{count} {unit}{}", if count == 1 { "" } else { "s" })
}

pub(super) fn time_zones(request: &ToolRequest, text: &str) -> Result<Value, String> {
    let source = Zone::parse(option_str(request, "sourceTimeZone", "local"))?;
    let targets = option_str(request, "timeZones", "UTC")
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();
    if targets.is_empty() || targets.len() > 8 {
        return Err("Choose between one and eight target time zones".into());
    }
    let moment = parse_moment(text, source)?;
    let instant = moment.at.with_timezone(&Utc);
    let conversions = targets
        .iter()
        .map(|name| {
            let zone = Zone::parse(name)?;
            let local = zone.at(instant);
            Ok(json!({
                "timeZone": zone.label(),
                "time": local.format("%a %-d %b %Y, %H:%M").to_string(),
                "offset": format!("UTC{}", local.format("%:z")),
                "iso": local.to_rfc3339_opts(SecondsFormat::Secs, true),
            }))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(json!({
        "headline": format!("{} in {}", readable(&moment.at, false), source.label()),
        "instantUtc": instant.to_rfc3339_opts(SecondsFormat::Secs, true),
        "conversions": conversions,
    }))
}

pub(super) fn date_calculator(request: &ToolRequest, text: &str) -> Result<Value, String> {
    let zone = Zone::parse(option_str(request, "timeZone", "local"))?;
    match option_str(request, "action", "difference") {
        "difference" => difference(text, option_str(request, "end", "now"), zone),
        action @ ("add" | "subtract") => shift(request, text, action, zone),
        "timestamp" => timestamp(text, zone),
        other => Err(format!("Unknown action `{other}`")),
    }
}

fn difference(start: &str, end: &str, zone: Zone) -> Result<Value, String> {
    let start = parse_moment(start, zone)?;
    let end = parse_moment(if end.trim().is_empty() { "now" } else { end }, zone)?;
    let negative = end.at < start.at;
    let (from, to) = if negative {
        (&end, &start)
    } else {
        (&start, &end)
    };
    let (years, months, days) = calendar_span(from.at, to.at);
    let total = to.at.signed_duration_since(from.at);
    let total_days = to
        .at
        .date_naive()
        .signed_duration_since(from.at.date_naive())
        .num_days();
    let mut parts = Vec::new();
    for (count, unit) in [(years, "year"), (months, "month"), (days, "day")] {
        if count > 0 {
            parts.push(plural(count, unit));
        }
    }
    let date_only = start.date_only && end.date_only;
    if !date_only {
        let rest = total - Duration::days(total.num_days());
        if rest.num_hours() > 0 {
            parts.push(plural(rest.num_hours(), "hour"));
        }
        if rest.num_minutes() % 60 > 0 {
            parts.push(plural(rest.num_minutes() % 60, "minute"));
        }
    }
    let span = if parts.is_empty() {
        "0 days".to_owned()
    } else {
        parts.join(", ")
    };
    Ok(json!({
        "headline": format!("{span}{}", if negative { " earlier" } else { "" }),
        "from": readable(&start.at, start.date_only),
        "to": readable(&end.at, end.date_only),
        "totalDays": if negative { -total_days } else { total_days },
        "weeks": format!("{} and {}", plural(total_days / 7, "week"), plural(total_days % 7, "day")),
        "weekdays": weekdays_between(from.at.date_naive(), to.at.date_naive()),
        "totalHours": total.num_hours(),
        "totalMinutes": total.num_minutes(),
        "totalSeconds": total.num_seconds(),
    }))
}

/// Whole years, months, and days from `from` to `to` (`from <= to`).
fn calendar_span(from: DateTime<FixedOffset>, to: DateTime<FixedOffset>) -> (i64, i64, i64) {
    let (a, b) = (from.date_naive(), to.date_naive());
    let mut months =
        i64::from(b.year() - a.year()) * 12 + i64::from(b.month()) - i64::from(a.month());
    if b.day() < a.day() {
        months -= 1;
    }
    let months = months.max(0);
    let anchor = a
        .checked_add_months(Months::new(months as u32))
        .unwrap_or(b);
    let days = b.signed_duration_since(anchor).num_days().max(0);
    (months / 12, months % 12, days)
}

/// Monday-to-Friday days from `start` (inclusive) to `end` (exclusive).
fn weekdays_between(start: NaiveDate, end: NaiveDate) -> i64 {
    let total = end.signed_duration_since(start).num_days();
    let full_weeks = total / 7;
    let mut count = full_weeks * 5;
    let mut day = start + Duration::days(full_weeks * 7);
    while day < end {
        if !matches!(day.weekday(), Weekday::Sat | Weekday::Sun) {
            count += 1;
        }
        day += Duration::days(1);
    }
    count
}

fn shift(request: &ToolRequest, text: &str, action: &str, zone: Zone) -> Result<Value, String> {
    let start = parse_moment(text, zone)?;
    let amount = option_f64(request, "amount")
        .filter(|value| value.fract() == 0.0 && value.abs() <= 1_000_000.0)
        .ok_or("Enter a whole number between -1,000,000 and 1,000,000")? as i64;
    let amount = if action == "subtract" {
        -amount
    } else {
        amount
    };
    let unit = option_str(request, "unit", "days");
    let result = match unit {
        "months" | "years" => {
            let months = if unit == "years" { amount * 12 } else { amount };
            let step = Months::new(months.unsigned_abs() as u32);
            if months >= 0 {
                start.at.checked_add_months(step)
            } else {
                start.at.checked_sub_months(step)
            }
        }
        _ => {
            let duration = match unit {
                "seconds" => Duration::try_seconds(amount),
                "minutes" => Duration::try_minutes(amount),
                "hours" => Duration::try_hours(amount),
                "days" => Duration::try_days(amount),
                "weeks" => Duration::try_weeks(amount),
                other => return Err(format!("Unknown unit `{other}`")),
            }
            .ok_or("That duration is too large")?;
            start.at.checked_add_signed(duration)
        }
    }
    .ok_or("The result is outside the supported date range")?;
    let date_only = start.date_only && matches!(unit, "days" | "weeks" | "months" | "years");
    Ok(json!({
        "headline": readable(&result, date_only),
        "start": readable(&start.at, start.date_only),
        "change": format!("{}{}", if amount < 0 { "−" } else { "+" }, plural(amount.abs(), unit.trim_end_matches('s'))),
        "iso": if date_only { result.date_naive().to_string() } else { result.to_rfc3339_opts(SecondsFormat::Secs, true) },
    }))
}

fn timestamp(text: &str, zone: Zone) -> Result<Value, String> {
    let text = text.trim();
    let digits = text.trim_start_matches('-');
    if !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()) {
        let number: i64 = text.parse().map_err(|_| "That timestamp is too large")?;
        // Pick the unit from the digit count: 10 digits are seconds, 13 milliseconds, and so on.
        let (unit, nanos) = match digits.len() {
            0..=11 => ("seconds", i128::from(number) * 1_000_000_000),
            12..=14 => ("milliseconds", i128::from(number) * 1_000_000),
            15..=17 => ("microseconds", i128::from(number) * 1_000),
            _ => ("nanoseconds", i128::from(number)),
        };
        let seconds =
            i64::try_from(nanos.div_euclid(1_000_000_000)).map_err(|_| "Timestamp out of range")?;
        let instant =
            DateTime::<Utc>::from_timestamp(seconds, nanos.rem_euclid(1_000_000_000) as u32)
                .ok_or("Timestamp is outside the supported date range")?;
        let local = zone.at(instant);
        return Ok(json!({
            "headline": readable(&local, false),
            "readAs": format!("Unix time in {unit}"),
            "utc": instant.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
            "iso": local.to_rfc3339_opts(SecondsFormat::Secs, true),
            "relative": relative(instant),
        }));
    }
    let moment = parse_moment(text, zone)?;
    let instant = moment.at.with_timezone(&Utc);
    Ok(json!({
        "headline": instant.timestamp().to_string(),
        "date": readable(&moment.at, moment.date_only),
        "unixSeconds": instant.timestamp(),
        "unixMilliseconds": instant.timestamp_millis(),
        "relative": relative(instant),
    }))
}

fn relative(instant: DateTime<Utc>) -> String {
    let delta = instant.signed_duration_since(Utc::now());
    let seconds = delta.num_seconds().abs();
    let (count, unit) = match seconds {
        0..60 => return "just now".into(),
        60..3_600 => (seconds / 60, "minute"),
        3_600..86_400 => (seconds / 3_600, "hour"),
        86_400..2_629_746 => (seconds / 86_400, "day"),
        2_629_746..31_556_952 => (seconds / 2_629_746, "month"),
        _ => (seconds / 31_556_952, "year"),
    };
    if delta.num_seconds() < 0 {
        format!("{} ago", plural(count, unit))
    } else {
        format!("in {}", plural(count, unit))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc() -> Zone {
        Zone::parse("UTC").unwrap()
    }

    #[test]
    fn differences_use_calendar_months_and_weekdays() {
        let value = difference("2024-01-31", "2025-03-15", utc()).unwrap();
        assert_eq!(value["headline"], "1 year, 1 month, 15 days");
        assert_eq!(value["totalDays"], 409);
        let value = difference("2026-10-05", "2026-10-12", utc()).unwrap();
        assert_eq!(value["weekdays"], 5);
        let value = difference("2026-10-12", "2026-10-05", utc()).unwrap();
        assert_eq!(value["headline"], "7 days earlier");
        assert_eq!(value["totalDays"], -7);
    }

    #[test]
    fn timestamps_convert_both_ways() {
        let value = timestamp("1735689600", utc()).unwrap();
        assert_eq!(value["utc"], "2025-01-01 00:00:00 UTC");
        let value = timestamp("1735689600000", utc()).unwrap();
        assert_eq!(value["readAs"], "Unix time in milliseconds");
        let value = timestamp("2025-01-01", utc()).unwrap();
        assert_eq!(value["unixSeconds"], 1735689600);
    }

    #[test]
    fn nonexistent_local_times_are_rejected() {
        let zone = Zone::parse("America/New_York").unwrap();
        assert!(parse_moment("2025-03-09 02:30", zone).is_err());
    }
}
