//! `gnomish-relay report`: one file to attach to a bug report, with no secret in it
//! (SPEC.md 8.5). It uploads nothing.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::config;
use crate::dirs::Dirs;
use crate::fs_safe::{make_private_dir, write_private};
use crate::log_command::{has_secret_name, looks_like_token};
use crate::log_file::{LOG_DIR, files_oldest_first};
use crate::receive::{RELAY_KEY_FILE, TIMEWAYS_KEY_FILE};

pub const REPORTS_DIR: &str = "reports";
const REMOVED: &str = "(removed)";
const HOUR: u64 = 3600;
/// The file that a start with no service writes (SPEC.md 11.3).
const BRIDGE_LOG: &str = "bridge.log";
pub const ISSUES: &str = "https://github.com/eserilev/gnomish-relay/issues";

/// Writes the report, and returns its path.
pub fn write_report(dirs: &Dirs, now: u64, status: &[String]) -> Result<PathBuf> {
    let text = report_text(dirs, now, status);
    let folder = dirs.data.join(REPORTS_DIR);
    make_private_dir(&folder)?;
    let name = format!("report-{now}.txt");
    write_private(&folder, &name, &text)?;
    Ok(folder.join(name))
}

pub fn report_text(dirs: &Dirs, now: u64, status: &[String]) -> String {
    let since = now.saturating_sub(HOUR);
    let sections = [
        section("Desktop app", &about_line()),
        section("Status", &status.join("\n")),
        section("config.toml", &config_section(&dirs.config)),
        section("Log of the last hour", &json_log_lines(&dirs.data, since)),
        section(
            "bridge.log of the last hour",
            &bridge_log_lines(&dirs.data, since),
        ),
    ];
    let text = sections.concat();
    let text = without_keys(&text, &dirs.config);
    let text = without_tokens(&text);
    with_home_as_tilde(&text, &dirs.home)
}

/// A TOML error in the status quotes its line, so the config section is not the only
/// place for a token.
fn without_tokens(text: &str) -> String {
    let is_end = |c: char| c.is_whitespace() || "\"'`,;()[]{}".contains(c);
    let mut out = String::with_capacity(text.len());
    for piece in text.split_inclusive(is_end) {
        let word = piece.trim_end_matches(is_end);
        if looks_like_token(word) {
            out.push_str(REMOVED);
            out.push_str(&piece[word.len()..]);
        } else {
            out.push_str(piece);
        }
    }
    out
}

fn section(title: &str, body: &str) -> String {
    let body = if body.trim().is_empty() {
        "(nothing)"
    } else {
        body.trim_end()
    };
    format!("== {title} ==\n{body}\n\n")
}

