//! Fill PDF forms (AcroForm). "List" reports every field with its type,
//! current value, and choices; "fill" sets values and asks viewers to redraw
//! the fields. Values come as `Field name = value` lines or a JSON object.

use super::{output_name, selected_pdf, success_file};
use crate::{
    artifacts::publish_without_overwrite,
    grants::FileGrants,
    tool_kit::{check_cancelled, read_limited},
};
use arcade_contract::{ToolManifest, ToolRequest, ToolResult, ToolValue};
use lopdf::{Dictionary, Document, Object, ObjectId, decode_text_string, text_string};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashSet},
    fs::File,
    sync::atomic::AtomicBool,
};

const MAX_PDF_BYTES: usize = 200 * 1024 * 1024;
const MAX_FIELDS: usize = 2000;

#[derive(Debug)]
struct Field {
    id: ObjectId,
    name: String,
    kind: &'static str,
    value: String,
    options: Vec<String>,
    read_only: bool,
    required: bool,
    widgets: Vec<ObjectId>,
}

pub(super) fn fill(
    manifest: &ToolManifest,
    request: &ToolRequest,
    grants: &FileGrants,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let source = selected_pdf(request, grants)?;
    let file = File::open(&source).map_err(|error| error.to_string())?;
    let bytes = read_limited(file, MAX_PDF_BYTES, cancelled)?;
    let mut document =
        Document::load_mem(&bytes).map_err(|error| format!("Could not read this PDF: {error}"))?;
    if document.is_encrypted() {
        return Err("This PDF is password-protected. Unlock it with Protect PDF first.".into());
    }
    let fields = collect_fields(&document)?;
    if fields.is_empty() {
        return Err("This PDF has no fillable form fields".into());
    }
    let values = request
        .options
        .get("values")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if values.is_empty() || request.options.get("action").and_then(Value::as_str) == Some("list") {
        return Ok(list_result(manifest, &fields));
    }
    let wanted = parse_values(values)?;
    let mut unknown = Vec::new();
    let mut filled = 0;
    for (name, value) in &wanted {
        check_cancelled(cancelled)?;
        let Some(field) = fields
            .iter()
            .find(|field| field.name == *name || field.name.eq_ignore_ascii_case(name))
        else {
            unknown.push(name.clone());
            continue;
        };
        if field.read_only {
            return Err(format!("`{}` is read-only", field.name));
        }
        set_value(&mut document, field, value)?;
        filled += 1;
    }
    if filled == 0 {
        return Err(format!(
            "None of the names matched a field. Fields in this form: {}",
            fields
                .iter()
                .take(20)
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    // Ask viewers to rebuild field appearances from the new values.
    let form_id = document
        .catalog()
        .ok()
        .and_then(|catalog| catalog.get(b"AcroForm").ok())
        .and_then(|form| form.as_reference().ok());
    match form_id {
        Some(id) => document
            .get_dictionary_mut(id)
            .map_err(|error| error.to_string())?
            .set("NeedAppearances", true),
        None => {
            if let Ok(Object::Dictionary(form)) = document
                .catalog_mut()
                .map_err(|error| error.to_string())?
                .get_mut(b"AcroForm")
            {
                form.set("NeedAppearances", true);
            }
        }
    }
    let parent = source.parent().ok_or("Selected PDF has no parent folder")?;
    let temp = tempfile::Builder::new()
        .prefix(".arcade-pdf-")
        .tempdir_in(parent)
        .map_err(|error| format!("Cannot create a private PDF workspace: {error}"))?;
    let staged = temp.path().join("filled.pdf");
    document
        .save(&staged)
        .map_err(|error| format!("Could not write the filled PDF: {error}"))?;
    let name = output_name(request, &source, "-filled.pdf")?;
    let final_path = publish_without_overwrite(&staged, parent, &name, cancelled)
        .map_err(|error| format!("Could not save the filled PDF: {error}"))?;
    let selected = grants
        .grant(&final_path)
        .map_err(|error| error.to_string())?;
    let mut result = success_file(
        manifest,
        selected,
        &format!(
            "Filled {filled} field{}",
            if filled == 1 { "" } else { "s" }
        ),
    );
    if !unknown.is_empty() {
        result
            .warnings
            .push(format!("No field named: {}", unknown.join(", ")));
    }
    result
        .warnings
        .push("Some viewers only show the new values after the form is opened for editing.".into());
    Ok(result)
}

fn list_result(manifest: &ToolManifest, fields: &[Field]) -> ToolResult {
    let template = fields
        .iter()
        .filter(|field| !field.read_only && field.kind != "signature")
        .map(|field| format!("{} = {}", field.name, field.value))
        .collect::<Vec<_>>()
        .join("; ");
    let report = json!({
        "count": fields.len(),
        "fields": fields.iter().map(|field| json!({
            "name": field.name,
            "type": field.kind,
            "value": field.value,
            "choices": if field.options.is_empty() { Value::Null } else { json!(field.options.join(", ")) },
            "required": field.required,
            "readOnly": field.read_only,
        })).collect::<Vec<_>>(),
    });
    ToolResult {
        tool_id: manifest.id.clone(),
        status: arcade_contract::ResultStatus::Success,
        outputs: vec![
            ToolValue::text(report.to_string(), "structured/pdf-form"),
            ToolValue::text(template, "text/plain"),
        ],
        message: Some(format!(
            "Found {} fields. Copy the template, edit the values, paste it into Values, and run again.",
            fields.len()
        )),
        warnings: vec![],
        metadata: Default::default(),
    }
}

/// `name = value` lines (or `;`-separated pairs), or a JSON object.
fn parse_values(text: &str) -> Result<BTreeMap<String, String>, String> {
    if text.starts_with('{') {
        let object: serde_json::Map<String, Value> = serde_json::from_str(text)
            .map_err(|error| format!("Values JSON is invalid: {error}"))?;
        return Ok(object
            .into_iter()
            .map(|(key, value)| {
                let value = match value {
                    Value::String(text) => text,
                    Value::Bool(flag) => if flag { "yes" } else { "no" }.into(),
                    other => other.to_string(),
                };
                (key, value)
            })
            .collect());
    }
    let separator = if text.contains('\n') { '\n' } else { ';' };
    text.split(separator)
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            let (name, value) = line
                .split_once('=')
                .ok_or_else(|| format!("Use `Field name = value`; this line has no `=`: {line}"))?;
            Ok((name.trim().to_owned(), value.trim().to_owned()))
        })
        .collect()
}

fn text_of(dictionary: &Dictionary, key: &[u8]) -> Option<String> {
    dictionary.get(key).ok().and_then(|object| match object {
        Object::Name(name) => Some(String::from_utf8_lossy(name).into_owned()),
        other => decode_text_string(other).ok(),
    })
}

/// Walk `/AcroForm /Fields` and every `/Kids` tree into terminal fields.
fn collect_fields(document: &Document) -> Result<Vec<Field>, String> {
    let catalog = document.catalog().map_err(|error| error.to_string())?;
    let Ok(form) = catalog.get(b"AcroForm") else {
        return Ok(Vec::new());
    };
    let (_, form) = document
        .dereference(form)
        .map_err(|error| error.to_string())?;
    let form = form
        .as_dict()
        .map_err(|_| "The form dictionary is damaged".to_string())?;
    let roots = match form.get(b"Fields") {
        Ok(fields) => document
            .dereference(fields)
            .map_err(|error| error.to_string())?
            .1
            .as_array()
            .map_err(|_| "The form field list is damaged".to_string())?
            .clone(),
        Err(_) => return Ok(Vec::new()),
    };
    let mut fields = Vec::new();
    let mut seen = HashSet::new();
    let mut stack = roots
        .iter()
        .rev()
        .filter_map(|object| object.as_reference().ok())
        .map(|id| (id, String::new(), None::<Vec<u8>>, 0u32))
        .collect::<Vec<_>>();
    while let Some((id, prefix, inherited_type, inherited_flags)) = stack.pop() {
        if !seen.insert(id) || fields.len() >= MAX_FIELDS {
            continue;
        }
        let Ok(dictionary) = document.get_dictionary(id) else {
            continue;
        };
        let partial = text_of(dictionary, b"T");
        let name = match (&partial, prefix.is_empty()) {
            (Some(part), true) => part.clone(),
            (Some(part), false) => format!("{prefix}.{part}"),
            (None, _) => prefix.clone(),
        };
        let field_type = dictionary
            .get(b"FT")
            .ok()
            .and_then(|value| value.as_name().ok())
            .map(<[u8]>::to_vec)
            .or(inherited_type);
        let flags = dictionary
            .get(b"Ff")
            .ok()
            .and_then(|value| value.as_i64().ok())
            .map_or(inherited_flags, |flags| flags as u32);
        let kids = dictionary
            .get(b"Kids")
            .ok()
            .and_then(|kids| document.dereference(kids).ok())
            .and_then(|(_, kids)| kids.as_array().ok().cloned())
            .unwrap_or_default();
        // Kids that carry their own name are sub-fields; the rest are widgets.
        let (named, widgets): (Vec<_>, Vec<_>) = kids
            .iter()
            .filter_map(|kid| kid.as_reference().ok())
            .partition(|kid| document.get_dictionary(*kid).is_ok_and(|kid| kid.has(b"T")));
        for kid in named.iter().rev() {
            stack.push((*kid, name.clone(), field_type.clone(), flags));
        }
        if !named.is_empty() && widgets.is_empty() && partial.is_some() && field_type.is_none() {
            continue;
        }
        let Some(field_type) = field_type else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        let kind = match (field_type.as_slice(), flags) {
            (b"Tx", _) => "text",
            (b"Btn", flags) if flags & (1 << 16) != 0 => "button",
            (b"Btn", flags) if flags & (1 << 15) != 0 => "radio",
            (b"Btn", _) => "checkbox",
            (b"Ch", flags) if flags & (1 << 17) != 0 => "dropdown",
            (b"Ch", _) => "list",
            (b"Sig", _) => "signature",
            _ => "other",
        };
        if kind == "button" {
            continue;
        }
        let widgets = if widgets.is_empty() {
            vec![id]
        } else {
            widgets
        };
        let mut options = match kind {
            "dropdown" | "list" => dictionary
                .get(b"Opt")
                .ok()
                .and_then(|options| options.as_array().ok())
                .map(|options| {
                    options
                        .iter()
                        .filter_map(|option| match option {
                            Object::Array(pair) => {
                                pair.get(1).and_then(|label| decode_text_string(label).ok())
                            }
                            other => decode_text_string(other).ok(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            "checkbox" | "radio" => on_states(document, &widgets),
            _ => Vec::new(),
        };
        options.dedup();
        fields.push(Field {
            id,
            name,
            kind,
            value: text_of(dictionary, b"V").unwrap_or_default(),
            options,
            read_only: flags & 1 != 0,
            required: flags & 2 != 0,
            widgets,
        });
    }
    Ok(fields)
}

/// The "on" appearance names of checkbox or radio widgets (e.g. `Yes`).
fn on_states(document: &Document, widgets: &[ObjectId]) -> Vec<String> {
    widgets
        .iter()
        .filter_map(|id| document.get_dictionary(*id).ok())
        .filter_map(|widget| {
            widget
                .get(b"AP")
                .ok()
                .and_then(|ap| document.dereference(ap).ok())
        })
        .filter_map(|(_, ap)| ap.as_dict().ok().and_then(|ap| ap.get(b"N").ok()))
        .filter_map(|normal| {
            document
                .dereference(normal)
                .ok()
                .and_then(|(_, n)| n.as_dict().ok().cloned())
        })
        .flat_map(|states| {
            states
                .iter()
                .map(|(key, _)| String::from_utf8_lossy(key).into_owned())
                .filter(|state| state != "Off")
                .collect::<Vec<_>>()
        })
        .collect()
}

fn truthy(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "yes" | "y" | "true" | "on" | "1" | "x" | "checked"
    )
}

fn set_value(document: &mut Document, field: &Field, value: &str) -> Result<(), String> {
    match field.kind {
        "text" | "dropdown" | "list" => {
            if matches!(field.kind, "dropdown" | "list")
                && !field.options.is_empty()
                && !field.options.iter().any(|option| option == value)
            {
                return Err(format!(
                    "`{}` must be one of: {}",
                    field.name,
                    field.options.join(", ")
                ));
            }
            let dictionary = document
                .get_dictionary_mut(field.id)
                .map_err(|error| error.to_string())?;
            dictionary.set("V", text_string(value));
            // Drop stale appearances so viewers draw the new value.
            for widget in &field.widgets {
                if let Ok(widget) = document.get_dictionary_mut(*widget) {
                    widget.remove(b"AP");
                }
            }
        }
        "checkbox" | "radio" => {
            let state = if field.kind == "radio" {
                field
                    .options
                    .iter()
                    .find(|option| option.eq_ignore_ascii_case(value))
                    .cloned()
                    .or_else(|| (!truthy(value)).then(|| "Off".to_owned()))
                    .ok_or_else(|| {
                        format!(
                            "`{}` must be one of: {}",
                            field.name,
                            field.options.join(", ")
                        )
                    })?
            } else if truthy(value) {
                field
                    .options
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "Yes".into())
            } else {
                "Off".into()
            };
            document
                .get_dictionary_mut(field.id)
                .map_err(|error| error.to_string())?
                .set("V", Object::Name(state.clone().into_bytes()));
            for widget_id in &field.widgets {
                let has_state =
                    on_states(document, std::slice::from_ref(widget_id)).contains(&state);
                if let Ok(widget) = document.get_dictionary_mut(*widget_id) {
                    let appearance = if has_state {
                        state.clone()
                    } else {
                        "Off".into()
                    };
                    widget.set("AS", Object::Name(appearance.into_bytes()));
                }
            }
        }
        "signature" => {
            return Err(format!(
                "`{}` is a digital signature field; use Sign PDF instead",
                field.name
            ));
        }
        _ => return Err(format!("`{}` can't be filled", field.name)),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_parse_from_lines_pairs_and_json() {
        let lines = parse_values("Name = Ada Lovelace\nAgree=yes").unwrap();
        assert_eq!(lines["Name"], "Ada Lovelace");
        assert_eq!(lines["Agree"], "yes");
        let pairs = parse_values("City=London; Zip = N1").unwrap();
        assert_eq!(pairs["Zip"], "N1");
        let json = parse_values(r#"{"Age": 36, "Member": true}"#).unwrap();
        assert_eq!(json["Age"], "36");
        assert_eq!(json["Member"], "yes");
        assert!(parse_values("no equals sign").is_err());
    }

    /// A one-page form with a text field and a checkbox, built in memory.
    pub(crate) fn sample_form() -> Document {
        use lopdf::{Stream, dictionary};
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let on = doc.add_object(Stream::new(dictionary! {}, b"0 0 m".to_vec()));
        let off = doc.add_object(Stream::new(dictionary! {}, Vec::new()));
        let name = doc.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Widget", "FT" => "Tx", "T" => text_string("Full name"),
            "Rect" => vec![50.into(), 700.into(), 300.into(), 720.into()],
        });
        let agree = doc.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Widget", "FT" => "Btn", "T" => text_string("Agree"),
            "Rect" => vec![50.into(), 650.into(), 65.into(), 665.into()], "AS" => "Off",
            "AP" => dictionary! { "N" => dictionary! { "Yes" => on, "Off" => off } },
        });
        let page = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            "Annots" => vec![name.into(), agree.into()],
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1,
            }),
        );
        let form = doc.add_object(dictionary! { "Fields" => vec![name.into(), agree.into()] });
        let catalog = doc.add_object(
            dictionary! { "Type" => "Catalog", "Pages" => pages_id, "AcroForm" => form },
        );
        doc.trailer.set("Root", catalog);
        doc
    }

    #[test]
    fn fields_are_listed_and_filled() {
        let mut doc = sample_form();
        let fields = collect_fields(&doc).unwrap();
        let kinds = fields
            .iter()
            .map(|field| (field.name.as_str(), field.kind))
            .collect::<Vec<_>>();
        assert_eq!(kinds, vec![("Full name", "text"), ("Agree", "checkbox")]);
        assert_eq!(fields[1].options, vec!["Yes".to_owned()]);
        set_value(&mut doc, &fields[0], "Ada Lovelace").unwrap();
        set_value(&mut doc, &fields[1], "yes").unwrap();
        let refreshed = collect_fields(&doc).unwrap();
        assert_eq!(refreshed[0].value, "Ada Lovelace");
        assert_eq!(refreshed[1].value, "Yes");
        let widget = doc.get_dictionary(fields[1].id).unwrap();
        assert_eq!(widget.get(b"AS").unwrap().as_name().unwrap(), b"Yes");
        if let Ok(path) = std::env::var("ARCADE_WRITE_SAMPLE_FORM") {
            sample_form().save(path).unwrap();
        }
    }
}
