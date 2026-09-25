//! A shell command, split into simple commands for the action classifier.
//! See `SPEC.md` 6.6.3.
//!
//! The grammar is a strict part of POSIX `sh`. A command outside it does not
//! parse, and the classifier then answers `desktop`. So a missing feature makes
//! the classifier stricter, never looser.

use crate::search::listed;

/// The longest command that parses: 1 MiB.
pub const MAX_COMMAND: usize = 1_048_576;

/// Words that start a compound command. A command such as `if x; then rm -r y; fi`
/// would hide `rm` behind `then`.
const RESERVED: [u8; 87] =
    *b" if then else elif fi for while until do done case esac select function coproc ! [[ ]] ";

#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
pub enum Access {
    Read,
    Write,
}

/// `Pipe` marks every simple command after the first `|`. So a shell inside a
/// group, as in `curl x | (echo; sh)`, still counts as piped.
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
pub enum Link {
    First,
    Pipe,
}

/// The words after quote removal. Assignments such as `A=1` stay in the words.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
pub struct Simple {
    pub words: Vec<Vec<u8>>,
    pub link: Link,
}

/// A redirect to a file. `2>&1` and other copies of a descriptor are not files.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
pub struct Redirect {
    pub target: Vec<u8>,
    pub access: Access,
}

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
pub struct Script {
    pub simples: Vec<Simple>,
    pub redirects: Vec<Redirect>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Plain,
    Single,
    Double,
    /// After `\` inside double quotes.
    DoubleEscape,
    /// After `\` outside quotes.
    Escape,
}

/// What the next word is for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pending {
    Word,
    Read,
    Write,
    /// The descriptor after `>&` or `<&`: digits or `-`.
    Copy,
}

struct Lexer {
    ok: bool,
    mode: Mode,
    simples: Vec<Simple>,
    redirects: Vec<Redirect>,
    words: Vec<Vec<u8>>,
    link: Link,
    word: Vec<u8>,
    /// A quote starts a word, so `''` is an empty word.
    started: bool,
    /// The word holds an unquoted `*`, `?`, or `[`.
    glob: bool,
    pending: Pending,
    depth: usize,
}

fn fail(mut lx: Lexer) -> Lexer {
    lx.ok = false;
    lx
}

fn add_byte(mut lx: Lexer, b: u8) -> Lexer {
    lx.word.push(b);
    lx.started = true;
    lx
}

fn is_digit(b: u8) -> bool {
    b'0' <= b && b <= b'9'
}

fn all_digits(bytes: &[u8]) -> bool {
    let mut digits = true;
    let mut i = 0;
    while digits && i < bytes.len() {
        digits = is_digit(bytes[i]);
        i += 1;
    }
    digits
}

fn is_descriptor(word: &[u8]) -> bool {
    word.len() > 0 && (all_digits(word) || (word.len() == 1 && word[0] == b'-'))
}

/// A glob in the command name can expand to any program, for example `/usr/bin/su?o`.
/// `[` alone is the test command.
fn bad_command_name(lx: &Lexer) -> bool {
    lx.words.len() == 0
        && (listed(&RESERVED, &lx.word) || (lx.glob && !(lx.word.len() == 1 && lx.word[0] == b'[')))
}

fn take_word(mut lx: Lexer) -> (Lexer, Vec<u8>) {
    let word = lx.word;
    lx.word = Vec::new();
    lx.started = false;
    lx.glob = false;
    (lx, word)
}

fn end_word(lx: Lexer) -> Lexer {
    if !lx.started {
        return lx;
    }
    if lx.pending == Pending::Word && bad_command_name(&lx) {
        return fail(lx);
    }
    let pending = lx.pending;
    let (mut lx, word) = take_word(lx);
    lx.pending = Pending::Word;
    if pending == Pending::Word {
        lx.words.push(word);
    } else if pending == Pending::Read {
        lx.redirects.push(Redirect {
            target: word,
            access: Access::Read,
        });
    } else if pending == Pending::Write {
        lx.redirects.push(Redirect {
            target: word,
            access: Access::Write,
        });
    } else if !is_descriptor(&word) {
        lx = fail(lx);
    }
    lx
}

