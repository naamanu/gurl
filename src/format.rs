use crate::curl::CurlRequest;
use colored::*;
use serde_json::Value;
use std::path::Path;
use std::time::Duration;

/// Pretty print JSON with syntax highlighting
pub fn print_json_colored(value: &Value, indent: usize) {
    let pad = "  ".repeat(indent);
    let pad_inner = "  ".repeat(indent + 1);

    match value {
        Value::Null => print!("{}", "null".magenta()),
        Value::Bool(b) => print!("{}", b.to_string().yellow()),
        Value::Number(n) => print!("{}", n.to_string().cyan()),
        Value::String(s) => print!("{}", format!("\"{s}\"").green()),
        Value::Array(arr) => {
            if arr.is_empty() {
                print!("[]");
            } else {
                println!("[");
                for (i, item) in arr.iter().enumerate() {
                    print!("{pad_inner}");
                    print_json_colored(item, indent + 1);
                    if i < arr.len() - 1 {
                        println!(",");
                    } else {
                        println!();
                    }
                }
                print!("{pad}]");
            }
        }
        Value::Object(obj) => {
            if obj.is_empty() {
                print!("{{}}");
            } else {
                println!("{{");
                let keys: Vec<_> = obj.keys().collect();
                for (i, key) in keys.iter().enumerate() {
                    print!("{pad_inner}{}: ", format!("\"{key}\"").blue().bold());
                    print_json_colored(&obj[*key], indent + 1);
                    if i < keys.len() - 1 {
                        println!(",");
                    } else {
                        println!();
                    }
                }
                print!("{pad}}}");
            }
        }
    }
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
            let formatted = format_request_body(body);
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

/// Print the response headers curl's `-i` emits, highlighted. They are part
/// of the requested output, so they go to stdout.
pub fn print_response_headers(headers: &str) {
    for line in headers.lines() {
        if let Some(code) = status_line_code(line) {
            println!("{}", status_colored(code, line));
        } else if let Some((name, value)) = line.split_once(':') {
            println!("{}:{value}", name.cyan());
        } else {
            println!("{line}");
        }
    }
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
