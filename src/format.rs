use crate::curl::{Body, CurlRequest};
use colored::*;
use serde_json::Value;
use std::io::{self, Write};
use std::path::Path;
use std::time::Duration;

/// Pretty print JSON with syntax highlighting
pub fn write_json(w: &mut impl Write, value: &Value, indent: usize) -> io::Result<()> {
    let pad = "  ".repeat(indent);
    let pad_inner = "  ".repeat(indent + 1);

    match value {
        Value::Null => write!(w, "{}", "null".magenta()),
        Value::Bool(b) => write!(w, "{}", b.to_string().yellow()),
        Value::Number(n) => write!(w, "{}", n.to_string().cyan()),
        Value::String(s) => write!(w, "{}", json_string(s).green()),
        Value::Array(arr) if arr.is_empty() => write!(w, "[]"),
        Value::Array(arr) => {
            writeln!(w, "[")?;
            for (i, item) in arr.iter().enumerate() {
                write!(w, "{pad_inner}")?;
                write_json(w, item, indent + 1)?;
                writeln!(w, "{}", if i + 1 < arr.len() { "," } else { "" })?;
            }
            write!(w, "{pad}]")
        }
        Value::Object(obj) if obj.is_empty() => write!(w, "{{}}"),
        Value::Object(obj) => {
            writeln!(w, "{{")?;
            for (i, (key, item)) in obj.iter().enumerate() {
                write!(w, "{pad_inner}{}: ", json_string(key).blue().bold())?;
                write_json(w, item, indent + 1)?;
                writeln!(w, "{}", if i + 1 < obj.len() { "," } else { "" })?;
            }
            write!(w, "{pad}}}")
        }
    }
}

/// A string as a JSON literal: quoted, with quotes, backslashes and control
/// characters escaped
fn json_string(s: &str) -> String {
    Value::String(s.to_string()).to_string()
}

/// Format JSON data payload for display
pub fn format_request_body(data: &str) -> String {
    if let Ok(json) = serde_json::from_str::<Value>(data) {
        serde_json::to_string_pretty(&json).unwrap_or_else(|_| data.to_string())
    } else {
        data.to_string()
    }
}

/// Request banner. Like every gurl decoration it goes to stderr, so stdout
/// carries nothing but the response.
pub fn print_request_info(req: &CurlRequest, file_path: Option<&Path>) {
    let method = req.method.as_str();
    let method_color = match method {
        "GET" => method.green(),
        "POST" => method.yellow(),
        "PUT" => method.blue(),
        "PATCH" => method.magenta(),
        "DELETE" => method.red(),
        _ => method.white(),
    };

    eprintln!();
    if let Some(path) = file_path {
        eprintln!(
            "{} {}",
            "📄".dimmed(),
            format!("Loading from {}", path.display()).dimmed()
        );
    }
    eprintln!(
        "{} {} {}",
        "▶".bold().cyan(),
        method_color.bold(),
        req.url.underline()
    );

    if req.verbose {
        if !req.headers.is_empty() {
            eprintln!("{}", "  Headers:".dimmed());
            for h in &req.headers {
                eprintln!("    {}", h.dimmed());
            }
        }

        if let Some(ref body) = req.body {
            eprintln!("{}", "  Body:".dimmed());
            let formatted = match body {
                Body::Raw(data) => format_request_body(data),
                Body::File(path) => format!("@{path}"),
            };
            for line in formatted.lines() {
                eprintln!("    {}", line.dimmed());
            }
        }
    }

    eprintln!();
}

fn status_colored(code: u16, text: &str) -> ColoredString {
    match code {
        200..=299 => text.green().bold(),
        300..=399 => text.cyan().bold(),
        400..=499 => text.yellow().bold(),
        500..=599 => text.red().bold(),
        _ => text.white().bold(),
    }
}

