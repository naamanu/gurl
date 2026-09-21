# gurl

A friendlier `curl` for the terminal: pretty JSON, httpie-style request items,
request files and collections with variables, and a status line after every
request. It runs the real `curl` underneath, so any curl flag still works.

```console
$ gurl POST :3000/users name=Jo age:=30

▶ POST http://localhost:3000/users

{
  "id": 7,
  "name": "Jo",
  "age": 30
}

✓ HTTP 201 Created · 12ms · 36 B
```

## Features

- 🎨 **Readable responses** — JSON pretty-printed and colored on a terminal, raw when piped
- ✏️ **Request items** — `name=Jo age:=30 X-Key:abc page==2` instead of hand-written JSON
- 📄 **Request files and collections** — JSON files of requests, with `{{VARIABLES}}` for secrets
- ⏱️ **Status at a glance** — `✓ HTTP 200 OK · 41ms · 1.2 KB` after every request
- 🚦 **Script-friendly** — only the response on stdout; `--fail` exits non-zero on HTTP errors
- 🏠 **Shorthands** — `:3000/api` for localhost, `--bearer`, `-q`, `--json`, `--form`
- 🔍 **No surprises** — `--dry-run` prints the exact curl command, with credentials masked
- 🔗 **Still curl** — pass any curl flag after `--`

## Installation

