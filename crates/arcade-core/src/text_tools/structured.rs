//! JSON, YAML, TOML, and XML conversion through one ordered data model.
//!
//! Keys and elements keep their source order. Values a target format can't
//! hold (TOML has no null, XML has no arrays) are reported with their path
//! instead of being dropped silently.

use serde_json::{Map, Value};

#[derive(Debug)]
pub(super) struct Converted {
    pub text: String,
    pub from: &'static str,
    pub to: &'static str,
    pub warnings: Vec<String>,
}

/// Convert `input` to `to` (or reformat it when `to` is `same`).
pub(super) fn convert(
    input: &str,
    from: &str,
    to: &str,
    indent: &str,
) -> Result<Converted, String> {
    let from = match from {
        "auto" => detect(input)?,
        "json" => "json",
        "yaml" => "yaml",
        "toml" => "toml",
        "xml" => "xml",
        other => return Err(format!("Unsupported source format: {other}")),
    };
    let to = match to {
        "same" => from,
        "json" => "json",
        "yaml" => "yaml",
        "toml" => "toml",
        "xml" => "xml",
        other => return Err(format!("Unsupported target format: {other}")),
    };
    let indent = Indent::parse(indent)?;
    if from == "xml" && to == "xml" {
        return Ok(Converted {
            text: reindent_xml(input, indent)?,
            from,
            to,
            warnings: Vec::new(),
        });
    }
    let mut warnings = Vec::new();
    let value = parse(input, from, &mut warnings)?;
    let text = render(&value, to, indent)?;
    if matches!(from, "yaml" | "toml") {
        warnings.push(format!("{} comments are not carried over.", label(from)));
    }
    if from == "xml" && to != "xml" {
        warnings.push("XML attributes become \"@name\" keys and mixed text becomes \"#text\"; namespaces and comments are not carried over.".into());
    }
    Ok(Converted {
        text,
        from,
        to,
        warnings,
    })
}

/// Re-indent (or minify) XML event by event, so comments, processing
/// instructions, namespaces, and mixed text survive untouched.
fn reindent_xml(input: &str, indent: Indent) -> Result<String, String> {
    use quick_xml::{Reader, Writer, events::Event};
    roxmltree::Document::parse(input).map_err(|error| format!("XML: {error}"))?;
    let mut reader = Reader::from_str(input);
    let mut writer = match indent {
        Indent::Spaces(count) => Writer::new_with_indent(Vec::new(), b' ', count),
        Indent::Tab => Writer::new_with_indent(Vec::new(), b'\t', 1),
        Indent::Minified => Writer::new(Vec::new()),
    };
    loop {
        match reader.read_event() {
            Ok(Event::Eof) => break,
            Ok(Event::Text(text)) if text.trim().is_empty() => {}
            Ok(event) => writer
                .write_event(event)
                .map_err(|error| format!("XML: {error}"))?,
            Err(error) => return Err(format!("XML byte {}: {error}", reader.error_position())),
        }
    }
    let mut text = String::from_utf8(writer.into_inner()).map_err(|error| error.to_string())?;
    text.push('\n');
    Ok(text)
}

fn label(format: &str) -> &'static str {
    match format {
        "json" => "JSON",
        "yaml" => "YAML",
        "toml" => "TOML",
        _ => "XML",
    }
}

/// Guess the source format from its shape, then confirm by parsing.
fn detect(input: &str) -> Result<&'static str, String> {
    let trimmed = input.trim_start_matches('\u{feff}').trim_start();
    if trimmed.is_empty() {
        return Err("Paste some structured data to convert".into());
    }
    if trimmed.starts_with('<') {
        return Ok("xml");
    }
    if (trimmed.starts_with('{') || trimmed.starts_with('['))
        && serde_json::from_str::<Value>(trimmed).is_ok()
    {
        return Ok("json");
    }
    if toml::from_str::<toml::Table>(trimmed).is_ok() {
        return Ok("toml");
    }
    if yaml_rust2::YamlLoader::load_from_str(trimmed).is_ok() {
        return Ok("yaml");
    }
    Err("Couldn't tell which format this is. Choose the source format.".into())
}

