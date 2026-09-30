//! The argument template and the output reader of a `command` agent (SPEC.md 9.2). The
//! input is `prompt NUL arg NUL arg ...`, and all of it is also the output of a harness.
//!
//! - The message is never a shell string: each argument of the template gives exactly
//!   one argument, a whole `{prompt}` is the message, and it never starts with `-`.
//! - An argument with no placeholder stays as it is, and the message is never read again.
//! - Clean output holds no escape and no control character but a newline and a tab.
//! - A progress line, a line of `Lines`, and a reply stay inside their limits.
#![no_main]

use std::path::Path;

use bridge::harness_args::{PROMPT, PROMPT_FILE, expand};
use bridge::harness_output::{Lines, clean, progress_line, reply_text};
use libfuzzer_sys::fuzz_target;

fn check_expand(data: &[u8]) {
    let text = String::from_utf8_lossy(data);
    let mut fields = text.split('\0');
    let prompt = fields.next().unwrap_or_default();
    let mut template = vec!["tool".to_owned()];
    template.extend(fields.map(str::to_owned));
    let extra = vec!["--more".to_owned()];

    let args = expand(&template, &extra, prompt, Path::new("/t/p.txt"));

    assert_eq!(args.len(), template.len());
    let at = template[1..]
        .iter()
        .position(|a| a == "--")
        .unwrap_or(template.len() - 1);
    assert_eq!(args[at], "--more");
    let mut filled = args.clone();
    filled.remove(at);
    for (arg, from) in filled.iter().zip(&template[1..]) {
        if from == PROMPT {
            let expected = if prompt.starts_with('-') {
                format!(" {prompt}")
            } else {
                prompt.to_owned()
            };
            assert_eq!(*arg, expected);
        } else if !from.contains(PROMPT) && !from.contains(PROMPT_FILE) {
            assert_eq!(arg, from);
        }
    }
}

fn check_output(data: &[u8]) {
    let text = clean(data);
    assert!(
        text.chars()
            .all(|c| c == '\n' || c == '\t' || !c.is_control()),
        "{text:?}"
    );
    if let Some(line) = progress_line(data, 200) {
        assert!(line.len() <= 200 && !line.contains('\n'));
    }
    let reply = reply_text(data, 4096);
    assert!(reply.len() <= 4096, "{}", reply.len());
    let mut lines = Lines::default();
    for chunk in data.chunks(7) {
        for line in lines.push(chunk, 64) {
            assert!(line.len() <= 64 && !line.contains(&b'\n'));
        }
    }
}

fuzz_target!(|data: &[u8]| {
    check_expand(data);
    check_output(data);
});
