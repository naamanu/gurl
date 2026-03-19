use colored::*;
use serde_json::Value;
use std::path::Path;

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

pub fn print_request_info(
    method: &str,
    url: &str,
    headers: &[String],
    data: Option<&str>,
    verbose: bool,
    file_path: Option<&Path>,
) {
    let method_color = match method {
        "GET" => method.green(),
        "POST" => method.yellow(),
        "PUT" => method.blue(),
        "PATCH" => method.magenta(),
        "DELETE" => method.red(),
        _ => method.white(),
    };

    println!();
    if let Some(path) = file_path {
        println!(
            "{} {}",
            "📄".dimmed(),
            format!("Loading from {}", path.display()).dimmed()
        );
    }
    println!(
        "{} {} {}",
        "▶".bold().cyan(),
        method_color.bold(),
        url.underline()
    );

    if verbose {
        if !headers.is_empty() {
            println!("{}", "  Headers:".dimmed());
            for h in headers {
                println!("    {}", h.dimmed());
            }
        }

        if let Some(body) = data {
            println!("{}", "  Body:".dimmed());
            let formatted = format_request_body(body);
            for line in formatted.lines() {
                println!("    {}", line.dimmed());
            }
        }
    }

    println!();
}

pub fn print_status_code(code: u16) {
    let colored_code = match code {
        200..=299 => code.to_string().green().bold(),
        300..=399 => code.to_string().cyan().bold(),
        400..=499 => code.to_string().yellow().bold(),
        500..=599 => code.to_string().red().bold(),
        _ => code.to_string().white().bold(),
    };
    print!("{colored_code} ");
}

pub fn print_error(code: Option<i32>) {
    let code = code.unwrap_or(1);
    let msg = match code {
        6 => "Could not resolve host",
        7 => "Failed to connect to host",
        28 => "Operation timed out",
        35 => "SSL connect error",
        52 => "Empty reply from server",
        56 => "Failure in receiving network data",
        _ => "Request failed",
    };
    eprintln!("{} {} (exit code {code})", "✗".red().bold(), msg.red());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_request_body_valid_json() {
        let result = format_request_body(r#"{"key":"value"}"#);
        assert!(result.contains("key"));
        assert!(result.contains("value"));
        assert!(result.contains('\n'));
    }

    #[test]
    fn format_request_body_non_json() {
        let input = "plain text body";
        assert_eq!(format_request_body(input), input);
    }

    #[test]
    fn print_error_known_codes() {
        print_error(Some(6));
        print_error(Some(7));
        print_error(Some(28));
        print_error(Some(35));
        print_error(Some(52));
        print_error(Some(56));
    }

    #[test]
    fn print_error_unknown_code() {
        print_error(Some(99));
        print_error(None);
    }
}