fn parse(input: &str, format: &str, warnings: &mut Vec<String>) -> Result<Value, String> {
    let input = input.trim_start_matches('\u{feff}');
    match format {
        "json" => serde_json::from_str(input).map_err(|error| format!("JSON: {error}")),
        "yaml" => {
            let documents = yaml_rust2::YamlLoader::load_from_str(input)
                .map_err(|error| format!("YAML: {error}"))?;
            match documents.len() {
                0 => Ok(Value::Null),
                1 => yaml_to_json(&documents[0], "$"),
                count => {
                    warnings.push(format!(
                        "The YAML had {count} documents; they were combined into one list."
                    ));
                    documents
                        .iter()
                        .enumerate()
                        .map(|(index, document)| yaml_to_json(document, &format!("$[{index}]")))
                        .collect::<Result<Vec<_>, _>>()
                        .map(Value::Array)
                }
            }
        }
        "toml" => {
            let table: toml::Table =
                toml::from_str(input).map_err(|error| format!("TOML: {error}"))?;
            Ok(toml_to_json(toml::Value::Table(table)))
        }
        "xml" => xml_to_json(input),
        other => Err(format!("Unsupported source format: {other}")),
    }
}

fn yaml_to_json(value: &yaml_rust2::Yaml, path: &str) -> Result<Value, String> {
    use yaml_rust2::Yaml;
    match value {
        Yaml::Null => Ok(Value::Null),
        Yaml::Boolean(value) => Ok(Value::Bool(*value)),
        Yaml::Integer(value) => Ok(Value::from(*value)),
        Yaml::Real(text) => {
            let number = text
                .parse::<f64>()
                .map_err(|_| format!("YAML number at {path} is invalid"))?;
            serde_json::Number::from_f64(number)
                .map(Value::Number)
                .ok_or_else(|| format!("{path} is {text}, which JSON can't represent"))
        }
        Yaml::String(value) => Ok(Value::String(value.clone())),
        Yaml::Array(values) => values
            .iter()
            .enumerate()
            .map(|(index, value)| yaml_to_json(value, &format!("{path}[{index}]")))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Yaml::Hash(values) => {
            let mut object = Map::new();
            for (key, value) in values {
                let key = match key {
                    Yaml::String(key) => key.clone(),
                    Yaml::Integer(key) => key.to_string(),
                    Yaml::Boolean(key) => key.to_string(),
                    Yaml::Real(key) => key.clone(),
                    _ => {
                        return Err(format!(
                            "A key at {path} is a list or mapping, which other formats can't use as a key"
                        ));
                    }
                };
                let child = yaml_to_json(value, &format!("{path}.{key}"))?;
                object.insert(key, child);
            }
            Ok(Value::Object(object))
        }
        Yaml::Alias(_) | Yaml::BadValue => Err(format!("The YAML value at {path} is invalid")),
    }
}

fn toml_to_json(value: toml::Value) -> Value {
    match value {
        toml::Value::String(text) => Value::String(text),
        toml::Value::Integer(number) => Value::from(number),
        toml::Value::Float(number) => serde_json::Number::from_f64(number)
            .map(Value::Number)
            .unwrap_or_else(|| Value::String(number.to_string())),
        toml::Value::Boolean(flag) => Value::Bool(flag),
        // Dates keep their exact TOML spelling.
        toml::Value::Datetime(datetime) => Value::String(datetime.to_string()),
        toml::Value::Array(items) => Value::Array(items.into_iter().map(toml_to_json).collect()),
        toml::Value::Table(table) => Value::Object(
            table
                .into_iter()
                .map(|(key, value)| (key, toml_to_json(value)))
                .collect(),
        ),
    }
}

fn xml_to_json(input: &str) -> Result<Value, String> {
    let document = roxmltree::Document::parse_with_options(
        input,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            ..Default::default()
        },
    )
    .map_err(|error| format!("XML: {error}"))?;
    let root = document.root_element();
    let mut object = Map::new();
    object.insert(root.tag_name().name().to_owned(), xml_element_to_json(root));
    Ok(Value::Object(object))
}

