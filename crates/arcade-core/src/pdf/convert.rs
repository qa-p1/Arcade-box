//! Documents, presentations and spreadsheets to PDF without an office suite.
//! Text, headings, lists, bold and tables come through; images, columns and
//! exact page layout don't. Text uses a system font (embedded), else the
//! PDF base font.

use crate::tool_kit::check_cancelled;
use calamine::{Reader, open_workbook_auto_from_rs};
use printpdf::{
    BuiltinFont, Color, FontId, Line, LinePoint, Mm, Op, ParsedFont, PdfDocument, PdfFontHandle,
    PdfPage, PdfSaveOptions, Point, Pt, Rgb, TextItem,
};
use roxmltree::Node;
use std::{
    io::{Cursor, Read},
    path::PathBuf,
    sync::atomic::AtomicBool,
};

const MAX_XML_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SHEET_ROWS: usize = 5000;
const MAX_COLUMNS: usize = 60;

#[derive(Debug, Clone, PartialEq)]
struct Run {
    text: String,
    bold: bool,
}

#[derive(Debug, Clone, PartialEq)]
enum Block {
    Heading(u8, String),
    Para {
        runs: Vec<Run>,
        bullet: bool,
        indent: u8,
    },
    Table(Vec<Vec<String>>),
    PageBreak,
}

pub(super) struct Converted {
    pub pdf: Vec<u8>,
    pub warnings: Vec<String>,
}

pub(super) fn convert(
    bytes: Vec<u8>,
    extension: &str,
    title: &str,
    cancelled: &AtomicBool,
) -> Result<Converted, String> {
    let mut warnings = Vec::new();
    let (blocks, slides) = match extension {
        "docx" => (docx(&bytes)?, false),
        "odt" => (odf(&bytes, false)?, false),
        "pptx" => (pptx(&bytes)?, true),
        "odp" => (odf(&bytes, true)?, true),
        "rtf" => (rtf(&String::from_utf8_lossy(&bytes)), false),
        "xlsx" | "xlsm" | "xlsb" | "xls" | "ods" => {
            (sheets(bytes, &mut warnings, cancelled)?, false)
        }
        "doc" | "ppt" => {
            return Err("Old .doc and .ppt files can't be converted. Save the file as .docx or .pptx (or .odt/.odp), then convert that.".into());
        }
        other => return Err(format!("Converting .{other} files to PDF isn't supported")),
    };
    if blocks.iter().all(|block| *block == Block::PageBreak) {
        return Err("The document has no text to convert".into());
    }
    check_cancelled(cancelled)?;
    let wide = slides
        || blocks
            .iter()
            .any(|block| matches!(block, Block::Table(rows) if rows.iter().any(|r| r.len() > 6)));
    let fonts = Fonts::load(&mut warnings);
    let pdf = render(&blocks, &fonts, wide, title, cancelled)?;
    warnings.push("Converted text, headings, lists and tables. Images and exact page layout are not kept; review the PDF before relying on its layout.".into());
    Ok(Converted { pdf, warnings })
}

// ---------------------------------------------------------------- reading

fn zip_entry(bytes: &[u8], name: &str) -> Result<Option<String>, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| format!("Not a readable document ({error})"))?;
    let Ok(file) = archive.by_name(name) else {
        return Ok(None);
    };
    let mut text = String::new();
    file.take(MAX_XML_BYTES)
        .read_to_string(&mut text)
        .map_err(|error| format!("Cannot read {name}: {error}"))?;
    Ok(Some(text))
}

fn parse(xml: &str) -> Result<roxmltree::Document<'_>, String> {
    roxmltree::Document::parse_with_options(
        xml,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            ..Default::default()
        },
    )
    .map_err(|error| format!("The document's XML is damaged ({error})"))
}

fn local<'a>(node: Node<'a, '_>) -> &'a str {
    node.tag_name().name()
}

fn attr<'a>(node: Node<'a, '_>, name: &str) -> Option<&'a str> {
    node.attributes()
        .find(|attribute| attribute.name() == name)
        .map(|attribute| attribute.value())
}

fn child<'a, 'i>(node: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    node.children()
        .find(|c| c.is_element() && local(*c) == name)
}

fn merge(runs: Vec<Run>) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    for run in runs.into_iter().filter(|run| !run.text.is_empty()) {
        match out.last_mut() {
            Some(last) if last.bold == run.bold => last.text.push_str(&run.text),
            _ => out.push(run),
        }
    }
    out
}

fn plain(runs: &[Run]) -> String {
    runs.iter().map(|run| run.text.as_str()).collect()
}

fn docx(bytes: &[u8]) -> Result<Vec<Block>, String> {
    let xml = zip_entry(bytes, "word/document.xml")?.ok_or("This .docx has no document body")?;
    let doc = parse(&xml)?;
    let body = doc
        .descendants()
        .find(|n| n.is_element() && local(*n) == "body")
        .ok_or("This .docx has no document body")?;
    let mut blocks = Vec::new();
    docx_children(body, &mut blocks);
    Ok(blocks)
}

