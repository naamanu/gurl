use anyhow::Context;
use clap::{CommandFactory, Parser};
use gurl::args::{Args, Cli, Command};
use gurl::exec::{self, PrettyChoice, RunOptions};
use gurl::vars::{self, Vars};
use gurl::{curl, format, request};

/// Exit code for an HTTP error response under `--fail`, the same as curl's
const HTTP_ERROR_EXIT: i32 = 22;

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let exit_code = match cli.command {
        Some(Command::Completions { shell }) => {
            clap_complete::generate(shell, &mut Cli::command(), "gurl", &mut std::io::stdout());
            0
        }
        None => send(&cli.args)?,
    };

    if exit_code != 0 {
        std::process::exit(exit_code);
    }
    Ok(())
}

fn send(args: &Args) -> anyhow::Result<i32> {
    let vars = Vars {
        cli: vars::parse_cli_vars(&args.vars)?,
        use_env: true,
        env_file: args
            .env_file
            .as_deref()
            .map(vars::load_env_file)
            .transpose()?
            .unwrap_or_default(),
        defaults: Default::default(),
    };

    let request_file = args
        .file
        .as_deref()
        .map(|path| {
            request::load_request_file(path)?
                .substitute(&vars)
                .with_context(|| format!("In request file {}", path.display()))
        })
        .transpose()?;

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
        format::print_request_info(&shown, args.file.as_deref());
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
