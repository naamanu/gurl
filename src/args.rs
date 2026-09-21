use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "A simple curl wrapper for easier terminal usage",
    long_about = None,
    after_help = "\
Examples:
  gurl https://api.example.com/users          GET request (default)
  gurl :8080/health                            Localhost shorthand
  gurl -m POST -d '{\"name\":\"foo\"}' :3000/api  POST with JSON body
  gurl -f request.json                         Load request from file
  gurl --pretty https://api.example.com/data   Pretty-print JSON response
  gurl -H 'Authorization: Bearer tok' api.com  Custom header
  gurl -L -t 30 https://example.com            Follow redirects, 30s timeout
  gurl -s https://api.example.com | jq .       Only the response, for scripts
  gurl --dry-run -d @body.json :3000/api       Show the curl command, don't run it
  gurl https://example.com -- -k --compressed  Pass extra args to curl"
)]
pub struct Args {
    /// The URL to request (use :port/path as shorthand for localhost)
    /// Can be omitted if using --file with url field
    pub url: Option<String>,

    /// Request method (GET, POST, PUT, DELETE, PATCH, etc.)
    #[arg(short = 'm', long)]
    pub method: Option<String>,

    /// Data payload, sent as-is (auto-detects JSON and sets Content-Type).
    /// Use @path to send a file, or @- for stdin
    #[arg(short, long)]
    pub data: Option<String>,

    /// Headers to include (can be used multiple times)
    #[arg(short = 'H', long, visible_alias = "header")]
    pub headers: Vec<String>,

    /// Load request from JSON file (headers, body, method, url)
    #[arg(short, long)]
    pub file: Option<PathBuf>,

    /// Include HTTP headers in the output
    #[arg(short = 'i', long)]
    pub include: bool,

    /// Verbose output (shows full curl command)
    #[arg(short, long)]
    pub verbose: bool,

    /// Silent mode (only the response: no request banner or status footer)
    #[arg(short, long)]
    pub silent: bool,

    /// Follow redirects
    #[arg(short = 'L', long)]
    pub location: bool,

    /// Request timeout in seconds
    #[arg(short = 't', long)]
    pub timeout: Option<u64>,

    /// Save response to file
    #[arg(short, long)]
    pub output: Option<String>,

    /// HTTP basic auth (user:password)
    #[arg(short, long)]
    pub user: Option<String>,

    /// Pretty print JSON output with syntax highlighting
    #[arg(short, long)]
    pub pretty: bool,

    /// Print the equivalent curl command instead of running it
    #[arg(long)]
    pub dry_run: bool,

    /// Show credentials in --verbose and --dry-run output instead of masking them
    #[arg(long)]
    pub show_secrets: bool,

    /// Extra arguments passed directly to curl
    #[arg(last = true)]
    pub extra_args: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Args::command().debug_assert();
    }
}