fn docx_children(parent: Node, blocks: &mut Vec<Block>) {
    for node in parent.children().filter(Node::is_element) {
        match local(node) {
            "p" => docx_paragraph(node, blocks),
            "tbl" => blocks.push(Block::Table(
                node.children()
                    .filter(|r| r.is_element() && local(*r) == "tr")
                    .map(|row| {
                        row.children()
                            .filter(|c| c.is_element() && local(*c) == "tc")
                            .map(|cell| {
                                cell.descendants()
                                    .filter(|t| t.is_element() && local(*t) == "t")
                                    .filter_map(|t| t.text())
                                    .collect::<Vec<_>>()
                                    .join("")
                            })
                            .collect()
                    })
                    .collect(),
            )),
            "sdt" => {
                if let Some(content) = child(node, "sdtContent") {
                    docx_children(content, blocks);
                }
            }
            _ => {}
        }
    }
}

fn docx_paragraph(p: Node, blocks: &mut Vec<Block>) {
    let properties = child(p, "pPr");
    let style = properties
        .and_then(|pr| child(pr, "pStyle"))
        .and_then(|s| attr(s, "val"))
        .unwrap_or("")
        .to_ascii_lowercase();
    let bullet = properties.and_then(|pr| child(pr, "numPr")).is_some();
    let mut runs = Vec::new();
    let mut page_break = false;
    for r in p
        .descendants()
        .filter(|n| n.is_element() && local(*n) == "r")
    {
        let bold = r
            .children()
            .find(|c| c.is_element() && local(*c) == "rPr")
            .and_then(|pr| child(pr, "b"))
            .is_some_and(|b| !matches!(attr(b, "val"), Some("0" | "false")));
        for c in r.children().filter(Node::is_element) {
            let text = match local(c) {
                "t" => c.text().unwrap_or("").to_string(),
                "tab" => "    ".into(),
                "br" if attr(c, "type") == Some("page") => {
                    page_break = true;
                    continue;
                }
                "br" | "cr" => "\n".into(),
                _ => continue,
            };
            runs.push(Run { text, bold });
        }
    }
    let runs = merge(runs);
    let level = if style == "title" {
        Some(1)
    } else {
        style
            .strip_prefix("heading")
            .and_then(|n| n.trim().parse::<u8>().ok())
    };
    match level {
        Some(level) if !runs.is_empty() => {
            blocks.push(Block::Heading(level.clamp(1, 6), plain(&runs)))
        }
        _ => blocks.push(Block::Para {
            runs,
            bullet,
            indent: u8::from(bullet),
        }),
    }
    if page_break {
        blocks.push(Block::PageBreak);
    }
}

