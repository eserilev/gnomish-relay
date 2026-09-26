//! A fake local model: a thread with a `TcpListener` on 127.0.0.1 that answers like the
//! OpenAI-compatible API of Ollama and LM Studio, in one of several ways.

// Each test file uses a different part of this module.
#![allow(dead_code)]
// Clippy sees a shared test module as normal code, so its test exceptions miss it.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[derive(Clone, Copy)]
pub enum Answer {
    /// "heard: <prompt>".
    Normal,
    Slow(Duration),
    /// A text of 1 MiB.
    Huge,
    Garbage,
    /// A 302 to `/elsewhere` on the same server, which would answer normally.
    Redirect,
    Error500,
    /// A text with a bell, a carriage return, and an escape sequence.
    Controls,
}

/// One request that reached the server.
#[derive(Clone, Debug)]
pub struct Request {
    pub path: String,
    pub body: String,
}

pub struct Server {
    pub url: String,
    pub requests: Arc<Mutex<Vec<Request>>>,
}

pub fn start(answer: Answer) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&requests);
    thread::spawn(move || {
        for stream in listener.incoming().map_while(Result::ok) {
            let seen = Arc::clone(&seen);
            thread::spawn(move || serve(stream, answer, port, &seen));
        }
    });
    Server {
        url: format!("http://127.0.0.1:{port}"),
        requests,
    }
}

impl Server {
    pub fn paths(&self) -> Vec<String> {
        let requests = self.requests.lock().unwrap();
        requests.iter().map(|r| r.path.clone()).collect()
    }
}

fn serve(stream: TcpStream, answer: Answer, port: u16, seen: &Mutex<Vec<Request>>) {
    let Some(request) = read_request(&stream) else {
        return;
    };
    let prompt = prompt_of(&request.body);
    let elsewhere = request.path == "/elsewhere";
    seen.lock().unwrap().push(request);
    let response = match answer {
        _ if elsewhere => ok(&completion("followed the redirect")),
        Answer::Normal => ok(&completion(&format!("heard: {prompt}"))),
        Answer::Slow(wait) => {
            thread::sleep(wait);
            ok(&completion(&format!("heard: {prompt}")))
        }
        Answer::Huge => ok(&completion(&"x".repeat(1024 * 1024))),
        Answer::Garbage => ok("<html>not json</html>"),
        Answer::Redirect => format!(
            "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{port}/elsewhere\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        ),
        Answer::Error500 => respond("500 Internal Server Error", &completion("broken")),
        Answer::Controls => ok(&completion("a\u{7}b\r\nc\u{1b}[31m")),
    };
    let mut stream = stream;
    let _ = stream.write_all(response.as_bytes());
}

fn read_request(stream: &TcpStream) -> Option<Request> {
    let mut reader = BufReader::new(stream);
    let mut first = String::new();
    reader.read_line(&mut first).ok()?;
    let path = first.split_whitespace().nth(1)?.to_owned();
    let mut length = 0;
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).ok()?;
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse().ok()?;
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).ok()?;
    Some(Request {
        path,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

fn prompt_of(body: &str) -> String {
    let body: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    body["messages"][0]["content"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

fn completion(text: &str) -> String {
    serde_json::json!({
        "id": "chatcmpl-1",
        "object": "chat.completion",
        "choices": [{ "index": 0, "message": { "role": "assistant", "content": text }, "finish_reason": "stop" }],
    })
    .to_string()
}

fn ok(body: &str) -> String {
    respond("200 OK", body)
}

fn respond(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// A server that only counts its connections, as a proxy would get them.
pub fn proxy() -> (String, Arc<Mutex<u32>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let hits = Arc::new(Mutex::new(0));
    let count = Arc::clone(&hits);
    thread::spawn(move || {
        for _ in listener.incoming() {
            *count.lock().unwrap() += 1;
        }
    });
    (format!("http://127.0.0.1:{port}"), hits)
}