fn end_simple(lx: Lexer, next: Link) -> Lexer {
    let mut lx = end_word(lx);
    if lx.pending != Pending::Word {
        return fail(lx);
    }
    if lx.words.len() > 0 {
        let words = lx.words;
        lx.words = Vec::new();
        lx.simples.push(Simple {
            words,
            link: lx.link,
        });
    }
    if lx.link == Link::First {
        lx.link = next;
    }
    lx
}

/// `2>x`: a word of digits right before `>` or `<` names a descriptor.
fn end_word_before_redirect(lx: Lexer) -> Lexer {
    if lx.started && all_digits(&lx.word) {
        return take_word(lx).0;
    }
    end_word(lx)
}

fn open_redirect(lx: Lexer, pending: Pending) -> Lexer {
    let mut lx = end_word_before_redirect(lx);
    if lx.pending != Pending::Word {
        return fail(lx);
    }
    lx.pending = pending;
    lx
}

fn open_group(mut lx: Lexer) -> Lexer {
    lx.depth += 1;
    end_simple(lx, Link::First)
}

fn close_group(mut lx: Lexer) -> Lexer {
    if lx.depth == 0 {
        return fail(lx);
    }
    lx.depth -= 1;
    end_simple(lx, Link::First)
}

/// The byte at `i`, or 0 past the end. Every caller compares it with a byte that is not 0.
fn peek(raw: &[u8], i: usize) -> u8 {
    if i < raw.len() { raw[i] } else { 0 }
}

/// `&&`, `&>`, `&>>`, or `&` alone, which runs the command in the background.
fn ampersand(lx: Lexer, raw: &[u8], i: usize) -> (Lexer, usize) {
    let next = peek(raw, i + 1);
    if next == b'&' {
        return (end_simple(lx, Link::First), i + 2);
    }
    if next == b'>' {
        let lx = open_redirect(lx, Pending::Write);
        if peek(raw, i + 2) == b'>' {
            return (lx, i + 3);
        }
        return (lx, i + 2);
    }
    (end_simple(lx, Link::First), i + 1)
}

/// `||`, `|&`, or `|`.
fn bar(lx: Lexer, raw: &[u8], i: usize) -> (Lexer, usize) {
    let next = peek(raw, i + 1);
    if next == b'|' {
        return (end_simple(lx, Link::First), i + 2);
    }
    if next == b'&' {
        return (end_simple(lx, Link::Pipe), i + 2);
    }
    (end_simple(lx, Link::Pipe), i + 1)
}

/// `>`, `>>`, `>|`, `>&`. `>(` is process substitution, which runs a command.
fn greater(lx: Lexer, raw: &[u8], i: usize) -> (Lexer, usize) {
    let next = peek(raw, i + 1);
    if next == b'>' || next == b'|' {
        return (open_redirect(lx, Pending::Write), i + 2);
    }
    if next == b'&' {
        return (open_redirect(lx, Pending::Copy), i + 2);
    }
    if next == b'(' {
        return (fail(lx), i + 1);
    }
    (open_redirect(lx, Pending::Write), i + 1)
}

/// `<`, `<&`, `<>`. A heredoc `<<` and `<(` do not parse.
fn less(lx: Lexer, raw: &[u8], i: usize) -> (Lexer, usize) {
    let next = peek(raw, i + 1);
    if next == b'<' || next == b'(' {
        return (fail(lx), i + 1);
    }
    if next == b'&' {
        return (open_redirect(lx, Pending::Copy), i + 2);
    }
    if next == b'>' {
        return (open_redirect(lx, Pending::Write), i + 2);
    }
    (open_redirect(lx, Pending::Read), i + 1)
}

/// `{}` is a plain word, as in `find -exec rm {} ;`. Any other brace is brace
/// expansion or a group, which do not parse.
fn brace(lx: Lexer, raw: &[u8], i: usize) -> (Lexer, usize) {
    if peek(raw, i + 1) == b'}' {
        return (add_byte(add_byte(lx, b'{'), b'}'), i + 2);
    }
    (fail(lx), i + 1)
}

