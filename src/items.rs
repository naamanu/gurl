//! httpie-style request items: `gurl POST :3000/users name=Jo age:=30 X-Key:abc`

use crate::curl::{Body, Part};
use anyhow::{Context, bail};
use serde_json::{Map, Value};
use std::fs;

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    /// `Name:value`, or `Name:` to drop a header curl would send
    Header { name: String, value: String },
    /// `name==value`, appended to the URL
    Query { name: String, value: String },
    /// `name=value`: a string field
    Field { name: String, value: String },
    /// `name=@path`: a string field holding a text file's contents
    FieldFromFile { name: String, path: String },
    /// `name:=json`: a raw JSON field (number, bool, null, array, object)
    Json { name: String, value: Value },
    /// `name:=@path`: a JSON field read from a file
    JsonFromFile { name: String, path: String },
    /// `name@path`: a file upload (`--form` only)
    File { name: String, path: String },
}

/// Longest first, so that at the same position `:=` beats `:`
const SEPARATORS: [&str; 7] = [":=@", ":=", "==", "=@", "=", ":", "@"];

/// Parse one item. `Ok(None)` means the token isn't an item at all.
pub fn parse_item(token: &str) -> anyhow::Result<Option<Item>> {
    // The earliest separator wins; ties go to the longest
    let Some((pos, sep)) = SEPARATORS
        .iter()
        .filter_map(|sep| token.find(sep).map(|pos| (pos, *sep)))
        .min_by_key(|(pos, sep)| (*pos, std::cmp::Reverse(sep.len())))
    else {
        return Ok(None);
    };

    let name = &token[..pos];
    let value = &token[pos + sep.len()..];
    if name.is_empty() {
        return Ok(None);
    }
    let (name, value) = (name.to_string(), value.to_string());

    Ok(Some(match sep {
        ":" => {
            // `https://...` is a URL, not a header named "https"
            if !is_header_name(&name) || value.starts_with("//") {
                return Ok(None);
            }
            Item::Header { name, value }
        }
        "==" => Item::Query { name, value },
        "=" => Item::Field { name, value },
        "=@" => Item::FieldFromFile { name, path: value },
        ":=" => {
            let parsed = serde_json::from_str(&value)
                .with_context(|| format!("`{token}`: the value after `:=` must be JSON"))?;
            Item::Json {
                name,
                value: parsed,
            }
        }
        ":=@" => Item::JsonFromFile { name, path: value },
        "@" => Item::File { name, path: value },
        _ => unreachable!("every separator is handled"),
    }))
}

/// RFC 9110 token characters
fn is_header_name(name: &str) -> bool {
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c))
}

/// What a list of items contributes to the request.
#[derive(Debug, Default, PartialEq)]
pub struct ItemParts {
    pub headers: Vec<String>,
    pub query: Vec<(String, String)>,
    pub body: Option<Body>,
    /// The body was built as JSON
    pub json: bool,
}

/// Turn items into headers, query parameters and a body: a JSON object by
/// default, or a form with `form` (multipart when a file is attached).
pub fn build(items: &[Item], form: bool) -> anyhow::Result<ItemParts> {
    let mut parts = ItemParts::default();
    let mut fields: Vec<(String, FieldValue)> = Vec::new();

    for item in items {
        match item {
            Item::Header { name, value } if value.is_empty() => {
                parts.headers.push(format!("{name}:"));
            }
            Item::Header { name, value } => parts.headers.push(format!("{name}: {value}")),
            Item::Query { name, value } => parts.query.push((name.clone(), value.clone())),
            Item::Field { name, value } => {
                fields.push((name.clone(), FieldValue::Text(value.clone())));
            }
            Item::FieldFromFile { name, path } => {
                let text = fs::read_to_string(path)
                    .with_context(|| format!("Failed to read `{path}` for field `{name}`"))?;
                fields.push((name.clone(), FieldValue::Text(text)));
            }
            Item::Json { name, value } => {
                fields.push((name.clone(), FieldValue::Json(value.clone())));
            }
            Item::JsonFromFile { name, path } => {
                let text = fs::read_to_string(path)
                    .with_context(|| format!("Failed to read `{path}` for field `{name}`"))?;
                let value = serde_json::from_str(&text)
                    .with_context(|| format!("`{path}` (field `{name}`) is not valid JSON"))?;
                fields.push((name.clone(), FieldValue::Json(value)));
            }
            Item::File { name, path } => {
                if !form {
                    bail!("`{name}@{path}` uploads a file, which needs --form");
                }
                fields.push((name.clone(), FieldValue::File(path.clone())));
            }
        }
    }

    if fields.is_empty() {
        return Ok(parts);
    }

    parts.body = Some(if form {
        form_body(fields)?
    } else {
        parts.json = true;
        let object: Map<String, Value> = fields
            .into_iter()
            .map(|(name, value)| match value {
                FieldValue::Text(text) => (name, Value::String(text)),
                FieldValue::Json(json) => (name, json),
                FieldValue::File(_) => unreachable!("rejected above without --form"),
            })
            .collect();
        Body::Raw(Value::Object(object).to_string())
    });
    Ok(parts)
}