/// Status code of an `HTTP/x <code> [reason]` line
fn status_line_code(line: &str) -> Option<u16> {
    line.strip_prefix("HTTP/")?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

/// Write the response headers curl's `-i` emits, highlighted.
pub fn write_response_headers(w: &mut impl Write, headers: &str) -> io::Result<()> {
    for line in headers.lines() {
        if let Some(code) = status_line_code(line) {
            writeln!(w, "{}", status_colored(code, line))?;
        } else if let Some((name, value)) = line.split_once(':') {
            writeln!(w, "{}:{value}", name.cyan())?;
        } else {
            writeln!(w, "{line}")?;
        }
    }
    Ok(())
}

pub fn print_success_footer(status: Option<u16>, elapsed: Duration) {
    eprintln!();
    // curl succeeded, but the server may still have answered with an error
    let failed = status.is_some_and(|code| code >= 400);
    if let Some(code) = status {
        eprint!("{} ", status_colored(code, &code.to_string()));
    }
    if failed {
        eprintln!(
            "{} {} in {elapsed:.2?}",
            "✗".red().bold(),
            "Error response received".red()
        );
    } else {
        eprintln!(
            "{} {} in {elapsed:.2?}",
            "✓".green().bold(),
            "Response received".green()
        );
    }
}

/// curl itself failed; it has already explained why on stderr (`-S`).
pub fn print_failure_footer(status: Option<u16>, exit_code: i32, elapsed: Duration) {
    eprintln!();
    if let Some(code) = status {
        eprint!("{} ", status_colored(code, &code.to_string()));
    }
    eprintln!(
        "{} {} in {elapsed:.2?} (curl exit code {exit_code})",
        "✗".red().bold(),
        "Failed".red()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(json: &str) -> String {
        colored::control::set_override(false);
        let value: Value = serde_json::from_str(json).unwrap();
        let mut out = Vec::new();
        write_json(&mut out, &value, 0).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn write_json_output_is_valid_json_with_escapes() {
        let input =
            r#"{"quote":"say \"hi\"","path":"C:\\tmp","multi":"a\nb\tc","uni":"é✓","key \"q\"":1}"#;
        let rendered = render(input);
        let reparsed: Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(reparsed, serde_json::from_str::<Value>(input).unwrap());
        assert!(rendered.contains(r#""quote": "say \"hi\"""#));
        assert!(rendered.contains(r#""multi": "a\nb\tc""#));
    }

    #[test]
    fn write_json_preserves_key_order() {
        assert_eq!(
            render(r#"{"zebra":1,"apple":2,"mango":3}"#),
            "{\n  \"zebra\": 1,\n  \"apple\": 2,\n  \"mango\": 3\n}"
        );
    }

    #[test]
    fn write_json_nested_layout() {
        assert_eq!(
            render(r#"{"a":[1,{"b":null}],"c":{},"d":[],"e":true}"#),
            "{\n  \"a\": [\n    1,\n    {\n      \"b\": null\n    }\n  ],\n  \"c\": {},\n  \"d\": [],\n  \"e\": true\n}"
        );
    }

    #[test]
    fn write_json_scalars() {
        assert_eq!(render("42"), "42");
        assert_eq!(render("\"x\""), "\"x\"");
        assert_eq!(render("null"), "null");
    }

    #[test]
    fn write_response_headers_normalizes_line_endings() {
        colored::control::set_override(false);
        let mut out = Vec::new();
        write_response_headers(
            &mut out,
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n",
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "HTTP/1.1 200 OK\nContent-Type: text/html\n\n"
        );
    }

    #[test]
    fn format_request_body_valid_json() {
        assert_eq!(
            format_request_body(r#"{"key":"value"}"#),
            "{\n  \"key\": \"value\"\n}"
        );
    }

    #[test]
    fn format_request_body_non_json() {
        let input = "plain text body";
        assert_eq!(format_request_body(input), input);
    }

    #[test]
    fn status_line_code_parses_http_versions() {
        assert_eq!(status_line_code("HTTP/1.1 404 Not Found"), Some(404));
        assert_eq!(status_line_code("HTTP/2 200 "), Some(200));
        assert_eq!(status_line_code("HTTP/1.1"), None);
        assert_eq!(status_line_code("Content-Type: text/html"), None);
    }
}