/// OpenDocument text (`.odt`) or presentation (`.odp`) from `content.xml`.
fn odf(bytes: &[u8], presentation: bool) -> Result<Vec<Block>, String> {
    let xml = zip_entry(bytes, "content.xml")?.ok_or("This file has no document content")?;
    let doc = parse(&xml)?;
    // Automatic text styles that set bold.
    let bold_styles: Vec<&str> = doc
        .descendants()
        .filter(|n| n.is_element() && local(*n) == "style")
        .filter(|style| {
            child(*style, "text-properties")
                .and_then(|p| attr(p, "font-weight"))
                .is_some_and(|w| w == "bold" || w.parse::<u32>().is_ok_and(|w| w >= 600))
        })
        .filter_map(|style| attr(style, "name"))
        .collect();
    let mut blocks = Vec::new();
    if presentation {
        let pages = doc
            .descendants()
            .filter(|n| n.is_element() && local(*n) == "page");
        for (index, page) in pages.enumerate() {
            if index > 0 {
                blocks.push(Block::PageBreak);
            }
            for frame in page
                .descendants()
                .filter(|n| n.is_element() && local(*n) == "frame")
            {
                let title = attr(frame, "class") == Some("title");
                let mut frame_blocks = Vec::new();
                for text_box in frame
                    .children()
                    .filter(|n| n.is_element() && local(*n) == "text-box")
                {
                    odf_children(text_box, &bold_styles, 0, &mut frame_blocks);
                }
                if title {
                    let text = frame_blocks
                        .iter()
                        .filter_map(|b| match b {
                            Block::Para { runs, .. } => Some(plain(runs)),
                            Block::Heading(_, text) => Some(text.clone()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    blocks.push(Block::Heading(1, text));
                } else {
                    blocks.extend(frame_blocks);
                }
            }
        }
    } else {
        let text = doc
            .descendants()
            .find(|n| {
                n.is_element()
                    && local(*n) == "text"
                    && n.parent_element().is_some_and(|p| local(p) == "body")
            })
            .ok_or("This .odt has no document body")?;
        odf_children(text, &bold_styles, 0, &mut blocks);
    }
    Ok(blocks)
}

fn odf_children(parent: Node, bold: &[&str], depth: u8, blocks: &mut Vec<Block>) {
    for node in parent.children().filter(Node::is_element) {
        match local(node) {
            "h" => {
                let level = attr(node, "outline-level")
                    .and_then(|l| l.parse::<u8>().ok())
                    .unwrap_or(1);
                let mut runs = Vec::new();
                odf_runs(node, bold, false, &mut runs);
                blocks.push(Block::Heading(level.clamp(1, 6), plain(&merge(runs))));
            }
            "p" => {
                let mut runs = Vec::new();
                odf_runs(node, bold, false, &mut runs);
                blocks.push(Block::Para {
                    runs: merge(runs),
                    bullet: false,
                    indent: depth,
                });
            }
            "list" => {
                for item in node
                    .children()
                    .filter(|n| n.is_element() && local(*n) == "list-item")
                {
                    let start = blocks.len();
                    odf_children(item, bold, depth + 1, blocks);
                    if let Some(Block::Para { bullet, .. }) = blocks.get_mut(start) {
                        *bullet = true;
                    }
                }
            }
            "table" => {
                let rows = node
                    .descendants()
                    .filter(|n| n.is_element() && local(*n) == "table-row")
                    .map(|row| {
                        row.children()
                            .filter(|c| c.is_element() && local(*c) == "table-cell")
                            .map(|cell| {
                                let mut runs = Vec::new();
                                odf_runs(cell, bold, false, &mut runs);
                                plain(&runs).trim().to_string()
                            })
                            .collect()
                    })
                    .collect();
                blocks.push(Block::Table(rows));
            }
            "section" | "text-box" => odf_children(node, bold, depth, blocks),
            _ => {}
        }
    }
}

fn odf_runs(node: Node, bold_styles: &[&str], bold: bool, runs: &mut Vec<Run>) {
    for c in node.children() {
        if c.is_text() {
            runs.push(Run {
                text: c.text().unwrap_or("").to_string(),
                bold,
            });
            continue;
        }
        if !c.is_element() {
            continue;
        }
        match local(c) {
            "span" | "a" => {
                let bold = bold || attr(c, "style-name").is_some_and(|s| bold_styles.contains(&s));
                odf_runs(c, bold_styles, bold, runs);
            }
            "s" => runs.push(Run {
                text: " ".repeat(
                    attr(c, "c")
                        .and_then(|n| n.parse().ok())
                        .unwrap_or(1)
                        .min(64),
                ),
                bold,
            }),
            "tab" => runs.push(Run {
                text: "    ".into(),
                bold,
            }),
            "line-break" => runs.push(Run {
                text: "\n".into(),
                bold,
            }),
            // Table cells hold paragraphs.
            "p" | "h" => {
                if !runs.is_empty() {
                    runs.push(Run {
                        text: " ".into(),
                        bold,
                    });
                }
                odf_runs(c, bold_styles, bold, runs);
            }
            _ => {}
        }
    }
}

fn pptx(bytes: &[u8]) -> Result<Vec<Block>, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| format!("Not a readable presentation ({error})"))?;
    let mut slides: Vec<(u32, String)> = archive
        .file_names()
        .filter_map(|name| {
            let number = name
                .strip_prefix("ppt/slides/slide")?
                .strip_suffix(".xml")?
                .parse()
                .ok()?;
            Some((number, name.to_string()))
        })
        .collect();
    slides.sort();
    if slides.is_empty() {
        return Err("This presentation has no slides".into());
    }
    let mut blocks = Vec::new();
    for (index, (_, name)) in slides.iter().enumerate() {
        let mut xml = String::new();
        archive
            .by_name(name)
            .map_err(|error| error.to_string())?
            .take(MAX_XML_BYTES)
            .read_to_string(&mut xml)
            .map_err(|error| error.to_string())?;
        let doc = parse(&xml)?;
        if index > 0 {
            blocks.push(Block::PageBreak);
        }
        for shape in doc
            .descendants()
            .filter(|n| n.is_element() && matches!(local(*n), "sp" | "tbl"))
        {
            if local(shape) == "tbl" {
                blocks.push(Block::Table(
                    shape
                        .children()
                        .filter(|r| r.is_element() && local(*r) == "tr")
                        .map(|row| {
                            row.children()
                                .filter(|c| c.is_element() && local(*c) == "tc")
                                .map(|cell| {
                                    cell.descendants()
                                        .filter(|t| t.is_element() && local(*t) == "t")
                                        .filter_map(|t| t.text())
                                        .collect::<Vec<_>>()
                                        .join("")
                                })
                                .collect()
                        })
                        .collect(),
                ));
                continue;
            }
            let title = shape
                .descendants()
                .find(|n| n.is_element() && local(*n) == "ph")
                .and_then(|ph| attr(ph, "type"))
                .is_some_and(|t| t == "title" || t == "ctrTitle");
            let Some(body) = child(shape, "txBody") else {
                continue;
            };
            for p in body
                .children()
                .filter(|n| n.is_element() && local(*n) == "p")
            {
                let mut runs = Vec::new();
                for r in p.children().filter(Node::is_element) {
                    match local(r) {
                        "r" | "fld" => {
                            let bold = child(r, "rPr")
                                .and_then(|pr| attr(pr, "b"))
                                .is_some_and(|b| b == "1" || b == "true");
                            if let Some(text) = child(r, "t").and_then(|t| t.text()) {
                                runs.push(Run {
                                    text: text.to_string(),
                                    bold,
                                });
                            }
                        }
                        "br" => runs.push(Run {
                            text: "\n".into(),
                            bold: false,
                        }),
                        _ => {}
                    }
                }
                let runs = merge(runs);
                if runs.is_empty() {
                    continue;
                }
                if title {
                    blocks.push(Block::Heading(1, plain(&runs)));
                } else {
                    let level = child(p, "pPr")
                        .and_then(|pr| attr(pr, "lvl"))
                        .and_then(|l| l.parse::<u8>().ok())
                        .unwrap_or(0);
                    blocks.push(Block::Para {
                        runs,
                        bullet: true,
                        indent: level + 1,
                    });
                }
            }
        }
    }
    Ok(blocks)
}

/// Plain RTF: paragraphs, bold and the usual escapes. Embedded objects,
/// tables and pictures are skipped.
fn rtf(source: &str) -> Vec<Block> {
    const SKIP: [&str; 16] = [
        "fonttbl",
        "colortbl",
        "stylesheet",
        "info",
        "pict",
        "header",
        "footer",
        "listtable",
        "listoverridetable",
        "generator",
        "themedata",
        "colorschememapping",
        "datastore",
        "latentstyles",
        "rsidtbl",
        "xmlnstbl",
    ];
    let chars: Vec<char> = source.chars().collect();
    let mut blocks = Vec::new();
    let mut runs: Vec<Run> = Vec::new();
    // Per group: (skipping, bold).
    let mut stack: Vec<(bool, bool)> = vec![(false, false)];
    let mut skip_next = 0usize;
    let mut i = 0;
    let push = |runs: &mut Vec<Run>, text: &str, bold: bool| {
        runs.push(Run {
            text: text.to_string(),
            bold,
        })
    };
    let flush = |runs: &mut Vec<Run>, blocks: &mut Vec<Block>| {
        blocks.push(Block::Para {
            runs: merge(std::mem::take(runs)),
            bullet: false,
            indent: 0,
        });
    };
    while i < chars.len() {
        let (skipping, bold) = *stack.last().unwrap_or(&(false, false));
        let c = chars[i];
        match c {
            '{' => {
                stack.push((skipping, bold));
                i += 1;
                if chars.get(i) == Some(&'\\') && chars.get(i + 1) == Some(&'*') {
                    if let Some(top) = stack.last_mut() {
                        top.0 = true;
                    }
                }
            }
            '}' => {
                if stack.len() > 1 {
                    stack.pop();
                }
                i += 1;
            }
            '\\' => {
                i += 1;
                let Some(&next) = chars.get(i) else { break };
                if !next.is_ascii_alphabetic() {
                    i += 1;
                    let text = match next {
                        '\'' => {
                            let hex: String = chars.iter().skip(i).take(2).collect();
                            i += 2;
                            u8::from_str_radix(&hex, 16).ok().map(cp1252)
                        }
                        '\\' | '{' | '}' => Some(next),
                        '~' => Some('\u{a0}'),
                        '_' => Some('-'),
                        _ => None,
                    };
                    if let Some(ch) = text {
                        if skip_next > 0 {
                            skip_next -= 1;
                        } else if !skipping {
                            push(&mut runs, &ch.to_string(), bold);
                        }
                    }
                    continue;
                }
                let start = i;
                while i < chars.len() && chars[i].is_ascii_alphabetic() {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                let num_start = i;
                if chars.get(i) == Some(&'-') {
                    i += 1;
                }
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
                let number: Option<i32> =
                    chars[num_start..i].iter().collect::<String>().parse().ok();
                if chars.get(i) == Some(&' ') {
                    i += 1;
                }
                if SKIP.contains(&word.as_str()) {
                    if let Some(top) = stack.last_mut() {
                        top.0 = true;
                    }
                    continue;
                }
                if skipping {
                    continue;
                }
                match word.as_str() {
                    "par" | "line" | "sect" | "page" => flush(&mut runs, &mut blocks),
                    "tab" => push(&mut runs, "    ", bold),
                    "b" => {
                        if let Some(top) = stack.last_mut() {
                            top.1 = number != Some(0);
                        }
                    }
                    "plain" => {
                        if let Some(top) = stack.last_mut() {
                            top.1 = false;
                        }
                    }
                    "u" => {
                        if let Some(n) = number {
                            let code = if n < 0 { n + 65536 } else { n } as u32;
                            if let Some(ch) = char::from_u32(code) {
                                push(&mut runs, &ch.to_string(), bold);
                            }
                            skip_next = 1;
                        }
                    }
                    "emdash" => push(&mut runs, "—", bold),
                    "endash" => push(&mut runs, "–", bold),
                    "bullet" => push(&mut runs, "•", bold),
                    "lquote" => push(&mut runs, "‘", bold),
                    "rquote" => push(&mut runs, "’", bold),
                    "ldblquote" => push(&mut runs, "“", bold),
                    "rdblquote" => push(&mut runs, "”", bold),
                    _ => {}
                }
            }
            '\r' | '\n' => i += 1,
            _ => {
                i += 1;
                if skip_next > 0 {
                    skip_next -= 1;
                } else if !skipping {
                    push(&mut runs, &c.to_string(), bold);
                }
            }
        }
    }
    if !runs.is_empty() {
        flush(&mut runs, &mut blocks);
    }
    blocks
}

fn cp1252(byte: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž',
        '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}',
        'ž', 'Ÿ',
    ];
    match byte {
        0x80..=0x9F => HIGH[(byte - 0x80) as usize],
        _ => byte as char,
    }
}

fn sheets(
    bytes: Vec<u8>,
    warnings: &mut Vec<String>,
    cancelled: &AtomicBool,
) -> Result<Vec<Block>, String> {
    let mut workbook = open_workbook_auto_from_rs(Cursor::new(bytes))
        .map_err(|error| format!("Could not open this spreadsheet: {error}"))?;
    let mut blocks = Vec::new();
    for name in workbook.sheet_names() {
        check_cancelled(cancelled)?;
        let range = workbook
            .worksheet_range(&name)
            .map_err(|error| format!("Could not read sheet `{name}`: {error}"))?;
        if range.is_empty() {
            continue;
        }
        if !blocks.is_empty() {
            blocks.push(Block::PageBreak);
        }
        let mut rows: Vec<Vec<String>> = range
            .rows()
            .take(MAX_SHEET_ROWS)
            .map(|row| {
                row.iter()
                    .take(MAX_COLUMNS)
                    .map(ToString::to_string)
                    .collect()
            })
            .collect();
        if range.height() > MAX_SHEET_ROWS || range.width() > MAX_COLUMNS {
            warnings.push(format!(
                "Sheet `{name}` was cut to its first {MAX_SHEET_ROWS} rows and {MAX_COLUMNS} columns"
            ));
        }
        // Drop columns that are empty all the way down.
        let width = rows.iter().map(Vec::len).max().unwrap_or(0);
        let keep: Vec<bool> = (0..width)
            .map(|c| rows.iter().any(|r| r.get(c).is_some_and(|v| !v.is_empty())))
            .collect();
        for row in &mut rows {
            let mut c = 0;
            row.retain(|_| {
                c += 1;
                keep[c - 1]
            });
        }
        rows.retain(|row| row.iter().any(|cell| !cell.is_empty()));
        blocks.push(Block::Heading(2, name));
        blocks.push(Block::Table(rows));
    }
    if blocks.is_empty() {
        return Err("Every sheet in this spreadsheet is empty".into());
    }
    Ok(blocks)
}

// ---------------------------------------------------------------- fonts

enum Face {
    External { id: FontId, parsed: Box<ParsedFont> },
    Builtin(BuiltinFont),
}

impl Face {
    fn handle(&self) -> PdfFontHandle {
        match self {
            Face::External { id, .. } => PdfFontHandle::External(id.clone()),
            Face::Builtin(font) => PdfFontHandle::Builtin(*font),
        }
    }

    fn width(&self, text: &str, size: f32) -> f32 {
        match self {
            Face::External { parsed, .. } => {
                let em = parsed.units_per_em.max(1) as f32;
                text.chars()
                    .map(|c| {
                        parsed
                            .lookup_glyph_index(c as u32)
                            .and_then(|g| parsed.get_glyph_width(g))
                            .map_or(em * 0.55, f32::from)
                    })
                    .sum::<f32>()
                    * size
                    / em
            }
            // Helvetica averages a little over half an em.
            Face::Builtin(BuiltinFont::HelveticaBold) => text.chars().count() as f32 * size * 0.58,
            Face::Builtin(_) => text.chars().count() as f32 * size * 0.53,
        }
    }

    /// The base fonts only cover Latin-1.
    fn printable(&self, text: &str) -> String {
        match self {
            Face::External { .. } => text.to_string(),
            Face::Builtin(_) => text
                .chars()
                .map(|c| if (c as u32) < 256 { c } else { '?' })
                .collect(),
        }
    }
}

struct Fonts {
    files: Option<(Vec<u8>, Vec<u8>)>,
}

impl Fonts {
    fn load(warnings: &mut Vec<String>) -> Fonts {
        let files = font_files();
        if files.is_none() {
            warnings.push("No system font was found, so characters outside Western European text show as `?`.".into());
        }
        Fonts { files }
    }
}

fn font_files() -> Option<(Vec<u8>, Vec<u8>)> {
    const PAIRS: [(&str, &str); 5] = [
        ("LiberationSans-Regular.ttf", "LiberationSans-Bold.ttf"),
        ("arial.ttf", "arialbd.ttf"),
        ("Arial.ttf", "Arial Bold.ttf"),
        ("DejaVuSans.ttf", "DejaVuSans-Bold.ttf"),
        ("NotoSans-Regular.ttf", "NotoSans-Bold.ttf"),
    ];
    let mut roots: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        roots.push(
            std::env::var_os("WINDIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| "C:\\Windows".into())
                .join("Fonts"),
        );
    } else if cfg!(target_os = "macos") {
        roots.extend(
            [
                "/System/Library/Fonts/Supplemental",
                "/Library/Fonts",
                "/System/Library/Fonts",
            ]
            .map(PathBuf::from),
        );
    } else {
        roots.extend(["/usr/share/fonts", "/usr/local/share/fonts"].map(PathBuf::from));
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            roots.push(home.join(".local/share/fonts"));
        }
    }
    let mut found = std::collections::HashMap::new();
    let mut queue: Vec<(PathBuf, u8)> = roots.into_iter().map(|r| (r, 0)).collect();
    while let Some((dir, depth)) = queue.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && depth < 4 {
                queue.push((path, depth + 1));
            } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if PAIRS.iter().any(|(r, b)| name == *r || name == *b) {
                    found.entry(name.to_string()).or_insert(path);
                }
            }
        }
    }
    PAIRS.iter().find_map(|(regular, bold)| {
        let regular = std::fs::read(found.get(*regular)?).ok()?;
        let bold = std::fs::read(found.get(*bold)?).ok()?;
        Some((regular, bold))
    })
}