fn xml_element_to_json(node: roxmltree::Node<'_, '_>) -> Value {
    let mut object = Map::new();
    for attribute in node.attributes() {
        object.insert(
            format!("@{}", attribute.name()),
            Value::String(attribute.value().to_owned()),
        );
    }
    // Repeated elements become lists, at the position of their first occurrence.
    let mut children: Vec<(String, Vec<Value>)> = Vec::new();
    for child in node.children().filter(roxmltree::Node::is_element) {
        let name = child.tag_name().name().to_owned();
        let value = xml_element_to_json(child);
        match children.iter_mut().find(|(existing, _)| *existing == name) {
            Some((_, values)) => values.push(value),
            None => children.push((name, vec![value])),
        }
    }
    for (name, mut values) in children {
        object.insert(
            name,
            if values.len() == 1 {
                values.remove(0)
            } else {
                Value::Array(values)
            },
        );
    }
    let text = node
        .children()
        .filter(|child| child.is_text())
        .filter_map(|child| child.text())
        .collect::<String>();
    let text = text.trim();
    if object.is_empty() {
        Value::String(text.to_owned())
    } else {
        if !text.is_empty() {
            object.insert("#text".into(), Value::String(text.to_owned()));
        }
        Value::Object(object)
    }
}

#[derive(Clone, Copy)]
enum Indent {
    Spaces(usize),
    Tab,
    Minified,
}

impl Indent {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "" | "2" => Ok(Self::Spaces(2)),
            "4" => Ok(Self::Spaces(4)),
            "tab" => Ok(Self::Tab),
            "minify" => Ok(Self::Minified),
            other => Err(format!("Unknown indentation: {other}")),
        }
    }

    fn unit(self) -> String {
        match self {
            Self::Spaces(count) => " ".repeat(count),
            Self::Tab => "\t".into(),
            Self::Minified => String::new(),
        }
    }
}

fn render(value: &Value, format: &str, indent: Indent) -> Result<String, String> {
    match format {
        "json" => {
            if let Indent::Minified = indent {
                return serde_json::to_string(value).map_err(|error| error.to_string());
            }
            let unit = indent.unit();
            let mut bytes = Vec::new();
            let formatter = serde_json::ser::PrettyFormatter::with_indent(unit.as_bytes());
            let mut serializer = serde_json::Serializer::with_formatter(&mut bytes, formatter);
            serde::Serialize::serialize(value, &mut serializer)
                .map_err(|error| error.to_string())?;
            let mut text = String::from_utf8(bytes).map_err(|error| error.to_string())?;
            text.push('\n');
            Ok(text)
        }
        "yaml" => {
            let yaml = json_to_yaml(value);
            let mut rendered = String::new();
            yaml_rust2::YamlEmitter::new(&mut rendered)
                .dump(&yaml)
                .map_err(|error| error.to_string())?;
            let mut text = rendered
                .strip_prefix("---\n")
                .or_else(|| rendered.strip_prefix("--- "))
                .unwrap_or(&rendered)
                .to_owned();
            text.push('\n');
            Ok(text)
        }
        "toml" => {
            let Value::Object(object) = value else {
                return Err(
                    "TOML needs a table at the top level; wrap the data in an object with a key"
                        .into(),
                );
            };
            if let Some(path) = find_null(value, "") {
                return Err(format!(
                    "TOML has no null value, and {} is null. Remove it or give it a value.",
                    path.trim_start_matches('.')
                ));
            }
            let mut table = toml::Table::new();
            for (key, child) in object {
                table.insert(key.clone(), json_to_toml(child));
            }
            toml::to_string_pretty(&table).map_err(|error| format!("TOML: {error}"))
        }
        "xml" => json_to_xml(value, indent),
        other => Err(format!("Unsupported target format: {other}")),
    }
}

fn find_null(value: &Value, path: &str) -> Option<String> {
    match value {
        Value::Null => Some(if path.is_empty() {
            "the value".into()
        } else {
            path.into()
        }),
        Value::Array(items) => items
            .iter()
            .enumerate()
            .find_map(|(index, item)| find_null(item, &format!("{path}[{index}]"))),
        Value::Object(object) => object
            .iter()
            .find_map(|(key, item)| find_null(item, &format!("{path}.{key}"))),
        _ => None,
    }
}

fn json_to_toml(value: &Value) -> toml::Value {
    match value {
        Value::Null => toml::Value::String(String::new()),
        Value::Bool(flag) => toml::Value::Boolean(*flag),
        Value::Number(number) => number
            .as_i64()
            .map(toml::Value::Integer)
            .or_else(|| number.as_f64().map(toml::Value::Float))
            .unwrap_or_else(|| toml::Value::String(number.to_string())),
        Value::String(text) => toml::Value::String(text.clone()),
        Value::Array(items) => toml::Value::Array(items.iter().map(json_to_toml).collect()),
        Value::Object(object) => toml::Value::Table(
            object
                .iter()
                .map(|(key, item)| (key.clone(), json_to_toml(item)))
                .collect(),
        ),
    }
}

