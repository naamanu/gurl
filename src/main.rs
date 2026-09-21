use anyhow::{Context, bail};
use clap::{CommandFactory, Parser};
use colored::*;
use gurl::args::{Args, Cli, Command};
use gurl::collection::{self, Collection};
use gurl::exec::{self, PrettyChoice, RunOptions};
use gurl::request::RequestFile;
use gurl::vars::{self, Vars};
use gurl::{curl, format, request};
use std::collections::HashMap;
use std::path::Path;

/// Exit code for an HTTP error response under `--fail`, the same as curl's
const HTTP_ERROR_EXIT: i32 = 22;

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let exit_code = match cli.command {
        Some(Command::Completions { shell }) => {
            clap_complete::generate(shell, &mut Cli::command(), "gurl", &mut std::io::stdout());
            0
        }
        Some(Command::Run {
            collection,
            name,
            args,
        }) => run_collection(&collection, name.as_deref(), &args)?,
        None => {
            // `gurl -s run x.json` parses `run` as a URL: options before a
            // subcommand turn subcommands off
            if let Some(word) = cli.args.targets.first()
                && Cli::command()
                    .get_subcommands()
                    .any(|sub| sub.get_name() == word)
            {
                bail!(
                    "Put options after the subcommand, e.g. `gurl {word} ... -s`. \
                     To request a host named `{word}`, write https://{word}"
                );
            }
            let file = match &cli.args.file {
                Some(path) => Some(Source {
                    request: request::load_request_file(path)?,
                    label: format!("Loading from {}", path.display()),
                    context: format!("In request file {}", path.display()),
                    vars: HashMap::new(),
                }),
                None => None,
            };
            send(&cli.args, file)?
        }
    };

    if exit_code != 0 {
        std::process::exit(exit_code);
    }
    Ok(())
}

/// A request read from a file, before variables are substituted
struct Source {
    request: RequestFile,
    /// For the banner
    label: String,
    /// For error messages
    context: String,
    /// Lowest-precedence variables (a collection's `vars`)
    vars: HashMap<String, String>,
}

fn run_collection(path: &Path, name: Option<&str>, args: &Args) -> anyhow::Result<i32> {
    if args.file.is_some() {
        bail!("--file can't be used with `gurl run`: the request comes from the collection");
    }
    let collection = collection::load_collection(path)?;

    let Some(name) = name else {
        print_listing(&collection);
        return Ok(0);
    };

    let request = collection
        .request(name)
        .with_context(|| format!("In collection {}", path.display()))?;
    let source = Source {
        request,
        label: format!("{} › {name}", path.display()),
        context: format!("In request `{name}` of collection {}", path.display()),
        vars: collection.vars,
    };
    send(args, Some(source))
}

fn print_listing(collection: &Collection) {
    let summaries = collection.summaries();
    let width = |f: fn(&collection::Summary) -> usize| summaries.iter().map(f).max().unwrap_or(0);
    let name_width = width(|s| s.name.len());
    let method_width = width(|s| s.method.len());
    let url_width = width(|s| s.url.len());
    for summary in &summaries {
        // Pad before coloring: escape codes would count towards the width
        let mut line = format!(
            "{}  {:method_width$}  {:url_width$}",
            format!("{:name_width$}", summary.name).bold(),
            summary.method,
            summary.url
        );
        if let Some(description) = &summary.description {
            line.push_str(&format!("  {}", description.dimmed()));
        }
        println!("{}", line.trim_end());
    }
}

fn send(args: &Args, source: Option<Source>) -> anyhow::Result<i32> {
    let (request_file, label) = match source {
        Some(source) => {
            let vars = Vars {
                cli: vars::parse_cli_vars(&args.vars)?,
                use_env: true,
                env_file: args
                    .env_file
                    .as_deref()
                    .map(vars::load_env_file)
                    .transpose()?
                    .unwrap_or_default(),
                defaults: source.vars,
            };
            let request = source
                .request
                .substitute(&vars)
                .with_context(|| source.context.clone())?;
            (Some(request), Some(source.label))
        }
        None => (None, None),
    };

    let req = request::resolve(args, request_file.as_ref())?;

    // What gets printed about the request; credentials masked unless asked
    let shown = if args.show_secrets {
        req.clone()
    } else {
        req.masked()
    };

    if args.dry_run {
        println!("{}", curl::display_command(&shown));
        return Ok(0);
    }

    if !args.silent {
        format::print_request_info(&shown, label.as_deref());
    }

    let opts = RunOptions {
        pretty: if args.pretty {
            PrettyChoice::Always
        } else if args.raw {
            PrettyChoice::Never
        } else {
            PrettyChoice::Auto
        },
        silent: args.silent,
        show_secrets: args.show_secrets,
    };
    let outcome = exec::run(&req, &opts)?;

    Ok(exit_code(outcome.exit_code, outcome.meta.status, args.fail))
}

fn exit_code(curl_exit_code: i32, status: Option<u16>, fail: bool) -> i32 {
    if curl_exit_code == 0 && fail && status.is_some_and(|code| code >= 400) {
        HTTP_ERROR_EXIT
    } else {
        curl_exit_code
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fail_turns_http_errors_into_exit_22() {
        assert_eq!(exit_code(0, Some(404), true), 22);
        assert_eq!(exit_code(0, Some(503), true), 22);
        assert_eq!(exit_code(0, Some(399), true), 0);
        assert_eq!(exit_code(0, None, true), 0);
    }

    #[test]
    fn without_fail_http_errors_exit_zero() {
        assert_eq!(exit_code(0, Some(404), false), 0);
    }

    #[test]
    fn curl_errors_take_precedence() {
        assert_eq!(exit_code(7, None, true), 7);
        assert_eq!(exit_code(28, Some(500), true), 28);
    }
}
