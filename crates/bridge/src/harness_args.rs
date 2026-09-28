//! The argument template of a `command` agent (SPEC.md 9.2): the message goes in as one
//! argument, as a file, or on stdin. Never through a shell.

use std::path::Path;

pub const PROMPT: &str = "{prompt}";
pub const PROMPT_FILE: &str = "{prompt_file}";
const PLACEHOLDERS: [&str; 2] = [PROMPT, PROMPT_FILE];

/// How the message reaches the harness.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    /// `{prompt}` in an argument.
    Argument,
    /// `{prompt_file}` in an argument: a file in the temp folder of the run.
    File,
    /// No placeholder: the message goes to stdin, and then stdin closes.
    Stdin,
}

impl Input {
    /// For `check-agent`.
    pub fn describe(self) -> &'static str {
        match self {
            Input::Argument => "the message is one argument",
            Input::File => "the message is a file",
            Input::Stdin => "the message goes to stdin",
        }
    }
}

pub fn input_of(template: &[String]) -> Input {
    if template.iter().any(|a| a.contains(PROMPT)) {
        return Input::Argument;
    }
    if template.iter().any(|a| a.contains(PROMPT_FILE)) {
        return Input::File;
    }
    Input::Stdin
}

/// A word such as `{promt}` in the template, which is a typo of a placeholder. The
/// program itself holds no placeholder.
pub fn check_template(template: &[String]) -> Result<(), String> {
    let Some((program, args)) = template.split_first() else {
        return Err("needs a command".into());
    };
    if program.is_empty() {
        return Err("needs a command".into());
    }
    if PLACEHOLDERS.iter().any(|p| program.contains(p)) {
        return Err(format!("the program {program:?} holds a placeholder"));
    }
    match args.iter().find_map(|a| unknown_placeholder(a)) {
        Some(word) => Err(format!(
            "has no placeholder {word}. The placeholders are {PROMPT} and {PROMPT_FILE}"
        )),
        None => Ok(()),
    }
}

/// The first `{word}` of lower-case letters and `_` that is not a placeholder.
fn unknown_placeholder(arg: &str) -> Option<&str> {
    let mut rest = arg;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let name_len = after
            .bytes()
            .take_while(|b| b.is_ascii_lowercase() || *b == b'_')
            .count();
        if name_len > 0 && after.as_bytes().get(name_len) == Some(&b'}') {
            let word = &rest[start..start + name_len + 2];
            if !PLACEHOLDERS.contains(&word) {
                return Some(word);
            }
        }
        rest = after;
    }
    None
}

/// The message as an argument can hold: a NUL byte cannot pass, and a message that is a
/// whole argument never starts with `-`, so the harness never reads it as a flag.
fn prompt_argument(prompt: &str, whole: bool) -> String {
    let text = prompt.replace('\0', " ");
    if whole && text.starts_with('-') {
        return format!(" {text}");
    }
    text
}

/// One argument with each placeholder filled in. The filled text is never read again,
/// so a message that holds `{prompt_file}` stays as it is.
fn fill(arg: &str, prompt: &str, prompt_file: &str) -> String {
    let whole = arg == PROMPT;
    let mut out = String::new();
    let mut rest = arg;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix(PROMPT_FILE) {
            out.push_str(prompt_file);
            rest = after;
        } else if let Some(after) = rest.strip_prefix(PROMPT) {
            out.push_str(&prompt_argument(prompt, whole));
            rest = after;
        } else {
            let ch = rest.chars().next().unwrap_or_default();
            out.push(ch);
            rest = &rest[ch.len_utf8()..];
        }
    }
    out
}

/// The arguments of one run, after the program. `extra`, such as the arguments of
/// `resume`, goes at the end, or before a `--`, because the words after `--` are not flags.
pub fn expand(
    template: &[String],
    extra: &[String],
    prompt: &str,
    prompt_file: &Path,
) -> Vec<String> {
    let file = prompt_file.to_string_lossy();
    let mut args: Vec<String> = template
        .iter()
        .skip(1)
        .map(|a| fill(a, prompt, &file))
        .collect();
    let at = template
        .iter()
        .skip(1)
        .position(|a| a == "--")
        .unwrap_or(args.len());
    args.splice(at..at, extra.iter().cloned());
    args
}

