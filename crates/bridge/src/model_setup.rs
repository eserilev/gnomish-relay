//! The models that setup finds for the `[story]` section of Timeways (SPEC.md 11.3):
//! `claude` on `PATH`, then Ollama, then LM Studio on the loopback. The bridge itself
//! never looks up a program on `PATH` for the story program.

use std::ffi::OsStr;
use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use crate::config::is_model_name;
use crate::process;
use crate::program::find_program;

pub const OLLAMA: &str = "http://127.0.0.1:11434";
pub const LM_STUDIO: &str = "http://127.0.0.1:1234";
const PROBE_TIME: Duration = Duration::from_secs(2);
/// Fast enough for the talk of a person in the game, and it spends less of the plan.
pub const CLAUDE_MODEL: &str = "haiku";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FoundModel {
    Claude,
    Local { url: String, model: String },
}

/// Every model that answers, best first. The first one goes into the config.
pub fn find_models(path_var: &OsStr) -> Vec<FoundModel> {
    let mut found = Vec::new();
    if find_program("claude", path_var, cfg!(windows)).is_some() {
        found.push(FoundModel::Claude);
    }
    let Some(curl) = find_program("curl", path_var, cfg!(windows)) else {
        return found;
    };
    for url in [OLLAMA, LM_STUDIO] {
        if let Some(model) = probe(&curl, url) {
            found.push(FoundModel::Local {
                url: url.to_owned(),
                model,
            });
        }
    }
    found
}

/// The first model of a local server, with the same `curl` flags as a model call.
pub fn probe(curl: &Path, url: &str) -> Option<String> {
    let seconds = PROBE_TIME.as_secs().to_string();
    let output = process::allowlisted(curl, &[])
        .args([
            "-q",
            "--proto",
            "=http",
            "--max-redirs",
            "0",
            "--noproxy",
            "*",
            "--max-time",
            &seconds,
            "--silent",
            "--fail",
            &format!("{url}/v1/models"),
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    first_chat_model(&output.stdout)
}

/// The first id of an OpenAI-style model list that can chat: an embedding model cannot.
pub fn first_chat_model(body: &[u8]) -> Option<String> {
    let list: Value = serde_json::from_slice(body).ok()?;
    let ids = list.get("data")?.as_array()?;
    ids.iter()
        .filter_map(|item| item.get("id")?.as_str())
        .find(|id| !id.contains("embed") && is_model_name(id))
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn the_first_chat_model_skips_embedding_models_and_bad_names() {
        let body =
            br#"{"data":[{"id":"nomic-embed-text"},{"id":"-x"},{"id":"llama3.2"},{"id":"qwen3"}]}"#;
        assert_eq!(first_chat_model(body), Some("llama3.2".into()));
    }

    #[test]
    fn a_list_with_no_chat_model_or_no_list_gives_none() {
        assert_eq!(first_chat_model(br#"{"data":[{"id":"bge-embed"}]}"#), None);
        assert_eq!(first_chat_model(br#"{"data":[]}"#), None);
        assert_eq!(first_chat_model(b"<html>"), None);
        assert_eq!(first_chat_model(br#"{"models":[]}"#), None);
    }

    fn curl() -> Option<std::path::PathBuf> {
        let path = std::env::var_os("PATH").unwrap_or_default();
        find_program("curl", &path, cfg!(windows))
    }

    /// A server that answers one request with `body`.
    fn server(body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request);
            let answer = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(answer.as_bytes());
        });
        url
    }

    #[test]
    fn a_probe_gets_the_first_model_of_a_local_server() {
        let Some(curl) = curl() else {
            return;
        };
        let url = server(r#"{"object":"list","data":[{"id":"llama3.2"}]}"#);
        assert_eq!(probe(&curl, &url), Some("llama3.2".into()));
    }

    #[test]
    fn a_probe_of_a_port_with_no_server_gives_none() {
        let Some(curl) = curl() else {
            return;
        };
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        assert_eq!(probe(&curl, &format!("http://127.0.0.1:{port}")), None);
    }

    #[test]
    fn claude_on_the_path_is_found_first() {
        let dir = tempfile::tempdir().unwrap();
        let name = if cfg!(windows) {
            "claude.exe"
        } else {
            "claude"
        };
        std::fs::write(dir.path().join(name), "").unwrap();
        assert_eq!(
            find_models(dir.path().as_os_str()).first(),
            Some(&FoundModel::Claude)
        );
    }
}
