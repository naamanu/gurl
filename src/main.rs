use clap::Parser;
use colored::*;
use serde_json::Value;
use std::process::Command;
use std::time::Instant;

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "A simple curl wrapper for easier terminal usage",
    long_about = None
)]
struct Args {
    /// The URL to request (use :port/path as shorthand for localhost)
    url: String,

    /// Request method (GET, POST, PUT, DELETE, PATCH, etc.)
    #[arg(short = 'm', long, default_value = "GET")]
    method: String,

    /// Data payload (auto-detects JSON and sets Content-Type)
    #[arg(short, long)]
    data: Option<String>,

    /// Headers to include (can be used multiple times)
    #[arg(short = 'H', long)]
    headers: Vec<String>,

    /// Include HTTP headers in the output
    #[arg(short = 'i', long)]
    include: bool,

    /// Verbose output (shows full curl command)
    #[arg(short, long)]
    verbose: bool,

    /// Silent mode (suppress progress meter)
    #[arg(short, long)]
    silent: bool,

    /// Follow redirects
    #[arg(short = 'L', long)]
    location: bool,

    /// Request timeout in seconds
    #[arg(short = 't', long)]
    timeout: Option<u64>,

    /// Save response to file
    #[arg(short, long)]
    output: Option<String>,

    /// HTTP basic auth (user:password)
    #[arg(short, long)]
    user: Option<String>,

    /// Pretty print JSON output with syntax highlighting
    #[arg(short, long)]
    pretty: bool,

    /// Extra arguments passed directly to curl
    #[arg(last = true)]
    extra_args: Vec<String>,
}

fn expand_url(url: &str) -> String {
    if url.starts_with(':') {
        format!("http://localhost{}", url)
    } else if !url.starts_with("http://") && !url.starts_with("https://") {
        format!("https://{}", url)
    } else {
        url.to_string()
    }
}

