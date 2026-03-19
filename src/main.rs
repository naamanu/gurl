use clap::Parser;
use gurl::args::{self, Args};
use gurl::curl::CurlRequest;
use gurl::{expand_url, has_content_type_header, looks_like_json};

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Load request file if specified
    let request_file = match args.file {
        Some(ref path) => Some(args::load_request_file(path)?),
        None => None,
    };

    // Resolve URL (CLI overrides file)
    let url = args
        .url
        .clone()
        .or_else(|| request_file.as_ref().and_then(|r| r.url.clone()))
        .ok_or_else(|| anyhow::anyhow!("URL is required (provide as argument or in --file)"))?;
    let url = expand_url(&url);

    // Resolve method (CLI overrides file, default GET)
    let method = args
        .method
        .clone()
        .or_else(|| request_file.as_ref().and_then(|r| r.method.clone()))
        .unwrap_or_else(|| "GET".to_string())
        .to_uppercase();

    // Merge headers (file headers first, then CLI headers)
    let mut all_headers: Vec<String> = request_file
        .as_ref()
        .map(|r| r.headers.to_vec())
        .unwrap_or_default();
    all_headers.extend(args.headers.iter().cloned());

    // Resolve body (CLI -d overrides file body)
    let body_data: Option<String> = args.data.clone().or_else(|| {
        request_file.as_ref().and_then(|r| {
            r.body.as_ref().map(|b| {
                b.as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| serde_json::to_string(b).unwrap_or_default())
            })
        })
    });

    // Auto-detect JSON and add Content-Type if missing
    if let Some(ref data) = body_data
        && looks_like_json(data)
        && !has_content_type_header(&all_headers)
    {
        all_headers.push("Content-Type: application/json".to_string());
    }

    // Display request info
    gurl::format::print_request_info(
        &method,
        &url,
        &all_headers,
        body_data.as_deref(),
        args.verbose,
        args.file.as_deref(),
    );

    // Execute
    let req = CurlRequest {
        method,
        url,
        headers: all_headers,
        body: body_data,
        verbose: args.verbose,
        include: args.include,
        location: args.location,
        timeout: args.timeout,
        output: args.output,
        user: args.user,
        pretty: args.pretty,
        extra_args: args.extra_args,
    };

    gurl::curl::execute_request(&req)
}