enum FieldValue {
    Text(String),
    Json(Value),
    File(String),
}

fn form_body(fields: Vec<(String, FieldValue)>) -> anyhow::Result<Body> {
    if let Some((name, _)) = fields
        .iter()
        .find(|(_, v)| matches!(v, FieldValue::Json(_)))
    {
        bail!("`{name}:=...` is a JSON field, which can't be sent with --form");
    }

    let has_file = fields.iter().any(|(_, v)| matches!(v, FieldValue::File(_)));
    if has_file {
        let parts = fields
            .into_iter()
            .map(|(name, value)| match value {
                FieldValue::Text(value) => Part::Text { name, value },
                FieldValue::File(path) => Part::File { name, path },
                FieldValue::Json(_) => unreachable!("rejected above"),
            })
            .collect();
        return Ok(Body::Multipart(parts));
    }

    let encoded = fields
        .into_iter()
        .map(|(name, value)| match value {
            FieldValue::Text(value) => format!(
                "{}={}",
                crate::url::percent_encode(&name),
                crate::url::percent_encode(&value)
            ),
            _ => unreachable!("only text fields remain"),
        })
        .collect::<Vec<_>>()
        .join("&");
    Ok(Body::Raw(encoded))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;

    fn item(token: &str) -> Item {
        parse_item(token).unwrap().unwrap()
    }

    fn not_item(token: &str) {
        assert_eq!(parse_item(token).unwrap(), None, "{token}");
    }

    fn s(v: &str) -> String {
        v.to_string()
    }

    #[test]
    fn parses_each_kind() {
        assert_eq!(
            item("X-Api-Key:abc"),
            Item::Header {
                name: s("X-Api-Key"),
                value: s("abc")
            }
        );
        assert_eq!(
            item("page==2"),
            Item::Query {
                name: s("page"),
                value: s("2")
            }
        );
        assert_eq!(
            item("name=Jo"),
            Item::Field {
                name: s("name"),
                value: s("Jo")
            }
        );
        assert_eq!(
            item("bio=@bio.txt"),
            Item::FieldFromFile {
                name: s("bio"),
                path: s("bio.txt")
            }
        );
        assert_eq!(
            item("age:=30"),
            Item::Json {
                name: s("age"),
                value: json!(30)
            }
        );
        assert_eq!(
            item("tags:=[\"a\",\"b\"]"),
            Item::Json {
                name: s("tags"),
                value: json!(["a", "b"])
            }
        );
        assert_eq!(
            item("meta:=@meta.json"),
            Item::JsonFromFile {
                name: s("meta"),
                path: s("meta.json")
            }
        );
        assert_eq!(
            item("avatar@me.png"),
            Item::File {
                name: s("avatar"),
                path: s("me.png")
            }
        );
    }

    #[test]
    fn earliest_separator_wins() {
        assert_eq!(
            item("email=me@example.com"),
            Item::Field {
                name: s("email"),
                value: s("me@example.com")
            }
        );
        assert_eq!(
            item("X-Time:12:00"),
            Item::Header {
                name: s("X-Time"),
                value: s("12:00")
            }
        );
        assert_eq!(
            item("q=a==b"),
            Item::Field {
                name: s("q"),
                value: s("a==b")
            }
        );
        assert_eq!(
            item("note=x:=y"),
            Item::Field {
                name: s("note"),
                value: s("x:=y")
            }
        );
    }

    #[test]
    fn empty_values_are_allowed() {
        assert_eq!(
            item("Accept:"),
            Item::Header {
                name: s("Accept"),
                value: s("")
            }
        );
        assert_eq!(
            item("name="),
            Item::Field {
                name: s("name"),
                value: s("")
            }
        );
    }

    #[test]
    fn non_items() {
        not_item("example.com");
        not_item(":3000/api");
        not_item("=value");
        not_item("https://example.com");
        not_item("Bad Header:x");
    }

    #[test]
    fn invalid_json_is_an_error() {
        let err = parse_item("age:=thirty").unwrap_err();
        assert!(err.to_string().contains("must be JSON"), "{err}");
    }

    #[test]
    fn builds_a_json_object_in_order() {
        let items = [
            item("name=Jo"),
            item("age:=30"),
            item("admin:=true"),
            item("X-Key:abc"),
            item("Accept:"),
            item("page==2"),
        ];
        let parts = build(&items, false).unwrap();
        assert_eq!(
            parts,
            ItemParts {
                headers: vec![s("X-Key: abc"), s("Accept:")],
                query: vec![(s("page"), s("2"))],
                body: Some(Body::Raw(s(r#"{"name":"Jo","age":30,"admin":true}"#))),
                json: true,
            }
        );
    }

    #[test]
    fn headers_and_query_alone_make_no_body() {
        let parts = build(&[item("X-Key:abc"), item("q==x")], false).unwrap();
        assert_eq!(parts.body, None);
        assert!(!parts.json);
    }

    #[test]
    fn form_fields_are_urlencoded() {
        let parts = build(&[item("name=Jo Bloggs"), item("q=a&b=c")], true).unwrap();
        assert_eq!(
            parts.body,
            Some(Body::Raw(s("name=Jo%20Bloggs&q=a%26b%3Dc")))
        );
        assert!(!parts.json);
    }

    #[test]
    fn form_with_a_file_is_multipart() {
        let parts = build(&[item("title=Hi"), item("doc@report.pdf")], true).unwrap();
        assert_eq!(
            parts.body,
            Some(Body::Multipart(vec![
                Part::Text {
                    name: s("title"),
                    value: s("Hi")
                },
                Part::File {
                    name: s("doc"),
                    path: s("report.pdf")
                },
            ]))
        );
    }

    #[test]
    fn file_upload_needs_form() {
        let err = build(&[item("doc@report.pdf")], false).unwrap_err();
        assert!(err.to_string().contains("needs --form"), "{err}");
    }

    #[test]
    fn json_fields_cannot_be_form_fields() {
        let err = build(&[item("age:=30")], true).unwrap_err();
        assert!(
            err.to_string().contains("can't be sent with --form"),
            "{err}"
        );
    }

    #[test]
    fn fields_can_come_from_files() {
        let mut text = tempfile::NamedTempFile::new().unwrap();
        text.write_all(b"line one\nline two").unwrap();
        let mut json_file = tempfile::NamedTempFile::new().unwrap();
        json_file.write_all(br#"{"k": [1, 2]}"#).unwrap();

        let items = [
            item(&format!("bio=@{}", text.path().display())),
            item(&format!("meta:=@{}", json_file.path().display())),
        ];
        let parts = build(&items, false).unwrap();
        assert_eq!(
            parts.body,
            Some(Body::Raw(s(
                r#"{"bio":"line one\nline two","meta":{"k":[1,2]}}"#
            )))
        );
    }

    #[test]
    fn missing_field_file_is_an_error() {
        let err = build(&[item("bio=@/nonexistent/bio.txt")], false).unwrap_err();
        assert!(err.to_string().contains("bio.txt"), "{err}");
    }
}
