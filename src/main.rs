use clap::Parser;
use gurl::args::Args;
use gurl::exec::{self, RunOptions};
use gurl::{curl, format, request};

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let request_file = args
        .file
        .as_deref()
        .map(request::load_request_file)
        .transpose()?;

    let req = request::resolve(&args, request_file.as_ref())?;

    // What gets printed about the request; credentials masked unless asked
    let shown = if args.show_secrets {
        req.clone()
    } else {
        req.masked()
    };

    if args.dry_run {
        println!("{}", curl::display_command(&shown));
        return Ok(());
    }

    if !args.silent {
        format::print_request_info(&shown, args.file.as_deref());
    }

    let opts = RunOptions {
        pretty: args.pretty,
        silent: args.silent,
        show_secrets: args.show_secrets,
    };
    let exit_code = exec::run(&req, &opts)?;

    if exit_code != 0 {
        std::process::exit(exit_code);
    }
    Ok(())
}