Prebuilt binaries for Linux, macOS and Windows are attached to each
[GitHub release](https://github.com/naamanu/gurl/releases). Or build from source:

```bash
cargo install --git https://github.com/naamanu/gurl
# or, from a checkout:
cargo install --path .
```

gurl needs `curl` 7.63 or newer on your `PATH` (check with `curl --version`).

Shell completions:

```bash
gurl completions zsh  > ~/.zfunc/_gurl                                  # zsh (with ~/.zfunc in fpath)
gurl completions bash > ~/.local/share/bash-completion/completions/gurl
gurl completions fish > ~/.config/fish/completions/gurl.fish
```

## Usage

### Basics

```bash
gurl https://api.example.com/users     # GET
gurl api.example.com/users             # https:// is implied...
gurl localhost:8080/health             # ...except for local hosts, which get http://
gurl :3000/api/users                   # short for http://localhost:3000/api/users
gurl DELETE :3000/api/users/7          # a method can go before the URL
gurl -L -t 30 example.com              # follow redirects, 30 second timeout
```

### Sending data

```bash
gurl -d '{"name":"Jo"}' :3000/users            # JSON is detected: POST with Content-Type set
gurl -m PUT -d '{"name":"Jo"}' :3000/users/7   # any other method with -m (or a method word)
gurl -d @payload.json :3000/users              # send a file
jq -n '{name: "Jo"}' | gurl -d @- :3000/users  # ...or stdin
```

Sending data implies POST, as with curl. `-d` sends its value exactly as
written (curl's `--data-raw`). Only a leading `@` reads a file, and the file is
sent byte for byte.

### Request items

After the URL, build the request from items instead of writing JSON by hand:

```bash
gurl POST :3000/users name=Jo age:=30 admin:=true tags:='["a","b"]'
# → POST http://localhost:3000/users  {"name":"Jo","age":30,"admin":true,"tags":["a","b"]}

gurl :3000/search q==rust page==2 X-Api-Key:abc
# → GET http://localhost:3000/search?q=rust&page=2  with header X-Api-Key: abc

gurl --form :3000/login user=jo pass=hunter2           # application/x-www-form-urlencoded
gurl --form :3000/upload title=Report doc@report.pdf   # multipart/form-data upload
```

| Item          | Meaning                                                        |
| ------------- | -------------------------------------------------------------- |
| `Name:value`  | Header. `Name:` with nothing after it drops a header curl adds |
| `name==value` | Query parameter, URL-encoded                                   |
| `name=value`  | String field in the JSON body (form field with `--form`)       |
| `name:=json`  | Raw JSON field: number, boolean, null, array, object           |
| `name=@path`  | String field read from a text file                             |
| `name:=@path` | JSON field read from a file                                    |
| `name@path`   | File upload (`--form`, sent as multipart/form-data)            |

Body fields make the request a POST unless a method is given, and send JSON
`Content-Type` and `Accept` headers. They can't be combined with `-d`.

### Shortcuts

```bash
gurl --bearer "$TOKEN" api.example.com/me          # Authorization: Bearer ...
gurl -u admin:secret api.example.com/admin         # basic auth
gurl -H 'Accept: text/csv' api.example.com/export  # any header (repeatable)
gurl -q 'q=rust lang' -q page=2 api.example.com/s  # URL-encoded query parameters
gurl --json -d @payload api.example.com/items      # JSON headers, whatever -d looks like
```

### Request files

Keep a request in a JSON file and run it with `-f`:

```json
{
  "method": "POST",
  "url": "https://{{API_HOST}}/users",
  "headers": { "Authorization": "Bearer {{TOKEN}}" },
  "body": { "name": "Jo", "email": "jo@example.com" }
}
```

```bash
gurl -f create-user.json --var API_HOST=api.example.com
gurl -f create-user.json https://staging.example.com/users   # override the URL
gurl -f create-user.json -m PUT                              # ...or the method
gurl -f create-user.json X-Trace:1 name=Ann                  # add items on top
```

Command-line values win over the file. `headers` can be an object (sent in the
order written; `null` drops a header curl would add) or an array of
`"Name: value"` strings. A string `body` is sent as written, even if it starts
with `@`.

**Variables.** Any string in a request file can contain `{{NAME}}`, so the file
can be shared without the secrets in it. Values are looked up in order:

1. `--var NAME=VALUE`
2. environment variables (`TOKEN=abc gurl -f ...`)
3. `--env-file .env` (`NAME=VALUE` lines, `#` comments, quotes; see [`examples/.env.example`](examples/.env.example))
4. a collection's `vars`

An undefined variable is an error, raised before anything is sent. Values are
inserted into the parsed JSON, so a quote in a value can't break the body.
Write `\\{{` in a JSON string for a literal `{{`. On the command line, use your
shell's `$VARIABLES` instead.

### Collections

A collection keeps several named requests in one file, with shared variables
and default headers ([`examples/collection.json`](examples/collection.json)):

```json
{
  "vars": { "base": "https://jsonplaceholder.typicode.com" },
  "defaults": { "headers": { "Authorization": "Bearer {{TOKEN}}" } },
  "requests": {
    "list-posts": { "url": "{{base}}/posts?_limit=5", "description": "First page of posts" },
    "get-post": { "url": "{{base}}/posts/{{ID}}" },
    "create-post": { "method": "POST", "url": "{{base}}/posts", "body": { "title": "Hi" } }
  }
}
```

```console
$ gurl run examples/collection.json
list-posts   GET     {{base}}/posts?_limit=5  First page of posts
get-post     GET     {{base}}/posts/{{ID}}    One post; pick it with --var ID=3
create-post  POST    {{base}}/posts           Create a post; add fields with title=...
delete-post  DELETE  {{base}}/posts/{{ID}}

$ TOKEN=abc gurl run examples/collection.json get-post --var ID=3
$ TOKEN=abc gurl run examples/collection.json create-post title='Hello' -v
```

Each request takes the same fields as a request file, plus an optional
`description`. Default headers come first. With object headers, a request can
override a default by name or drop it with `null`. Items and flags go after
the request name: `gurl run file name [ITEM]... [OPTIONS]`.

### Output

stdout carries only the response. The request banner and status line go to
stderr, so piping and redirecting work, including for binary and streaming
responses:

```bash
gurl api.example.com/users | jq '.[0]'
gurl example.com/logo.png > logo.png
gurl -s api.example.com/users > users.json   # -s: no banner or status line at all
```

On a terminal, JSON is pretty-printed and colored, with keys kept in the
server's order. `--raw` turns that off, and `--pretty` forces it when piping.
Anything that isn't JSON is passed through untouched as it arrives. Colors
follow the [`NO_COLOR`](https://no-color.org) and `CLICOLOR_FORCE` conventions.

Every request ends with a status line:

```
✓ HTTP 201 Created · 142ms · 1.2 KB
✗ HTTP 404 Not Found · 38ms · 21 B
✗ Failed · 3ms (curl exit code 7)
```

As with curl, an HTTP error response still exits 0. With `--fail` it exits 22
instead, and the body is still printed:

```bash
gurl --fail -s :3000/health > /dev/null || echo "down"
```

Otherwise gurl exits with curl's exit code (for example 6 when the host can't
be resolved, 7 when the connection fails, 28 on a timeout).

### See the curl command

`--dry-run` prints the curl command gurl would run, quoted for pasting into a
shell or a bug report, and sends nothing. `-v` shows the same command before
curl's own trace. `Authorization` headers and `-u` passwords appear as `***`
unless you add `--show-secrets`.

```console
$ gurl --dry-run --bearer abc :3000/items name=Jo
curl -H 'Authorization: ***' -H 'Content-Type: application/json' -H 'Accept: application/json, */*;q=0.5' --data-raw '{"name":"Jo"}' http://localhost:3000/items
```

### Pass-through to curl

Anything after `--` goes to curl unchanged:

```bash
gurl example.com -- --compressed
gurl internal.example.com -- --cacert /path/to/ca.pem
gurl :3000/slow -- --retry 3
```

## Options

```
gurl [OPTIONS] [METHOD] URL [ITEM]... [-- CURL_ARGS]...
gurl run COLLECTION [NAME] [ITEM]... [OPTIONS]
gurl completions SHELL
```

Put options after a subcommand (`gurl run api.json login -v`, not `gurl -v run ...`).

| Flag | Long             | Description                                          |
| ---- | ---------------- | ---------------------------------------------------- |
| `-m` | `--method`       | HTTP method (default GET, or POST when sending data) |
| `-d` | `--data`         | Request body, sent as-is; `@file`, or `@-` for stdin |
| `-H` | `--header`       | Add a header (repeatable; `--headers` also works)    |
| `-q` | `--query`        | Add a URL-encoded query parameter `NAME=VALUE`       |
|      | `--bearer`       | Send `Authorization: Bearer TOKEN`                   |
|      | `--json`         | Send JSON `Content-Type` and `Accept` headers        |
|      | `--form`         | Send request items as a form (multipart with files)  |
| `-u` | `--user`         | Basic auth (`user:password`)                         |
| `-f` | `--file`         | Load the request from a JSON file                    |
|      | `--var`          | Set a `{{NAME}}` variable: `NAME=VALUE`              |
|      | `--env-file`     | Read `{{NAME}}` variables from a `.env` file         |
| `-p` | `--pretty`       | Pretty-print JSON, even when piped                   |
|      | `--raw`          | Never reformat the response, even on a terminal      |
| `-i` | `--include`      | Include response headers                             |
| `-o` | `--output`       | Save the response to a file                          |
| `-s` | `--silent`       | Only the response: no banner, no status line         |
| `-v` | `--verbose`      | Show the curl command, the request and curl's trace  |
|      | `--fail`         | Exit 22 on HTTP 4xx/5xx (the body is still printed)  |
| `-L` | `--location`     | Follow redirects                                     |
| `-t` | `--timeout`      | Request timeout in seconds                           |
|      | `--dry-run`      | Print the equivalent curl command; send nothing      |
|      | `--show-secrets` | Don't mask credentials in `-v` / `--dry-run` output  |

## Development

```bash
cargo test                   # unit tests and end-to-end tests (these need curl)
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all
```

The end-to-end tests in `tests/cli.rs` run the real binary and the real curl
against a small local HTTP server, so they don't need network access.

[pre-commit](https://pre-commit.com) hooks run `cargo fmt` and `cargo clippy`
before each commit:

```bash
pre-commit install
pre-commit run --all-files
```

To release: bump `version` in `Cargo.toml`, add a `CHANGELOG.md` entry, and
push a `vX.Y.Z` tag. The release workflow builds the binaries and publishes
the GitHub release.

## License

[MIT](LICENSE)
