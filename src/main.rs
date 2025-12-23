use clap::Parser;
use colored::*;
use std::process::Command;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// The URL to request
    url: String,

    /// Request method (GET, POST, PUT, DELETE, etc.)
    #[arg(short = 'm', long, default_value = "GET")]
    method: String,

    /// Data payload (e.g. JSON string)
    #[arg(short, long)]
    data: Option<String>,

    /// Headers to include
    #[arg(short = 'H', long)]
    headers: Vec<String>,

    /// Include HTTP headers in the output
    #[arg(short = 'i', long)]
    include: bool,

    /// Verbose output
    #[arg(short, long)]
    verbose: bool,

    /// Extra arguments passed directly to curl
    #[arg(last = true)]
    extra_args: Vec<String>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let mut cmd = Command::new("curl");

    // Method
    cmd.arg("-X").arg(&args.method.to_uppercase());

    // Data
    if let Some(data) = args.data {
        // If it looks like JSON, add Content-Type: application/json automatically if not present?
        // Let's keep it simple for now, user adds headers manually.
        cmd.arg("-d").arg(data);
    }

    // Headers
    for header in args.headers {
        cmd.arg("-H").arg(header);
    }

    // Flags
    if args.verbose {
        cmd.arg("-v");
    }
    if args.include {
        cmd.arg("-i");
    }

    // Extra args
    for arg in args.extra_args {
        cmd.arg(arg);
    }

    // URL
    cmd.arg(&args.url);

    // Visual feedback
    eprintln!("{} curl ... {}", "Running:".dimmed(), args.url);

    let status = cmd.status()?;

    if !status.success() {
        eprintln!("{}", "Request failed".red());
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(())
}