fn json_to_yaml(value: &Value) -> yaml_rust2::Yaml {
    use yaml_rust2::{Yaml, yaml::Hash};
    match value {
        Value::Null => Yaml::Null,
        Value::Bool(flag) => Yaml::Boolean(*flag),
        Value::Number(number) => number
            .as_i64()
            .map(Yaml::Integer)
            .unwrap_or_else(|| Yaml::Real(number.to_string())),
        Value::String(text) => Yaml::String(text.clone()),
        Value::Array(items) => Yaml::Array(items.iter().map(json_to_yaml).collect()),
        Value::Object(object) => {
            let mut hash = Hash::new();
            for (key, item) in object {
                hash.insert(Yaml::String(key.clone()), json_to_yaml(item));
            }
            Yaml::Hash(hash)
        }
    }
}

fn json_to_xml(value: &Value, indent: Indent) -> Result<String, String> {
    let mut writer = XmlWriter {
        output: String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"),
        unit: indent.unit(),
        pretty: !matches!(indent, Indent::Minified),
    };
    match value {
        Value::Object(object)
            if object.len() == 1 && !object.keys().any(|key| key.starts_with('@')) =>
        {
            let (name, child) = object.iter().next().expect("one entry");
            writer.element(name, child, 0)?;
        }
        _ => writer.element("root", value, 0)?,
    }
    writer.output.push('\n');
    Ok(writer.output)
}

struct XmlWriter {
    output: String,
    unit: String,
    pretty: bool,
}

impl XmlWriter {
    fn newline(&mut self, depth: usize) {
        if self.pretty {
            self.output.push('\n');
            self.output.push_str(&self.unit.repeat(depth));
        }
    }

    fn element(&mut self, name: &str, value: &Value, depth: usize) -> Result<(), String> {
        if !valid_xml_name(name) {
            return Err(format!("\"{name}\" can't be used as an XML element name"));
        }
        self.newline(depth);
        match value {
            Value::Object(object) => {
                self.output.push('<');
                self.output.push_str(name);
                for (key, item) in object {
                    let Some(attribute) = key.strip_prefix('@') else {
                        continue;
                    };
                    if !valid_xml_name(attribute) {
                        return Err(format!(
                            "\"{attribute}\" can't be used as an XML attribute name"
                        ));
                    }
                    let text = match item {
                        Value::String(text) => text.clone(),
                        Value::Number(_) | Value::Bool(_) => item.to_string(),
                        _ => {
                            return Err(format!(
                                "The XML attribute \"{attribute}\" must be a single value"
                            ));
                        }
                    };
                    self.output.push(' ');
                    self.output.push_str(attribute);
                    self.output.push_str("=\"");
                    self.output.push_str(&quick_xml::escape::escape(&text));
                    self.output.push('"');
                }
                let children: Vec<_> = object
                    .iter()
                    .filter(|(key, _)| !key.starts_with('@') && key.as_str() != "#text")
                    .collect();
                let text = match object.get("#text") {
                    Some(Value::String(text)) => Some(text.clone()),
                    Some(Value::Null) | None => None,
                    Some(other) => Some(other.to_string()),
                };
                if children.is_empty() && text.is_none() {
                    self.output.push_str("/>");
                    return Ok(());
                }
                self.output.push('>');
                if let Some(text) = &text {
                    self.output.push_str(&quick_xml::escape::escape(text));
                }
                for (key, child) in &children {
                    match child {
                        Value::Array(items) => {
                            for item in items {
                                self.element(key, item, depth + 1)?;
                            }
                        }
                        _ => self.element(key, child, depth + 1)?,
                    }
                }
                if !children.is_empty() {
                    self.newline(depth);
                }
                self.output.push_str("</");
                self.output.push_str(name);
                self.output.push('>');
            }
            Value::Array(items) => {
                self.output.push('<');
                self.output.push_str(name);
                self.output.push('>');
                for item in items {
                    self.element("item", item, depth + 1)?;
                }
                if !items.is_empty() {
                    self.newline(depth);
                }
                self.output.push_str("</");
                self.output.push_str(name);
                self.output.push('>');
            }
            Value::Null => {
                self.output.push('<');
                self.output.push_str(name);
                self.output.push_str("/>");
            }
            scalar => {
                let text = match scalar {
                    Value::String(text) => text.clone(),
                    other => other.to_string(),
                };
                self.output.push('<');
                self.output.push_str(name);
                self.output.push('>');
                self.output.push_str(&quick_xml::escape::escape(&text));
                self.output.push_str("</");
                self.output.push_str(name);
                self.output.push('>');
            }
        }
        Ok(())
    }
}