fn looks_like_json(data: &str) -> bool {
    let trimmed = data.trim();
    (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
}

fn has_content_type_header(headers: &[String]) -> bool {
    headers
        .iter()
        .any(|h| h.to_lowercase().starts_with("content-type"))
}

/// Pretty print JSON with syntax highlighting
fn print_json_colored(value: &Value, indent: usize) {
    let pad = "  ".repeat(indent);
    let pad_inner = "  ".repeat(indent + 1);

    match value {
        Value::Null => print!("{}", "null".magenta()),
        Value::Bool(b) => print!("{}", b.to_string().yellow()),
        Value::Number(n) => print!("{}", n.to_string().cyan()),
        Value::String(s) => print!("{}", format!("\"{}\"", s).green()),
        Value::Array(arr) => {
            if arr.is_empty() {
                print!("[]");
            } else {
                println!("[");
                for (i, item) in arr.iter().enumerate() {
                    print!("{}", pad_inner);
                    print_json_colored(item, indent + 1);
                    if i < arr.len() - 1 {
                        println!(",");
                    } else {
                        println!();
                    }
                }
                print!("{}]", pad);
            }
        }
        Value::Object(obj) => {
            if obj.is_empty() {
                print!("{{}}");
            } else {
                println!("{{");
                let keys: Vec<_> = obj.keys().collect();
                for (i, key) in keys.iter().enumerate() {
                    print!("{}{}: ", pad_inner, format!("\"{}\"", key).blue().bold());
                    print_json_colored(&obj[*key], indent + 1);
                    if i < keys.len() - 1 {
                        println!(",");
                    } else {
                        println!();
                    }
                }
                print!("{}}}", pad);
            }
        }
    }
}

/// Format JSON data payload for display
fn format_request_body(data: &str) -> String {
    if let Ok(json) = serde_json::from_str::<Value>(data) {
        serde_json::to_string_pretty(&json).unwrap_or_else(|_| data.to_string())
    } else {
        data.to_string()
    }
}

fn print_request_info(
    method: &str,
    url: &str,
    headers: &[String],
    data: Option<&String>,
    verbose: bool,
) {
    // Method and URL
    let method_color = match method {
        "GET" => method.green(),
        "POST" => method.yellow(),
        "PUT" => method.blue(),
        "PATCH" => method.magenta(),
        "DELETE" => method.red(),
        _ => method.white(),
    };

    println!();
    println!(
        "{} {} {}",
        "▶".bold().cyan(),
        method_color.bold(),
        url.underline()
    );

    if verbose {
        // Headers
        if !headers.is_empty() {
            println!("{}", "  Headers:".dimmed());
            for h in headers {
                println!("    {}", h.dimmed());
            }
        }

        // Body
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

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let url = expand_url(&args.url);

    let mut cmd = Command::new("curl");
    let mut curl_args: Vec<String> = Vec::new();

    // Method
    curl_args.push("-X".to_string());
    curl_args.push(args.method.to_uppercase());

    // Data with auto JSON detection
    let mut effective_headers = args.headers.clone();
    if let Some(ref data) = args.data {
        if looks_like_json(data) && !has_content_type_header(&args.headers) {
            effective_headers.push("Content-Type: application/json".to_string());
        }
    }

    // Headers
    for header in &effective_headers {
        curl_args.push("-H".to_string());
        curl_args.push(header.clone());
    }

    // Data
    if let Some(ref data) = args.data {
        curl_args.push("-d".to_string());
        curl_args.push(data.clone());
    }

    // Flags
    if args.verbose {
        curl_args.push("-v".to_string());
    }
    if args.include {
        curl_args.push("-i".to_string());
    }
    if args.silent || args.pretty {
        curl_args.push("-s".to_string());
    }
    if args.location {
        curl_args.push("-L".to_string());
    }

    // Timeout
    if let Some(timeout) = args.timeout {
        curl_args.push("--max-time".to_string());
        curl_args.push(timeout.to_string());
    }

    // Output file
    if let Some(ref output) = args.output {
        curl_args.push("-o".to_string());
        curl_args.push(output.clone());
    }

    // Basic auth
    if let Some(ref user) = args.user {
        curl_args.push("-u".to_string());
        curl_args.push(user.clone());
    }

    // Extra args
    for arg in &args.extra_args {
        curl_args.push(arg.clone());
    }

    // URL (always last)
    curl_args.push(url.clone());

    // Build command
    for arg in &curl_args {
        cmd.arg(arg);
    }

    // Request info
    print_request_info(
        &args.method.to_uppercase(),
        &url,
        &effective_headers,
        args.data.as_ref(),
        args.verbose,
    );

    if args.verbose {
        eprintln!(
            "{} curl {}",
            "Command:".dimmed(),
            curl_args.join(" ").dimmed()
        );
        eprintln!();
    }

    let start = Instant::now();

    // Pretty print JSON
    if args.pretty && args.output.is_none() {
        let output = cmd.output()?;
        let elapsed = start.elapsed();

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);

            // Try to parse as JSON
            if let Ok(json) = serde_json::from_str::<Value>(&stdout) {
                print_json_colored(&json, 0);
                println!();
            } else {
                // Not JSON, print raw
                print!("{}", stdout);
            }

            println!();
            println!(
                "{} {} in {:.2?}",
                "✓".green().bold(),
                "Response received".green(),
                elapsed
            );
        } else {
            eprint!("{}", String::from_utf8_lossy(&output.stderr));
            print_error(output.status.code());
            std::process::exit(output.status.code().unwrap_or(1));
        }
    } else {
        let status = cmd.status()?;
        let elapsed = start.elapsed();

        println!();
        if status.success() {
            println!(
                "{} {} in {:.2?}",
                "✓".green().bold(),
                "Done".green(),
                elapsed
            );
        } else {
            print_error(status.code());
            eprintln!("{} in {:.2?}", "Failed".red(), elapsed);
            std::process::exit(status.code().unwrap_or(1));
        }
    }

    Ok(())
}

fn print_error(code: Option<i32>) {
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
    eprintln!("{} {} (exit code {})", "✗".red().bold(), msg.red(), code);
}
