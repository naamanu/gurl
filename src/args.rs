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
  gurl -d '{\"name\":\"foo\"}' :3000/api          POST with JSON body
  gurl POST :3000/users name=Jo age:=30        POST {\"name\":\"Jo\",\"age\":30}
  gurl :3000/search q==rust X-Api-Key:abc      Query parameter and header
  gurl --form :3000/upload title=Hi doc@a.pdf  Multipart file upload
  gurl -f request.json --var TOKEN=abc         Request file with {{TOKEN}}
  gurl run api.json                            List a collection's requests
  gurl run api.json create name=Jo             Run one, adding a body field
  gurl --fail :3000/health && echo up          Exit non-zero on HTTP 4xx/5xx
  gurl --bearer \"$TOKEN\" api.example.com/me    Bearer token
  gurl -L -t 30 https://example.com            Follow redirects, 30s timeout
  gurl -s https://api.example.com | jq .       Only the response, for scripts
  gurl --dry-run -d @body.json :3000/api       Show the curl command, don't run it
  gurl https://example.com -- -k --compressed  Pass extra args to curl

Request items (after the URL):
  Name:value     header (Name: with no value drops a header curl would send)
  name==value    query parameter
  name=value     string field of the JSON body (or form field with --form)
  name:=json     raw JSON field: numbers, booleans, null, arrays, objects
  name=@path     string field read from a file (name:=@path: JSON file)
  name@path      file upload (--form)

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
    /// Run a named request from a collection, or list them
    ///
    /// A collection is a JSON file of named requests:
    /// {"vars": {...}, "defaults": {"headers": {...}}, "requests": {"name": {...}}}.
    /// Each request has the same fields as a --file request file. Request
    /// items and flags after the name are applied on top, as with --file.
    #[command(
        after_help = "Examples:\n  gurl run api.json                  List requests\n  gurl run api.json login            Run `login`\n  gurl run api.json get-user id==7   Add a query parameter\n  TOKEN=abc gurl run api.json me     Set a {{TOKEN}} variable"
    )]
    Run {
        /// The collection file
        collection: PathBuf,

        /// The request to run; leave out to list them
        name: Option<String>,

        #[command(flatten)]
        args: Box<Args>,
    },

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
    /// [METHOD] URL [ITEM]...: an optional method (GET, POST, ...), the URL
    /// (:port/path is short for localhost; can be omitted when --file has
    /// one), then request items
    #[arg(value_name = "REQUEST")]
    pub targets: Vec<String>,

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

    /// Query parameter NAME=VALUE, URL-encoded (can be used multiple times)
    #[arg(short = 'q', long = "query", value_name = "NAME=VALUE")]
    pub query: Vec<String>,

    /// Send `Authorization: Bearer TOKEN`
    #[arg(long, value_name = "TOKEN")]
    pub bearer: Option<String>,

    /// Send and accept JSON: sets Content-Type and Accept, even when -d
    /// doesn't look like JSON
    #[arg(long, conflicts_with = "form")]
    pub json: bool,

    /// Send request items as a form: URL-encoded, or multipart when a file
    /// (name@path) is attached
    #[arg(long)]
    pub form: bool,

    /// Load request from JSON file (headers, body, method, url)
    #[arg(short, long)]
    pub file: Option<PathBuf>,

    /// Define a {{NAME}} variable for the request file (can be used multiple
    /// times). Also looked up: environment variables, then --env-file
    #[arg(long = "var", value_name = "NAME=VALUE")]
    pub vars: Vec<String>,

    /// Read {{NAME}} variables from a .env-style file of NAME=VALUE lines
    #[arg(long, value_name = "PATH")]
    pub env_file: Option<PathBuf>,

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
        let cli = parse(&["-L", "POST", "example.com", "a=1", "--", "-k"]);
        assert!(cli.command.is_none());
        assert_eq!(cli.args.targets, vec!["POST", "example.com", "a=1"]);
        assert_eq!(cli.args.extra_args, vec!["-k"]);
    }

    #[test]
    fn run_subcommand_takes_name_items_and_flags() {
        let cli = parse(&["run", "api.json", "login", "-v", "user=jo", "--", "-k"]);
        let Some(Command::Run {
            collection,
            name,
            args,
        }) = cli.command
        else {
            panic!("expected run");
        };
        assert_eq!(collection, PathBuf::from("api.json"));
        assert_eq!(name.as_deref(), Some("login"));
        assert!(args.verbose);
        assert_eq!(args.targets, vec!["user=jo"]);
        assert_eq!(args.extra_args, vec!["-k"]);
    }

    #[test]
    fn run_without_a_name_lists() {
        let cli = parse(&["run", "api.json"]);
        assert!(matches!(cli.command, Some(Command::Run { name: None, .. })));
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
    fn json_and_form_conflict() {
        assert!(Cli::try_parse_from(["gurl", "--json", "--form", "x"]).is_err());
    }

    #[test]
    fn pretty_and_raw_conflict() {
        assert!(Cli::try_parse_from(["gurl", "--pretty", "--raw", "x"]).is_err());
    }
}