fn valid_xml_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_alphabetic() || first == '_')
        && chars
            .all(|character| character.is_alphanumeric() || matches!(character, '_' | '-' | '.'))
        && !name.to_ascii_lowercase().starts_with("xml")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(input: &str, from: &str, to: &str) -> String {
        convert(input, from, to, "2").unwrap().text
    }

    #[test]
    fn keys_and_elements_keep_their_order() {
        let yaml = "zeta: 1\nalpha: 2\nmid:\n  b: true\n  a: false\n";
        assert_eq!(
            run(yaml, "yaml", "json"),
            "{\n  \"zeta\": 1,\n  \"alpha\": 2,\n  \"mid\": {\n    \"b\": true,\n    \"a\": false\n  }\n}\n"
        );
        let xml = "<cfg><z>1</z><a>2</a><z>3</z></cfg>";
        let json = run(xml, "xml", "json");
        assert!(
            json.find("\"z\"").unwrap() < json.find("\"a\"").unwrap(),
            "{json}"
        );
        assert!(
            json.contains("[\n      \"1\",\n      \"3\"\n    ]"),
            "{json}"
        );
    }

    #[test]
    fn toml_dates_nulls_and_detection() {
        let toml = "title = \"x\"\n[owner]\nborn = 1979-05-27T07:32:00Z\n";
        let converted = convert(toml, "auto", "json", "2").unwrap();
        assert_eq!(converted.from, "toml");
        assert!(
            converted
                .text
                .contains("\"born\": \"1979-05-27T07:32:00Z\""),
            "{}",
            converted.text
        );
        let error = convert("{\"a\": {\"b\": null}}", "auto", "toml", "2").unwrap_err();
        assert!(error.contains("a.b is null"), "{error}");
        assert_eq!(convert("a: 1", "auto", "json", "2").unwrap().from, "yaml");
        assert_eq!(convert("[1, 2]", "auto", "yaml", "2").unwrap().from, "json");
        assert_eq!(run("{\"a\":1}", "json", "same"), "{\n  \"a\": 1\n}\n");
        assert_eq!(
            convert("{\"a\": 1}", "auto", "same", "minify")
                .unwrap()
                .text,
            "{\"a\":1}"
        );
        assert!(
            convert("{\"a\":", "json", "same", "2")
                .unwrap_err()
                .contains("line 1")
        );
    }

    #[test]
    fn xml_reformat_keeps_comments_and_mixed_text() {
        let xml = "<a><!-- note --><p>Hello <b>world</b>!</p></a>";
        let pretty = run(xml, "xml", "same");
        assert!(pretty.contains("<!-- note -->"), "{pretty}");
        assert!(pretty.contains("\n  <p>"), "{pretty}");
        let text = roxmltree::Document::parse(&pretty)
            .unwrap()
            .descendants()
            .filter(|node| node.is_text())
            .filter_map(|node| node.text())
            .filter(|text| !text.trim().is_empty())
            .collect::<String>();
        assert_eq!(text, "Hello world!");
        assert!(convert("<a/><b/>", "xml", "same", "2").is_err());
    }

    #[test]
    fn xml_output_is_indented_and_round_trips_attributes() {
        let json =
            r#"{"book": {"@id": "7", "title": "Dune", "tags": ["sf", "classic"], "note": null}}"#;
        let xml = run(json, "json", "xml");
        assert_eq!(
            xml,
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<book id=\"7\">\n  <title>Dune</title>\n  <tags>sf</tags>\n  <tags>classic</tags>\n  <note/>\n</book>\n"
        );
        let back = run(&xml, "xml", "json");
        assert!(back.contains("\"@id\": \"7\""), "{back}");
        assert_eq!(
            convert(json, "json", "xml", "minify")
                .unwrap()
                .text
                .lines()
                .count(),
            1
        );
        assert_eq!(run("[1, 2]", "json", "yaml"), "- 1\n- 2\n");
    }
}