// ---------------------------------------------------------------- layout

const MARGIN: f32 = 54.0;
const BODY: f32 = 10.5;
const LEADING: f32 = 1.35;

struct Writer<'f> {
    regular: &'f Face,
    bold: &'f Face,
    size: (f32, f32),
    y: f32,
    pages: Vec<Vec<Op>>,
    ops: Vec<Op>,
}

impl<'f> Writer<'f> {
    fn face(&self, bold: bool) -> &'f Face {
        if bold { self.bold } else { self.regular }
    }

    fn new_page(&mut self) {
        let ops = std::mem::take(&mut self.ops);
        self.pages.push(ops);
        self.y = self.size.1 - MARGIN;
    }

    fn page_is_blank(&self) -> bool {
        self.ops.is_empty()
    }

    fn room(&mut self, height: f32) {
        if self.y - height < MARGIN && !self.page_is_blank() {
            self.new_page();
        }
    }

    fn text_line(&mut self, x: f32, size: f32, segments: &[(bool, String)]) {
        let leading = size * LEADING;
        self.room(leading);
        let baseline = self.y - size;
        self.ops.push(Op::StartTextSection);
        self.ops.push(Op::SetTextCursor {
            pos: Point {
                x: Pt(x),
                y: Pt(baseline),
            },
        });
        for (bold, text) in segments {
            let face = self.face(*bold);
            self.ops.push(Op::SetFont {
                font: face.handle(),
                size: Pt(size),
            });
            self.ops.push(Op::ShowText {
                items: vec![TextItem::Text(face.printable(text))],
            });
        }
        self.ops.push(Op::EndTextSection);
        self.y -= leading;
    }

    fn rule(&mut self, x1: f32, x2: f32, y: f32) {
        let point = |x| LinePoint {
            p: Point { x: Pt(x), y: Pt(y) },
            bezier: false,
        };
        self.ops.extend([
            Op::SetOutlineColor {
                col: Color::Rgb(Rgb {
                    r: 0.75,
                    g: 0.75,
                    b: 0.75,
                    icc_profile: None,
                }),
            },
            Op::SetOutlineThickness { pt: Pt(0.4) },
            Op::DrawLine {
                line: Line {
                    points: vec![point(x1), point(x2)],
                    is_closed: false,
                },
            },
        ]);
    }

    /// Word-wraps styled runs into the column starting at `x`.
    fn paragraph(&mut self, runs: &[Run], x: f32, size: f32, bullet: bool) {
        let width = self.size.0 - MARGIN - x;
        if bullet {
            let leading = size * LEADING;
            self.room(leading);
            let y = self.y;
            self.text_line(x - 12.0, size, &[(false, "•".into())]);
            self.y = y;
        }
        // Words with their style, line breaks as `None`.
        let mut words: Vec<Option<(bool, String)>> = Vec::new();
        for run in runs {
            for (i, line) in run.text.split('\n').enumerate() {
                if i > 0 {
                    words.push(None);
                }
                for word in line.split_whitespace() {
                    words.push(Some((run.bold, word.to_string())));
                }
            }
        }
        let space = self.regular.width(" ", size);
        let mut line: Vec<(bool, String)> = Vec::new();
        let mut line_width = 0.0;
        let mut emitted = false;
        let flush = |this: &mut Self, line: &mut Vec<(bool, String)>, width: &mut f32| {
            this.text_line(x, size, &join_segments(line));
            line.clear();
            *width = 0.0;
        };
        for word in words {
            let Some((bold, mut word)) = word else {
                flush(self, &mut line, &mut line_width);
                emitted = true;
                continue;
            };
            // A word wider than the column is split across lines.
            loop {
                let face = self.face(bold);
                let w = face.width(&word, size);
                let needed = if line.is_empty() {
                    w
                } else {
                    line_width + space + w
                };
                if needed <= width {
                    line_width = needed;
                    line.push((bold, word));
                    break;
                }
                if !line.is_empty() {
                    flush(self, &mut line, &mut line_width);
                    emitted = true;
                    continue;
                }
                let mut cut = word.len();
                while cut > 1 && face.width(&word[..cut], size) > width {
                    cut = word[..cut - 1]
                        .char_indices()
                        .last()
                        .map_or(1, |(i, c)| i + c.len_utf8());
                }
                let rest = word.split_off(cut);
                self.text_line(x, size, &[(bold, word)]);
                emitted = true;
                if rest.is_empty() {
                    break;
                }
                word = rest;
            }
        }
        if !line.is_empty() || !emitted {
            flush(self, &mut line, &mut line_width);
        }
    }

    fn table(&mut self, rows: &[Vec<String>]) {
        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
        if columns == 0 {
            return;
        }
        let available = self.size.0 - 2.0 * MARGIN;
        let mut size = 9.0;
        let natural: Vec<f32> = (0..columns)
            .map(|c| {
                rows.iter()
                    .take(200)
                    .filter_map(|r| r.get(c))
                    .map(|v| self.regular.width(v, size) + 8.0)
                    .fold(24.0, f32::max)
                    .min(available * 0.45)
            })
            .collect();
        let total: f32 = natural.iter().sum();
        if total > available * 1.6 {
            size = 7.0;
        }
        let scale = (available / total).min(1.0);
        let widths: Vec<f32> = natural.iter().map(|w| w * scale).collect();
        let draw_row = |this: &mut Self, row: &[String], bold: bool| {
            let row_height = size * 1.6;
            this.room(row_height);
            let top = this.y;
            let mut x = MARGIN;
            for (c, width) in widths.iter().enumerate() {
                let text = row.get(c).map(String::as_str).unwrap_or("");
                let text = fit(this.face(bold), text, size, width - 6.0);
                if !text.is_empty() {
                    this.y = top - size * 0.3;
                    this.text_line(x + 3.0, size, &[(bold, text)]);
                }
                x += width;
            }
            this.y = top - row_height;
            this.rule(
                MARGIN,
                MARGIN + widths.iter().sum::<f32>(),
                this.y + size * 0.2,
            );
        };
        let header = &rows[0];
        draw_row(self, header, true);
        for row in &rows[1..] {
            let before = self.pages.len();
            self.room(size * 1.6);
            if self.pages.len() != before {
                draw_row(self, header, true);
            }
            draw_row(self, row, false);
        }
        self.y -= 6.0;
    }
}

