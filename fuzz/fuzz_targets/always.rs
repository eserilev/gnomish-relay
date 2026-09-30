//! S36 to S39 on the compiled code: `propose` and `offer` never panic, each proposal is
//! the first words of its command and holds only plain words, and each offer makes the
//! classifier give `allow` under a ceiling of `allow`.
//! The input is `raw NUL rule NUL rule ...`. The words of a rule split at spaces.
#![no_main]

use libfuzzer_sys::fuzz_target;
use protocol::action::{Policy, ToolCall, Verdict, ceiling, classify};
use protocol::always::{MAX_OFFER, MAX_WORD, offer, propose};
use protocol::shell::split;

const SPECIAL: &[u8] = b"*?[]$`'\"\\;&|<>(){}~#=";

fn policy() -> Policy {
    Policy {
        roots: vec![b"/home/x/Code".to_vec()],
        chat: b"/home/x/Code/app".to_vec(),
        deny_folders: vec![b"/home/x/.config/gnomish-relay".to_vec()],
        desktop_paths: vec![b".ssh".to_vec(), b".env*".to_vec()],
        desktop_writes: vec![b".git/hooks".to_vec()],
        allow: vec![vec![b"cargo".to_vec(), b"fmt".to_vec()]],
    }
}

fn is_plain(word: &[u8]) -> bool {
    !word.is_empty()
        && word.len() <= MAX_WORD
        && word[0] != b'-'
        && word[0] != b'+'
        && word
            .iter()
            .all(|&b| (b'!'..=b'~').contains(&b) && !SPECIAL.contains(&b))
}

fn check_proposals(raw: &[u8]) {
    let Some(script) = split(raw) else {
        return;
    };
    for simple in &script.simples {
        let Some(rule) = propose(simple) else {
            continue;
        };
        assert!(rule.len() == 1 || rule.len() == 2);
        assert!(simple.words.starts_with(&rule));
        assert!(rule.iter().all(|w| is_plain(w)));
        assert!(!rule[0].contains(&b'/'));
    }
}

fn check_offer(raw: &[u8], rules: &[Vec<Vec<u8>>]) {
    let policy = policy();
    let call = ToolCall::Command {
        raw: raw.to_vec(),
        cwd: b"/home/x/Code/app".to_vec(),
    };
    let Some(new) = offer(&call, &policy, rules) else {
        return;
    };
    assert!(!new.is_empty() && new.len() <= MAX_OFFER);
    let mut all = rules.to_vec();
    all.extend(new.iter().cloned());
    assert!(classify(&call, &policy, &all) == Verdict::Allow);
    assert!(ceiling(&call, &policy) == Verdict::Allow);
    let Some(script) = split(raw) else {
        panic!("an offer for a command that does not parse");
    };
    for rule in &new {
        assert!(script.simples.iter().any(|s| s.words.starts_with(rule)));
    }
}

fuzz_target!(|data: &[u8]| {
    let mut fields = data.split(|&b| b == 0);
    let raw = fields.next().unwrap_or_default();
    let rules: Vec<Vec<Vec<u8>>> = fields
        .map(|rule| rule.split(|&b| b == b' ').map(<[u8]>::to_vec).collect())
        .collect();
    check_proposals(raw);
    check_offer(raw, &rules);
});