fn glob_byte(lx: Lexer, b: u8) -> Lexer {
    let mut lx = add_byte(lx, b);
    lx.glob = true;
    lx
}

fn set_mode(mut lx: Lexer, mode: Mode) -> Lexer {
    lx.mode = mode;
    lx
}

/// A quote starts a word even if nothing follows. A `\` does not: `\` and a newline
/// add nothing.
fn start_quote(lx: Lexer, mode: Mode) -> Lexer {
    let mut lx = set_mode(lx, mode);
    lx.started = true;
    lx
}

/// `$` starts an expansion and a backtick starts a command, so neither parses.
fn plain_single(lx: Lexer, b: u8) -> Lexer {
    if b == b' ' || b == b'\t' {
        end_word(lx)
    } else if b == b'\n' || b == b';' {
        end_simple(lx, Link::First)
    } else if b == b'(' {
        open_group(lx)
    } else if b == b')' {
        close_group(lx)
    } else if b == b'$' || b == b'`' || b == b'}' || (b == b'#' && !lx.started) {
        fail(lx)
    } else if b == b'*' || b == b'?' || b == b'[' {
        glob_byte(lx, b)
    } else if b == b'\\' {
        set_mode(lx, Mode::Escape)
    } else if b == b'\'' {
        start_quote(lx, Mode::Single)
    } else if b == b'"' {
        start_quote(lx, Mode::Double)
    } else {
        add_byte(lx, b)
    }
}

fn plain(lx: Lexer, raw: &[u8], i: usize) -> (Lexer, usize) {
    let b = raw[i];
    if b == b'&' {
        ampersand(lx, raw, i)
    } else if b == b'|' {
        bar(lx, raw, i)
    } else if b == b'>' {
        greater(lx, raw, i)
    } else if b == b'<' {
        less(lx, raw, i)
    } else if b == b'{' {
        brace(lx, raw, i)
    } else {
        (plain_single(lx, b), i + 1)
    }
}

fn single(lx: Lexer, b: u8) -> Lexer {
    if b == b'\'' {
        return set_mode(lx, Mode::Plain);
    }
    add_byte(lx, b)
}

fn double(lx: Lexer, b: u8) -> Lexer {
    if b == b'"' {
        set_mode(lx, Mode::Plain)
    } else if b == b'\\' {
        set_mode(lx, Mode::DoubleEscape)
    } else if b == b'$' || b == b'`' {
        fail(lx)
    } else {
        add_byte(lx, b)
    }
}

/// Inside double quotes, `\` escapes only `$`, a backtick, `"`, `\`, and a newline.
fn double_escape(lx: Lexer, b: u8) -> Lexer {
    let lx = set_mode(lx, Mode::Double);
    if b == b'\n' {
        lx
    } else if b == b'$' || b == b'`' || b == b'"' || b == b'\\' {
        add_byte(lx, b)
    } else {
        add_byte(add_byte(lx, b'\\'), b)
    }
}

/// `\` and a newline join two lines.
fn escape(lx: Lexer, b: u8) -> Lexer {
    let lx = set_mode(lx, Mode::Plain);
    if b == b'\n' {
        return lx;
    }
    add_byte(lx, b)
}

fn step(lx: Lexer, raw: &[u8], i: usize) -> (Lexer, usize) {
    let b = raw[i];
    if lx.mode == Mode::Plain {
        plain(lx, raw, i)
    } else if lx.mode == Mode::Single {
        (single(lx, b), i + 1)
    } else if lx.mode == Mode::Double {
        (double(lx, b), i + 1)
    } else if lx.mode == Mode::DoubleEscape {
        (double_escape(lx, b), i + 1)
    } else {
        (escape(lx, b), i + 1)
    }
}

fn new_lexer() -> Lexer {
    Lexer {
        ok: true,
        mode: Mode::Plain,
        simples: Vec::new(),
        redirects: Vec::new(),
        words: Vec::new(),
        link: Link::First,
        word: Vec::new(),
        started: false,
        glob: false,
        pending: Pending::Word,
        depth: 0,
    }
}