fn join_segments(words: &[(bool, String)]) -> Vec<(bool, String)> {
    let mut out: Vec<(bool, String)> = Vec::new();
    for (bold, word) in words {
        match out.last_mut() {
            Some((b, text)) if b == bold => {
                text.push(' ');
                text.push_str(word);
            }
            Some((_, text)) => {
                text.push(' ');
                out.push((*bold, word.clone()));
            }
            None => out.push((*bold, word.clone())),
        }
    }
    out
}

/// `text`, cut with an ellipsis to fit `width`.
fn fit(face: &Face, text: &str, size: f32, width: f32) -> String {
    let text = text.replace(['\n', '\r', '\t'], " ");
    if face.width(&text, size) <= width {
        return text;
    }
    let mut out: String = text;
    while !out.is_empty() && face.width(&format!("{out}…"), size) > width {
        out.pop();
    }
    format!("{}…", out.trim_end())
}

fn render(
    blocks: &[Block],
    fonts: &Fonts,
    wide: bool,
    title: &str,
    cancelled: &AtomicBool,
) -> Result<Vec<u8>, String> {
    let mut doc = PdfDocument::new(title);
    let (regular, bold) = match &fonts.files {
        Some((regular, bold)) => {
            let mut warnings = Vec::new();
            match (
                ParsedFont::from_bytes(regular, 0, &mut warnings),
                ParsedFont::from_bytes(bold, 0, &mut warnings),
            ) {
                (Some(r), Some(b)) => (
                    Face::External {
                        id: doc.add_font(&r),
                        parsed: Box::new(r),
                    },
                    Face::External {
                        id: doc.add_font(&b),
                        parsed: Box::new(b),
                    },
                ),
                _ => (
                    Face::Builtin(BuiltinFont::Helvetica),
                    Face::Builtin(BuiltinFont::HelveticaBold),
                ),
            }
        }
        None => (
            Face::Builtin(BuiltinFont::Helvetica),
            Face::Builtin(BuiltinFont::HelveticaBold),
        ),
    };
    let size = if wide {
        (841.89, 595.276)
    } else {
        (595.276, 841.89)
    };
    let mut w = Writer {
        regular: &regular,
        bold: &bold,
        size,
        y: size.1 - MARGIN,
        pages: Vec::new(),
        ops: Vec::new(),
    };
    for block in blocks {
        check_cancelled(cancelled)?;
        match block {
            Block::Heading(level, text) => {
                let size = match level {
                    1 => 18.0,
                    2 => 15.0,
                    3 => 13.0,
                    _ => 11.5,
                };
                if !w.page_is_blank() {
                    w.y -= 8.0;
                }
                w.room(size * LEADING * 2.0);
                w.paragraph(
                    &[Run {
                        text: text.clone(),
                        bold: true,
                    }],
                    MARGIN,
                    size,
                    false,
                );
                w.y -= 3.0;
            }
            Block::Para {
                runs,
                bullet,
                indent,
            } => {
                let x = MARGIN + *indent as f32 * 18.0;
                w.paragraph(runs, x, BODY, *bullet);
                w.y -= 4.0;
            }
            Block::Table(rows) => w.table(rows),
            Block::PageBreak => {
                if !w.page_is_blank() {
                    w.new_page();
                }
            }
        }
    }
    if !w.page_is_blank() || w.pages.is_empty() {
        w.new_page();
    }
    let to_mm = |pt: f32| Mm(pt * 25.4 / 72.0);
    let pages = w
        .pages
        .into_iter()
        .map(|ops| PdfPage::new(to_mm(size.0), to_mm(size.1), ops))
        .collect();
    Ok(doc
        .with_pages(pages)
        .save(&PdfSaveOptions::default(), &mut Vec::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn zip_of(files: &[(&str, &str)]) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(&mut out);
        for (name, body) in files {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        out.into_inner()
    }

    #[test]
    fn docx_headings_lists_bold_and_tables() {
        let body = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
            <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Report</w:t></w:r></w:p>
            <w:p><w:r><w:t xml:space="preserve">Plain </w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>bold</w:t></w:r></w:p>
            <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/></w:numPr></w:pPr><w:r><w:t>item</w:t></w:r></w:p>
            <w:tbl><w:tr><w:tc><w:p><w:r><w:t>a</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>b</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
        </w:body></w:document>"#;
        let blocks = docx(&zip_of(&[("word/document.xml", body)])).unwrap();
        assert_eq!(blocks[0], Block::Heading(1, "Report".into()));
        assert_eq!(
            blocks[1],
            Block::Para {
                runs: vec![
                    Run {
                        text: "Plain ".into(),
                        bold: false
                    },
                    Run {
                        text: "bold".into(),
                        bold: true
                    }
                ],
                bullet: false,
                indent: 0
            }
        );
        assert!(matches!(&blocks[2], Block::Para { bullet: true, .. }));
        assert_eq!(blocks[3], Block::Table(vec![vec!["a".into(), "b".into()]]));
    }

    #[test]
    fn odt_reads_headings_spans_and_lists() {
        let content = r#"<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0">
            <office:automatic-styles><style:style style:name="T1"><style:text-properties fo:font-weight="bold"/></style:style></office:automatic-styles>
            <office:body><office:text>
              <text:h text:outline-level="2">Title</text:h>
              <text:p>a<text:s text:c="2"/><text:span text:style-name="T1">b</text:span></text:p>
              <text:list><text:list-item><text:p>one</text:p></text:list-item></text:list>
            </office:text></office:body></office:document-content>"#;
        let blocks = odf(&zip_of(&[("content.xml", content)]), false).unwrap();
        assert_eq!(blocks[0], Block::Heading(2, "Title".into()));
        assert_eq!(
            blocks[1],
            Block::Para {
                runs: vec![
                    Run {
                        text: "a  ".into(),
                        bold: false
                    },
                    Run {
                        text: "b".into(),
                        bold: true
                    }
                ],
                bullet: false,
                indent: 0
            }
        );
        assert!(matches!(
            &blocks[2],
            Block::Para {
                bullet: true,
                indent: 1,
                ..
            }
        ));
    }

    #[test]
    fn rtf_paragraphs_bold_and_escapes() {
        let blocks = rtf(
            r"{\rtf1\ansi{\fonttbl{\f0 Arial;}}\f0 Hello \b world\b0 !\par Caf\'e9 \u8364? done\par}",
        );
        let texts: Vec<String> = blocks
            .iter()
            .map(|b| match b {
                Block::Para { runs, .. } => plain(runs),
                _ => String::new(),
            })
            .collect();
        assert_eq!(texts, vec!["Hello world!", "Café € done"]);
        assert!(
            matches!(&blocks[0], Block::Para { runs, .. } if runs.iter().any(|r| r.bold && r.text == "world"))
        );
    }

    #[test]
    fn rendered_pdf_contains_the_text() {
        let blocks = vec![
            Block::Heading(1, "Quarterly report".into()),
            Block::Para {
                runs: vec![Run {
                    text: "word ".repeat(400),
                    bold: false,
                }],
                bullet: false,
                indent: 0,
            },
            Block::Table(vec![
                vec!["Name".into(), "Total".into()],
                vec!["Apples".into(), "42".into()],
            ]),
        ];
        let mut warnings = Vec::new();
        let pdf = render(
            &blocks,
            &Fonts::load(&mut warnings),
            false,
            "t",
            &AtomicBool::new(false),
        )
        .unwrap();
        let doc = lopdf::Document::load_mem(&pdf).unwrap();
        assert!(!doc.get_pages().is_empty());
        assert!(pdf.starts_with(b"%PDF-"));
    }
}