fn about_line() -> String {
    let wsl = if crate::wsl::this().is_some() {
        " (WSL2)"
    } else {
        ""
    };
    format!(
        "gnomish-relay {}\nOS: {} {}{wsl}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}

fn config_section(config_dir: &Path) -> String {
    let Ok(text) = std::fs::read_to_string(config_dir.join(config::FILE)) else {
        return "No config.toml.".into();
    };
    redacted_config(&text)
}

/// The config as TOML again, so no comment comes along, with each secret as `(removed)`.
pub fn redacted_config(text: &str) -> String {
    let Ok(mut table) = toml::from_str::<toml::Table>(text) else {
        return "config.toml has a TOML error, so the report leaves it out.".into();
    };
    redact_table(&mut table);
    toml::to_string(&table).unwrap_or_default()
}

fn redact_table(table: &mut toml::Table) {
    for (key, value) in table.iter_mut() {
        if has_secret_name(key) {
            remove_strings(value);
        } else {
            redact_value(value);
        }
    }
}

fn redact_value(value: &mut toml::Value) {
    match value {
        toml::Value::String(s) if looks_like_token(s) => *s = REMOVED.into(),
        toml::Value::Array(items) => items.iter_mut().for_each(redact_value),
        toml::Value::Table(table) => redact_table(table),
        _ => {}
    }
}

fn remove_strings(value: &mut toml::Value) {
    match value {
        toml::Value::String(s) => *s = REMOVED.into(),
        toml::Value::Array(items) => items.iter_mut().for_each(remove_strings),
        toml::Value::Table(table) => table.iter_mut().for_each(|(_, v)| remove_strings(v)),
        _ => {}
    }
}

/// The lines of the JSON log files with a `unix` time of `since` or later, oldest first.
fn json_log_lines(data: &Path, since: u64) -> String {
    let mut lines = Vec::new();
    for file in files_oldest_first(&data.join(LOG_DIR)) {
        let text = std::fs::read_to_string(file).unwrap_or_default();
        lines.extend(
            text.lines()
                .filter(|l| json_time(l) >= Some(since))
                .map(str::to_owned),
        );
    }
    lines.join("\n")
}

fn json_time(line: &str) -> Option<u64> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    value.get("unix")?.as_u64()
}

/// `bridge.log` lines start with their unix time (SPEC.md 8.5).
fn bridge_log_lines(data: &Path, since: u64) -> String {
    let text = std::fs::read_to_string(data.join(BRIDGE_LOG)).unwrap_or_default();
    let recent: Vec<&str> = text
        .lines()
        .filter(|l| line_time(l) >= Some(since))
        .collect();
    recent.join("\n")
}

fn line_time(line: &str) -> Option<u64> {
    line.split(' ').next()?.parse().ok()
}

/// No log holds a key. This is the second wall: the text of each key file goes.
fn without_keys(text: &str, config_dir: &Path) -> String {
    let mut text = text.to_owned();
    for name in [RELAY_KEY_FILE, TIMEWAYS_KEY_FILE] {
        let Ok(key) = std::fs::read_to_string(config_dir.join(name)) else {
            continue;
        };
        let key = key.trim();
        if !key.is_empty() {
            text = text.replace(key, REMOVED);
        }
    }
    text
}

fn with_home_as_tilde(text: &str, home: &Path) -> String {
    let home = home.to_string_lossy();
    let home = home.trim_end_matches(['/', '\\']);
    if home.is_empty() {
        return text.to_owned();
    }
    text.replace(home, "~")
}

#[cfg(test)]
mod tests {
    use super::*;

    const STRIP_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const TIMEWAYS_KEY: &str = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

    fn dirs(root: &Path) -> Dirs {
        let dirs = Dirs {
            home: root.join("home"),
            config: root.join("home/.config/gnomish-relay"),
            data: root.join("home/.local/share/gnomish-relay"),
        };
        std::fs::create_dir_all(&dirs.config).unwrap();
        std::fs::create_dir_all(dirs.data.join(LOG_DIR)).unwrap();
        dirs
    }

    #[test]
    fn the_report_holds_no_key_or_token() {
        let root = tempfile::tempdir().unwrap();
        let dirs = dirs(root.path());
        std::fs::write(dirs.config.join(RELAY_KEY_FILE), format!("{STRIP_KEY}\n")).unwrap();
        std::fs::write(dirs.config.join(TIMEWAYS_KEY_FILE), TIMEWAYS_KEY).unwrap();
        let config = format!(
            "# my token: ghp_comment\nallowed_roots = [\"{home}/Code\"]\napi_key = \"plain-secret\"\n\n[agents.claude]\ncommand = [\"claude\", \"sk-ant-abc\"]\nenv = [\"ANTHROPIC_API_KEY\"]\n",
            home = dirs.home.display()
        );
        std::fs::write(dirs.config.join(config::FILE), config).unwrap();
        let log = format!(
            "{{\"unix\":1000,\"line\":\"leaked {STRIP_KEY} in {home}/Code\"}}\n",
            home = dirs.home.display()
        );
        std::fs::write(dirs.data.join(LOG_DIR).join("bridge.jsonl"), log).unwrap();

        let text = report_text(&dirs, 1000, &["Desktop app: running".into()]);

        for secret in [
            STRIP_KEY,
            TIMEWAYS_KEY,
            "plain-secret",
            "sk-ant-abc",
            "ghp_comment",
        ] {
            assert!(!text.contains(secret), "{secret} in:\n{text}");
        }
        assert!(text.contains("allowed_roots = [\"~/Code\"]"), "{text}");
        assert!(text.contains("api_key = \"(removed)\""), "{text}");
        assert!(text.contains("leaked (removed) in ~/Code"), "{text}");
        assert!(text.contains("env = [\"ANTHROPIC_API_KEY\"]"), "{text}");
        assert!(!text.contains(&dirs.home.display().to_string()), "{text}");
    }

    #[test]
    fn the_report_holds_the_version_the_status_and_the_log_of_the_last_hour() {
        let root = tempfile::tempdir().unwrap();
        let dirs = dirs(root.path());
        let logs = dirs.data.join(LOG_DIR);
        std::fs::write(
            logs.join("bridge.jsonl.1"),
            "{\"unix\":6000,\"line\":\"older file\"}\n",
        )
        .unwrap();
        std::fs::write(
            logs.join("bridge.jsonl"),
            "{\"unix\":1000,\"line\":\"too old\"}\nnot json\n{\"unix\":9000,\"line\":\"new\"}\n",
        )
        .unwrap();
        std::fs::write(
            dirs.data.join(BRIDGE_LOG),
            "100 old line\n8000 recent line\n",
        )
        .unwrap();

        let text = report_text(&dirs, 9000, &["Desktop app: stopped".into()]);

        assert!(text.contains(&format!("gnomish-relay {}", env!("CARGO_PKG_VERSION"))));
        assert!(text.contains(&format!("OS: {}", std::env::consts::OS)));
        assert!(
            text.contains("== Status ==\nDesktop app: stopped\n"),
            "{text}"
        );
        assert!(text.contains("No config.toml."), "{text}");
        let older = text.find("older file").unwrap();
        let new = text.find("\"new\"").unwrap();
        assert!(older < new, "{text}");
        assert!(
            !text.contains("too old") && !text.contains("not json"),
            "{text}"
        );
        assert!(
            text.contains("8000 recent line") && !text.contains("old line\n"),
            "{text}"
        );
    }

    #[test]
    fn a_config_that_does_not_parse_stays_out() {
        let text = redacted_config("api_key = \"x\" broken");

        assert_eq!(
            text,
            "config.toml has a TOML error, so the report leaves it out."
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_report_is_a_private_file_in_the_data_folder() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let dirs = dirs(root.path());

        let path = write_report(&dirs, 1234, &[]).unwrap();

        assert_eq!(path, dirs.data.join("reports/report-1234.txt"));
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