fn run(raw: &[u8]) -> Lexer {
    let mut lx = new_lexer();
    let mut i = 0;
    while lx.ok && i < raw.len() {
        let (next, j) = step(lx, raw, i);
        lx = next;
        i = j;
    }
    lx
}

/// An open quote, a trailing `\`, an open `(`, or a redirect with no file does not parse.
fn finish(lx: Lexer) -> Option<Script> {
    if !lx.ok || lx.mode != Mode::Plain || lx.depth != 0 {
        return None;
    }
    let lx = end_simple(lx, Link::First);
    if !lx.ok {
        return None;
    }
    Some(Script {
        simples: lx.simples,
        redirects: lx.redirects,
    })
}

/// Returns `None` for a command that does not parse.
#[must_use]
pub fn split(raw: &[u8]) -> Option<Script> {
    if raw.len() > MAX_COMMAND {
        return None;
    }
    finish(run(raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(raw: &str) -> Vec<Vec<String>> {
        let script = split(raw.as_bytes()).expect("parses");
        script
            .simples
            .iter()
            .map(|s| {
                s.words
                    .iter()
                    .map(|w| String::from_utf8_lossy(w).into_owned())
                    .collect()
            })
            .collect()
    }

    fn targets(raw: &str) -> Vec<(String, Access)> {
        let script = split(raw.as_bytes()).expect("parses");
        script
            .redirects
            .iter()
            .map(|r| (String::from_utf8_lossy(&r.target).into_owned(), r.access))
            .collect()
    }

    fn links(raw: &str) -> Vec<Link> {
        let script = split(raw.as_bytes()).expect("parses");
        script.simples.iter().map(|s| s.link).collect()
    }

    fn fails(raw: &str) -> bool {
        split(raw.as_bytes()).is_none()
    }

    #[test]
    fn spaces_and_tabs_split_words() {
        assert_eq!(words("cargo  test\t-q"), [["cargo", "test", "-q"]]);
    }

    #[test]
    fn operators_split_simple_commands() {
        assert_eq!(
            words("a; b && c || d & e\nf"),
            [["a"], ["b"], ["c"], ["d"], ["e"], ["f"]]
        );
    }

    #[test]
    fn a_pipe_marks_every_later_command() {
        assert_eq!(
            links("a; b | c |& d; e"),
            [Link::First, Link::First, Link::Pipe, Link::Pipe, Link::Pipe]
        );
    }

    #[test]
    fn quotes_hide_operators() {
        assert_eq!(
            words("echo 'a; b' \"c && d\""),
            [["echo", "a; b", "c && d"]]
        );
    }

    #[test]
    fn quote_removal_joins_the_parts_of_a_word() {
        assert_eq!(words(r#"e"va"l x"#), [["eval", "x"]]);
        assert_eq!(words(r"e\val x"), [["eval", "x"]]);
    }

    #[test]
    fn empty_quotes_make_an_empty_word() {
        assert_eq!(words("echo ''"), [["echo", ""]]);
    }

    #[test]
    fn a_backslash_inside_double_quotes_escapes_only_five_bytes() {
        assert_eq!(words(r#"echo "a\"b\\c\d""#), [["echo", r#"a"b\c\d"#]]);
    }

    #[test]
    fn a_backslash_newline_joins_lines() {
        assert_eq!(words("cargo \\\ntest"), [["cargo", "test"]]);
        assert_eq!(words("cargo \\\n test"), [["cargo", "test"]]);
    }

    #[test]
    fn redirects_are_not_words() {
        assert_eq!(words("echo hi > out.txt"), [["echo", "hi"]]);
        assert_eq!(
            targets("echo hi > out.txt"),
            [("out.txt".to_owned(), Access::Write)]
        );
    }

    #[test]
    fn every_redirect_form_gives_its_access() {
        assert_eq!(
            targets("a >x >>y >|z &>v &>>w <r <>b 2>e"),
            [
                ("x".to_owned(), Access::Write),
                ("y".to_owned(), Access::Write),
                ("z".to_owned(), Access::Write),
                ("v".to_owned(), Access::Write),
                ("w".to_owned(), Access::Write),
                ("r".to_owned(), Access::Read),
                ("b".to_owned(), Access::Write),
                ("e".to_owned(), Access::Write),
            ]
        );
    }

    #[test]
    fn a_descriptor_copy_is_not_a_file() {
        assert_eq!(targets("cargo test 2>&1"), []);
        assert_eq!(words("cargo test 2>&1"), [["cargo", "test"]]);
        assert_eq!(targets("a <&0 >&-"), []);
    }

    #[test]
    fn a_descriptor_copy_to_a_name_does_not_parse() {
        assert!(fails("a >&file"));
    }

    #[test]
    fn digits_before_a_redirect_name_the_descriptor() {
        assert_eq!(words("echo 12>x"), [["echo"]]);
        assert_eq!(words("echo a2>x"), [["echo", "a2"]]);
    }

    #[test]
    fn a_redirect_with_no_file_does_not_parse() {
        assert!(fails("echo >"));
        assert!(fails("echo > ; ls"));
        assert!(fails("echo > > x"));
    }

    #[test]
    fn subshells_split_and_must_balance() {
        assert_eq!(
            words("(cd a && make) ; ls"),
            [vec!["cd", "a"], vec!["make"], vec!["ls"]]
        );
        assert!(fails("(ls"));
        assert!(fails("ls)"));
    }

    #[test]
    fn expansions_and_substitutions_do_not_parse() {
        assert!(fails("echo $HOME"));
        assert!(fails("cat $(echo /etc/passwd)"));
        assert!(fails("echo `id`"));
        assert!(fails("echo \"$x\""));
        assert!(fails("echo \"`id`\""));
    }

    #[test]
    fn single_quotes_keep_dollar_and_backtick() {
        assert_eq!(words("echo '$x `y`'"), [["echo", "$x `y`"]]);
    }

    #[test]
    fn heredocs_and_process_substitution_do_not_parse() {
        assert!(fails("cat <<EOF\nx\nEOF"));
        assert!(fails("cat <<< x"));
        assert!(fails("diff <(a) b"));
        assert!(fails("tee >(sh)"));
    }

    #[test]
    fn unbalanced_quotes_do_not_parse() {
        assert!(fails("echo 'a"));
        assert!(fails("echo \"a"));
        assert!(fails("echo a\\"));
    }

    #[test]
    fn braces_parse_only_as_the_find_placeholder() {
        assert_eq!(
            words("find . -exec rm {} \\;"),
            [["find", ".", "-exec", "rm", "{}", ";"]]
        );
        assert!(fails("echo {a,b}"));
        assert!(fails("{ ls; }"));
    }

    #[test]
    fn a_comment_does_not_parse() {
        assert!(fails("ls # rm -rf /"));
        assert_eq!(words("echo a#b"), [["echo", "a#b"]]);
    }

    #[test]
    fn reserved_words_do_not_parse_as_a_command_name() {
        assert!(fails("if true; then rm -r x; fi"));
        assert!(fails("! ls"));
        assert_eq!(words("echo if"), [["echo", "if"]]);
    }

    #[test]
    fn a_glob_in_the_command_name_does_not_parse() {
        assert!(fails("/usr/bin/su?o ls"));
        assert!(fails("*"));
        assert_eq!(words("ls *.rs"), [["ls", "*.rs"]]);
        assert_eq!(words("'*' x"), [["*", "x"]]);
    }

    #[test]
    fn the_test_bracket_is_a_command_name() {
        assert_eq!(words("[ -f x ]"), [["[", "-f", "x", "]"]]);
    }

    #[test]
    fn an_empty_command_has_no_simple_commands() {
        assert_eq!(words(""), Vec::<Vec<String>>::new());
        assert_eq!(words(" ;; "), Vec::<Vec<String>>::new());
    }

    #[test]
    fn invalid_utf8_is_plain_bytes() {
        let script = split(b"echo \xff\xfe").expect("parses");
        assert_eq!(script.simples[0].words[1], b"\xff\xfe");
    }

    #[test]
    fn a_command_over_the_limit_does_not_parse() {
        let long = vec![b'a'; MAX_COMMAND + 1];
        assert!(split(&long).is_none());
        let fits = vec![b'a'; MAX_COMMAND];
        assert!(split(&fits).is_some());
    }
}
