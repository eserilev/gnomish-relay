//! A model call of the story program to a local model, such as Ollama or LM Studio,
//! through the OpenAI-compatible `POST /v1/chat/completions` (SPEC.md 9.7, decision
//! 10). `curl` makes the request, so the bridge needs no HTTP crate. The answer is
//! hostile text.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use serde::Deserialize;
use serde_json::json;

use crate::agent::StopSignal;
use crate::process::{self, Exchange};
use crate::program::find_program;

/// The JSON around an answer of `model::MAX_ANSWER` bytes fits many times over.
pub const MAX_HTTP_ANSWER: usize = 256 * 1024;
const PATH: &str = "/v1/chat/completions";

/// Where the local model listens. `url` is `http://127.0.0.1:<port>` or
/// `http://[::1]:<port>`, checked at config load.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalModel {
    pub url: String,
    pub model: String,
}

/// Only the literal loopback addresses. `localhost` can resolve to another host
/// through the hosts file or DNS.
pub fn check_url(url: &str) -> Option<String> {
    let port = url
        .strip_prefix("http://127.0.0.1:")
        .or_else(|| url.strip_prefix("http://[::1]:"))?;
    let digits = !port.is_empty() && port.len() <= 5 && port.bytes().all(|b| b.is_ascii_digit());
    let open = digits && port.parse::<u16>().is_ok_and(|p| p != 0);
    open.then(|| url.to_owned())
}

pub fn request_body(model: &str, prompt: &str) -> Vec<u8> {
    let body = json!({
        "model": model,
        "messages": [{ "role": "user", "content": prompt }],
        "stream": false,
    });
    body.to_string().into_bytes()
}

/// `-q` comes first, so no `.curlrc` applies. The body comes through stdin, so the
/// prompt never shows in the arguments. `curl` never follows a redirect, and never
/// uses a proxy.
pub fn curl_command(curl: &Path, url: &str, timeout: Duration) -> Command {
    let mut command = process::allowlisted(curl, &[]);
    let seconds = timeout.as_secs().max(1).to_string();
    command.args([
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
        "--show-error",
        "--fail",
        "--header",
        "Content-Type: application/json",
        "--data-binary",
        "@-",
        &format!("{url}{PATH}"),
    ]);
    command
}

pub fn ask(
    local: &LocalModel,
    prompt: &str,
    timeout: Duration,
    stop: StopSignal,
) -> Result<String, String> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let curl =
        find_program("curl", &path, cfg!(windows)).ok_or("Cannot start curl: not found on PATH")?;
    let command = curl_command(&curl, &local.url, timeout);
    send(command, &local.model, prompt, timeout, stop)
}

/// Runs a `curl_command`. Tests add variables to it, such as a proxy.
pub fn send(
    command: Command,
    model: &str,
    prompt: &str,
    timeout: Duration,
    stop: StopSignal,
) -> Result<String, String> {
    let limits = Exchange {
        max_output: MAX_HTTP_ANSWER,
        timeout,
        stop,
    };
    let bytes = process::exchange(command, request_body(model, prompt), &limits)?;
    read_answer(&bytes)
}

#[derive(Deserialize)]
struct Completion {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Deserialize)]
struct Message {
    content: Option<String>,
}

/// The text of the first choice. Other fields can come and go between servers, so
/// they do not count.
pub fn read_answer(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() > MAX_HTTP_ANSWER {
        return Err(process::OVER_LIMIT.into());
    }
    let completion: Completion = serde_json::from_slice(bytes)
        .map_err(|_| "The local model sent an answer that the bridge cannot read.".to_owned())?;
    let first = completion.choices.into_iter().next();
    first
        .and_then(|choice| choice.message.content)
        .ok_or_else(|| "The local model sent no text.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_literal_loopback_address_with_a_port_is_a_local_url() {
        for good in [
            "http://127.0.0.1:11434",
            "http://[::1]:1234",
            "http://127.0.0.1:65535",
        ] {
            assert_eq!(check_url(good).as_deref(), Some(good));
        }
        for bad in [
            "http://localhost:11434",
            "http://127.0.0.2:11434",
            "http://example.com:80",
            "https://127.0.0.1:11434",
            "http://127.0.0.1",
            "http://127.0.0.1:",
            "http://127.0.0.1:0",
            "http://127.0.0.1:65536",
            "http://127.0.0.1:11434/",
            "http://127.0.0.1:11434@evil.test",
            "http://127.0.0.1:11434/v1",
            "http://127.0.0.1:+80",
            "http://[::1]:80 ",
            "HTTP://127.0.0.1:80",
        ] {
            assert_eq!(check_url(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_answer_is_the_text_of_the_first_choice() {
        let body = br#"{"id":"x","choices":[{"index":0,"message":{"role":"assistant","content":"A wolf howls."},"finish_reason":"stop"},{"message":{"content":"second"}}],"usage":{}}"#;
        assert_eq!(read_answer(body).unwrap(), "A wolf howls.");
    }

    #[test]
    fn an_answer_with_no_text_or_of_another_shape_is_an_error() {
        for body in [
            &br#"{"choices":[]}"#[..],
            br#"{"choices":[{"message":{"content":null}}]}"#,
            br#"{"choices":[{"message":{"content":7}}]}"#,
            br#"{"error":"model not found"}"#,
            b"<html>302</html>",
            b"",
        ] {
            assert!(
                read_answer(body).is_err(),
                "{}",
                String::from_utf8_lossy(body)
            );
        }
    }

    #[test]
    fn an_answer_over_the_limit_is_refused_before_it_is_read() {
        let body = format!(
            r#"{{"choices":[{{"message":{{"content":"{}"}}}}]}}"#,
            "x".repeat(MAX_HTTP_ANSWER)
        );
        assert_eq!(
            read_answer(body.as_bytes()),
            Err(process::OVER_LIMIT.into())
        );
    }

    #[test]
    fn the_request_holds_the_model_and_the_prompt_as_one_user_message() {
        let body: serde_json::Value =
            serde_json::from_slice(&request_body("llama3.2", "tell \"me\"")).unwrap();
        assert_eq!(body["model"], "llama3.2");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "tell \"me\"");
        assert_eq!(body["stream"], false);
    }

    #[test]
    fn curl_starts_with_q_and_never_follows_a_redirect_or_uses_a_proxy() {
        let command = curl_command(
            Path::new("/usr/bin/curl"),
            "http://127.0.0.1:11434",
            Duration::from_mins(1),
        );
        let args: Vec<String> = command
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args[0], "-q");
        let after = |flag: &str| {
            let at = args.iter().position(|a| a == flag).unwrap();
            args[at + 1].as_str()
        };
        assert_eq!(after("--proto"), "=http");
        assert_eq!(after("--max-redirs"), "0");
        assert_eq!(after("--noproxy"), "*");
        assert_eq!(after("--max-time"), "60");
        assert_eq!(after("--data-binary"), "@-");
        assert!(!args.iter().any(|a| a == "-L" || a == "--location"));
        assert_eq!(
            args.last().unwrap(),
            "http://127.0.0.1:11434/v1/chat/completions"
        );
    }
}
