use crate::curl::{Body, CurlRequest, Part};
use crate::exec::Meta;
use colored::*;
use serde_json::Value;
use std::io::{self, Write};
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
/// carries nothing but the response. `source` says where the request came
/// from, e.g. `Loading from req.json`.
pub fn print_request_info(req: &CurlRequest, source: Option<&str>) {
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
    if let Some(source) = source {
        eprintln!("{} {}", "📄".dimmed(), source.dimmed());
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
                Body::Multipart(parts) => parts
                    .iter()
                    .map(|part| match part {
                        Part::Text { name, value } => format!("{name}={value}"),
                        Part::File { name, path } => format!("{name}=@{path}"),
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
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

/// Reason phrase for the status codes worth naming
pub fn reason_phrase(code: u16) -> Option<&'static str> {
    Some(match code {
        100 => "Continue",
        101 => "Switching Protocols",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        206 => "Partial Content",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        413 => "Content Too Large",
        415 => "Unsupported Media Type",
        422 => "Unprocessable Content",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => return None,
    })
}

pub fn format_duration(d: Duration) -> String {
    let ms = d.as_secs_f64() * 1000.0;
    if ms < 1.0 {
        format!("{ms:.1}ms")
    } else if ms < 1000.0 {
        format!("{ms:.0}ms")
    } else if ms < 60_000.0 {
        format!("{:.2}s", ms / 1000.0)
    } else {
        let secs = d.as_secs();
        format!("{}m {:02}s", secs / 60, secs % 60)
    }
}

pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// `HTTP 404 Not Found`, colored by status class
fn status_text(code: u16) -> ColoredString {
    let text = match reason_phrase(code) {
        Some(reason) => format!("HTTP {code} {reason}"),
        None => format!("HTTP {code}"),
    };
    status_colored(code, &text)
}

/// The footer printed after a response, e.g. `✓ HTTP 200 OK · 41ms · 1.2 KB`.
/// `curl_exit_code` is set when curl itself failed.
pub fn footer_line(meta: &Meta, elapsed: Duration, curl_exit_code: Option<i32>) -> String {
    // Prefer curl's own timing: it excludes gurl's startup and printing
    let time = meta
        .time_total
        .map(Duration::from_secs_f64)
        .unwrap_or(elapsed);

    let is_error = curl_exit_code.is_some() || meta.status.is_some_and(|code| code >= 400);
    let mut parts: Vec<String> = Vec::new();
    match meta.status {
        Some(code) => parts.push(status_text(code).to_string()),
        None if curl_exit_code.is_none() => parts.push("Done".green().to_string()),
        None => {}
    }
    if curl_exit_code.is_some() {
        parts.push("Failed".red().to_string());
    }
    parts.push(format_duration(time));
    if curl_exit_code.is_none()
        && let Some(size) = meta.size_download
    {
        parts.push(format_size(size));
    }

    let icon = if is_error {
        "✗".red().bold()
    } else {
        "✓".green().bold()
    };
    let mut line = format!("{icon} {}", parts.join(&" · ".dimmed().to_string()));
    if let Some(code) = curl_exit_code {
        line.push_str(&format!(" {}", format!("(curl exit code {code})").dimmed()));
    }
    line
}

pub fn print_success_footer(meta: &Meta, elapsed: Duration) {
    eprintln!();
    eprintln!("{}", footer_line(meta, elapsed, None));
}

/// curl itself failed; it has already explained why on stderr (`-S`).
pub fn print_failure_footer(meta: &Meta, exit_code: i32, elapsed: Duration) {
    eprintln!();
    eprintln!("{}", footer_line(meta, elapsed, Some(exit_code)));
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

    fn meta(status: Option<u16>, time: f64, size: u64) -> Meta {
        Meta {
            status,
            time_total: Some(time),
            size_download: Some(size),
            content_type: None,
        }
    }

    fn footer(meta: &Meta, exit: Option<i32>) -> String {
        colored::control::set_override(false);
        footer_line(meta, Duration::from_secs(9), exit)
    }

    #[test]
    fn footer_for_a_successful_response() {
        assert_eq!(
            footer(&meta(Some(201), 0.1423, 1234), None),
            "✓ HTTP 201 Created · 142ms · 1.2 KB"
        );
    }

    #[test]
    fn footer_for_an_error_response() {
        assert_eq!(
            footer(&meta(Some(404), 0.05, 12), None),
            "✗ HTTP 404 Not Found · 50ms · 12 B"
        );
        assert_eq!(
            footer(&meta(Some(599), 0.05, 0), None),
            "✗ HTTP 599 · 50ms · 0 B"
        );
    }

    #[test]
    fn footer_without_http_status() {
        assert_eq!(
            footer(&meta(None, 0.0004, 213), None),
            "✓ Done · 0.4ms · 213 B"
        );
    }

    #[test]
    fn footer_when_curl_fails() {
        assert_eq!(
            footer(&meta(None, 0.009, 0), Some(7)),
            "✗ Failed · 9ms (curl exit code 7)"
        );
        assert_eq!(
            footer(&meta(Some(200), 1.5, 999), Some(28)),
            "✗ HTTP 200 OK · Failed · 1.50s (curl exit code 28)"
        );
    }

    #[test]
    fn footer_falls_back_to_our_own_timing() {
        assert_eq!(footer(&Meta::default(), None), "✓ Done · 9.00s");
    }

    #[test]
    fn format_duration_picks_a_readable_unit() {
        assert_eq!(format_duration(Duration::from_micros(250)), "0.2ms");
        assert_eq!(format_duration(Duration::from_millis(41)), "41ms");
        assert_eq!(format_duration(Duration::from_millis(1234)), "1.23s");
        assert_eq!(format_duration(Duration::from_secs(65)), "1m 05s");
    }

    #[test]
    fn format_size_picks_a_readable_unit() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1023), "1023 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(format_size(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    #[test]
    fn status_line_code_parses_http_versions() {
        assert_eq!(status_line_code("HTTP/1.1 404 Not Found"), Some(404));
        assert_eq!(status_line_code("HTTP/2 200 "), Some(200));
        assert_eq!(status_line_code("HTTP/1.1"), None);
        assert_eq!(status_line_code("Content-Type: text/html"), None);
    }
}
