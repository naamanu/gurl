//! `{{NAME}}` variables in request files, and `.env` files to define them.

use anyhow::{Context, bail};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Where variables come from, highest precedence first:
/// `--var`, the process environment, `--env-file`, then a collection's `vars`.
#[derive(Debug, Default)]
pub struct Vars {
    pub cli: HashMap<String, String>,
    pub use_env: bool,
    pub env_file: HashMap<String, String>,
    pub defaults: HashMap<String, String>,
}

impl Vars {
    pub fn get(&self, name: &str) -> Option<String> {
        if let Some(value) = self.cli.get(name) {
            return Some(value.clone());
        }
        if self.use_env
            && let Ok(value) = std::env::var(name)
        {
            return Some(value);
        }
        self.env_file
            .get(name)
            .or_else(|| self.defaults.get(name))
            .cloned()
    }

    /// Replace every `{{NAME}}` (spaces inside the braces allowed) in `text`.
    /// `\{{` stands for a literal `{{`. Braces around something that isn't a
    /// variable name (`{{#each}}`) are left alone.
    pub fn substitute(&self, text: &str) -> anyhow::Result<String> {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;

        while let Some(start) = rest.find("{{") {
            if rest[..start].ends_with('\\') {
                out.push_str(&rest[..start - 1]);
                out.push_str("{{");
                rest = &rest[start + 2..];
                continue;
            }
            out.push_str(&rest[..start]);
            let after = &rest[start + 2..];
            let Some(end) = after.find("}}") else {
                out.push_str("{{");
                rest = after;
                continue;
            };
            let name = after[..end].trim();
            if !is_var_name(name) {
                out.push_str("{{");
                rest = after;
                continue;
            }
            match self.get(name) {
                Some(value) => out.push_str(&value),
                None => bail!(
                    "Undefined variable `{name}`: set it with --var {name}=..., \
                     in the environment, or in an --env-file"
                ),
            }
            rest = &after[end + 2..];
        }
        out.push_str(rest);
        Ok(out)
    }
}

fn is_var_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Parse `--var NAME=VALUE` arguments.
pub fn parse_cli_vars(pairs: &[String]) -> anyhow::Result<HashMap<String, String>> {
    pairs
        .iter()
        .map(|pair| {
            let (name, value) = pair
                .split_once('=')
                .with_context(|| format!("--var `{pair}`: expected NAME=VALUE"))?;
            if !is_var_name(name) {
                bail!("--var `{pair}`: `{name}` is not a valid variable name");
            }
            Ok((name.to_string(), value.to_string()))
        })
        .collect()
}

pub fn load_env_file(path: &Path) -> anyhow::Result<HashMap<String, String>> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read env file: {}", path.display()))?;
    parse_env(&content).with_context(|| format!("Failed to parse env file: {}", path.display()))
}

/// `KEY=value` lines, as in a `.env` file: `#` comments, optional `export `,
/// single quotes (literal) and double quotes (with `\n \t \" \\` escapes).
pub fn parse_env(content: &str) -> anyhow::Result<HashMap<String, String>> {
    let mut vars = HashMap::new();
    for (index, line) in content.lines().enumerate() {
        let lineno = index + 1;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((name, value)) = line.split_once('=') else {
            bail!("line {lineno}: expected KEY=VALUE");
        };
        let name = name.trim();
        if !is_var_name(name) {
            bail!("line {lineno}: `{name}` is not a valid variable name");
        }
        let value = parse_env_value(value.trim_start())
            .with_context(|| format!("line {lineno}: unterminated quote"))?;
        vars.insert(name.to_string(), value);
    }
    Ok(vars)
}