/// The bytes on stdin: the message and a newline, only when no argument holds it.
pub fn stdin_bytes(input: Input, prompt: &str) -> Vec<u8> {
    match input {
        Input::Stdin => format!("{prompt}\n").into_bytes(),
        Input::Argument | Input::File => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|w| (*w).to_owned()).collect()
    }

    fn run(template: &[&str], prompt: &str) -> Vec<String> {
        expand(&words(template), &[], prompt, Path::new("/t/prompt.txt"))
    }

    #[test]
    fn the_message_is_one_argument_even_with_spaces_and_quotes() {
        let args = run(&["tool", "-p", "{prompt}"], "fix it; rm -rf / \"$(x)\"");
        assert_eq!(args, ["-p", "fix it; rm -rf / \"$(x)\""]);
    }

    #[test]
    fn a_message_that_starts_with_a_dash_gets_a_space_in_front() {
        assert_eq!(run(&["tool", "{prompt}"], "--yes"), [" --yes"]);
        assert_eq!(
            run(&["tool", "--message={prompt}"], "--yes"),
            ["--message=--yes"]
        );
    }

    #[test]
    fn a_nul_byte_in_the_message_becomes_a_space() {
        assert_eq!(run(&["tool", "{prompt}"], "a\0b"), ["a b"]);
    }

    #[test]
    fn a_placeholder_inside_the_message_stays_as_it_is() {
        let args = run(
            &["tool", "{prompt}", "{prompt_file}"],
            "{prompt_file} {prompt}",
        );
        assert_eq!(args, ["{prompt_file} {prompt}", "/t/prompt.txt"]);
    }

    #[test]
    fn the_prompt_file_fills_its_placeholder() {
        let args = run(&["aider", "--message-file", "{prompt_file}"], "hi");
        assert_eq!(args, ["--message-file", "/t/prompt.txt"]);
    }

    #[test]
    fn the_input_follows_the_placeholders() {
        assert_eq!(input_of(&words(&["t", "a{prompt}"])), Input::Argument);
        assert_eq!(input_of(&words(&["t", "{prompt_file}"])), Input::File);
        assert_eq!(input_of(&words(&["t", "-q"])), Input::Stdin);
        assert_eq!(stdin_bytes(Input::Stdin, "hi"), b"hi\n");
        assert!(stdin_bytes(Input::File, "hi").is_empty());
    }

    #[test]
    fn extra_arguments_go_at_the_end_or_before_a_double_dash() {
        let extra = words(&["--continue"]);
        let file = Path::new("/f");
        let end = expand(&words(&["t", "run", "{prompt}"]), &extra, "hi", file);
        assert_eq!(end, ["run", "hi", "--continue"]);
        let dash = expand(&words(&["t", "--", "{prompt}"]), &extra, "hi", file);
        assert_eq!(dash, ["--continue", "--", "hi"]);
    }

    #[test]
    fn a_typo_of_a_placeholder_is_an_error() {
        let error = check_template(&words(&["t", "--m={promt}"])).unwrap_err();
        assert!(error.contains("{promt}"), "{error}");
        assert!(check_template(&words(&["t", "{\"a\":1}", "{}", "{prompt}"])).is_ok());
    }

    #[test]
    fn the_program_needs_a_name_and_no_placeholder() {
        assert!(check_template(&[]).is_err());
        assert!(check_template(&words(&[""])).is_err());
        assert!(check_template(&words(&["{prompt}"])).is_err());
    }

    #[test]
    fn each_input_has_a_description() {
        for input in [Input::Argument, Input::File, Input::Stdin] {
            assert!(input.describe().starts_with("the message"));
        }
    }
}
