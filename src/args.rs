use clap::{Parser, Subcommand};
use clap_complete::Shell;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "A simple curl wrapper for easier terminal usage",
    long_about = None,
    // `gurl <url>` unless the first word is a subcommand
    args_conflicts_with_subcommands = true,
    after_help = "\
Examples:
  gurl https://api.example.com/users          GET request (default)
  gurl :8080/health                            Localhost shorthand
  gurl -m POST -d '{\"name\":\"foo\"}' :3000/api  POST with JSON body
  gurl -f request.json                         Load request from file
  gurl --fail :3000/health && echo up          Exit non-zero on HTTP 4xx/5xx
  gurl -H 'Authorization: Bearer tok' api.com  Custom header
  gurl -L -t 30 https://example.com            Follow redirects, 30s timeout
  gurl -s https://api.example.com | jq .       Only the response, for scripts
  gurl --dry-run -d @body.json :3000/api       Show the curl command, don't run it
  gurl https://example.com -- -k --compressed  Pass extra args to curl

JSON responses are pretty-printed when stdout is a terminal; use --raw to
turn that off, or --pretty to force it when piping."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    #[command(flatten)]
    pub args: Args,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Print a shell completion script
    ///
    /// e.g. `gurl completions zsh > ~/.zfunc/_gurl`,
    /// `gurl completions fish > ~/.config/fish/completions/gurl.fish`
    Completions {
        #[arg(value_enum)]
        shell: Shell,
    },
}

#[derive(clap::Args, Debug)]
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

    /// Pretty print JSON output with syntax highlighting, even when piped
    #[arg(short, long, conflicts_with = "raw")]
    pub pretty: bool,

    /// Print the response exactly as received, even on a terminal
    #[arg(long)]
    pub raw: bool,

    /// Exit with code 22 when the server answers with HTTP 4xx or 5xx
    /// (the response is still printed)
    #[arg(long)]
    pub fail: bool,

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

    fn parse(argv: &[&str]) -> Cli {
        Cli::try_parse_from(std::iter::once("gurl").chain(argv.iter().copied())).unwrap()
    }

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn url_without_subcommand() {
        let cli = parse(&["-L", "example.com", "--", "-k"]);
        assert!(cli.command.is_none());
        assert_eq!(cli.args.url.as_deref(), Some("example.com"));
        assert_eq!(cli.args.extra_args, vec!["-k"]);
    }

    #[test]
    fn completions_subcommand() {
        let cli = parse(&["completions", "fish"]);
        assert!(matches!(
            cli.command,
            Some(Command::Completions { shell: Shell::Fish })
        ));
    }

    #[test]
    fn pretty_and_raw_conflict() {
        assert!(Cli::try_parse_from(["gurl", "--pretty", "--raw", "x"]).is_err());
    }
}
