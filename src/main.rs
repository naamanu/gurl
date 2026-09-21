use clap::Parser;
use gurl::args::Args;
use gurl::exec::{self, RunOptions};
use gurl::{format, request};

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let request_file = args
        .file
        .as_deref()
        .map(request::load_request_file)
        .transpose()?;

    let req = request::resolve(&args, request_file.as_ref())?;

    if !args.silent {
        format::print_request_info(&req, args.file.as_deref());
    }

    let opts = RunOptions {
        pretty: args.pretty,
        silent: args.silent,
    };
    let exit_code = exec::run(&req, &opts)?;

    if exit_code != 0 {
        std::process::exit(exit_code);
    }
    Ok(())
}