fn parse_env_value(raw: &str) -> Option<String> {
    if let Some(inner) = raw.strip_prefix('\'') {
        return inner.find('\'').map(|end| inner[..end].to_string());
    }
    if let Some(inner) = raw.strip_prefix('"') {
        let mut value = String::new();
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            match c {
                '"' => return Some(value),
                '\\' => match chars.next()? {
                    'n' => value.push('\n'),
                    't' => value.push('\t'),
                    other => value.push(other),
                },
                c => value.push(c),
            }
        }
        return None;
    }
    // Unquoted: a ` #` starts a comment
    let value = match raw.find(" #") {
        Some(pos) => &raw[..pos],
        None => raw,
    };
    Some(value.trim_end().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> Vars {
        Vars {
            cli: pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn substitutes_variables() {
        let v = vars(&[("HOST", "api.test"), ("TOKEN", "abc")]);
        assert_eq!(
            v.substitute("https://{{HOST}}/me?t={{ TOKEN }}").unwrap(),
            "https://api.test/me?t=abc"
        );
        assert_eq!(v.substitute("no variables").unwrap(), "no variables");
        assert_eq!(
            v.substitute("{{HOST}}{{HOST}}").unwrap(),
            "api.testapi.test"
        );
    }

    #[test]
    fn undefined_variable_is_an_error_naming_it() {
        let err = vars(&[]).substitute("Bearer {{TOKEN}}").unwrap_err();
        assert!(err.to_string().contains("`TOKEN`"), "{err}");
    }

    #[test]
    fn leaves_non_variables_alone() {
        let v = vars(&[]);
        assert_eq!(v.substitute("{{#each items}}").unwrap(), "{{#each items}}");
        assert_eq!(v.substitute("{{ 1 + 2 }}").unwrap(), "{{ 1 + 2 }}");
        assert_eq!(
            v.substitute("unterminated {{X").unwrap(),
            "unterminated {{X"
        );
        assert_eq!(v.substitute("{}").unwrap(), "{}");
    }

    #[test]
    fn backslash_escapes_braces() {
        let v = vars(&[("X", "1")]);
        assert_eq!(v.substitute(r"\{{X}} is {{X}}").unwrap(), "{{X}} is 1");
    }

    #[test]
    fn substituted_values_are_not_rescanned() {
        let v = vars(&[("A", "{{B}}")]);
        assert_eq!(v.substitute("{{A}}").unwrap(), "{{B}}");
    }

    #[test]
    fn precedence_is_cli_then_env_file_then_defaults() {
        let v = Vars {
            cli: HashMap::from([("A".into(), "cli".into())]),
            use_env: false,
            env_file: HashMap::from([("A".into(), "file".into()), ("B".into(), "file".into())]),
            defaults: HashMap::from([
                ("A".into(), "default".into()),
                ("B".into(), "default".into()),
                ("C".into(), "default".into()),
            ]),
        };
        assert_eq!(v.get("A").as_deref(), Some("cli"));
        assert_eq!(v.get("B").as_deref(), Some("file"));
        assert_eq!(v.get("C").as_deref(), Some("default"));
        assert_eq!(v.get("D"), None);
    }

    #[test]
    fn process_environment_sits_between_cli_and_env_file() {
        // PATH is set in any test environment
        let path = std::env::var("PATH").unwrap();
        let v = Vars {
            use_env: true,
            env_file: HashMap::from([("PATH".into(), "file".into())]),
            ..Default::default()
        };
        assert_eq!(v.get("PATH"), Some(path));

        let v = Vars {
            cli: HashMap::from([("PATH".into(), "cli".into())]),
            use_env: true,
            ..Default::default()
        };
        assert_eq!(v.get("PATH").as_deref(), Some("cli"));
    }

    #[test]
    fn parse_cli_vars_splits_on_the_first_equals() {
        let parsed = parse_cli_vars(&["A=1".into(), "B=x=y".into(), "C=".into()]).unwrap();
        assert_eq!(parsed["A"], "1");
        assert_eq!(parsed["B"], "x=y");
        assert_eq!(parsed["C"], "");
        assert!(parse_cli_vars(&["nope".into()]).is_err());
        assert!(parse_cli_vars(&["1BAD=x".into()]).is_err());
    }

    #[test]
    fn parse_env_handles_the_usual_syntax() {
        let env = parse_env(
            r#"
# comment
PLAIN=value
export EXPORTED=yes
SPACED = padded value   # trailing comment
SINGLE='literal $HOME \n # not a comment'
DOUBLE="line1\nline2 \"quoted\" # kept"
EMPTY=
URL=https://example.com/#anchor
"#,
        )
        .unwrap();
        assert_eq!(env["PLAIN"], "value");
        assert_eq!(env["EXPORTED"], "yes");
        assert_eq!(env["SPACED"], "padded value");
        assert_eq!(env["SINGLE"], r"literal $HOME \n # not a comment");
        assert_eq!(env["DOUBLE"], "line1\nline2 \"quoted\" # kept");
        assert_eq!(env["EMPTY"], "");
        assert_eq!(env["URL"], "https://example.com/#anchor");
    }

    #[test]
    fn parse_env_reports_the_line() {
        let err = parse_env("A=1\nnot a pair\n").unwrap_err();
        assert!(err.to_string().contains("line 2"), "{err}");
        let err = parse_env("A=\"open").unwrap_err();
        assert!(format!("{err:#}").contains("unterminated"), "{err:#}");
    }
}
