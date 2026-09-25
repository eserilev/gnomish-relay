//! S16, S17, S27, and S28 on the compiled code: the classifier and the splitter never
//! panic, no rule list gets more than the ceiling, a file call that runs stays inside
//! its folders, and the command floor holds.
//! The input is `raw NUL cwd NUL rule NUL rule ...`. The words of a rule split at spaces.
#![no_main]

use libfuzzer_sys::fuzz_target;
use protocol::action::{Policy, ToolCall, Verdict, ceiling, classify, rank};
use protocol::shell::split;

const DESKTOP: [&[u8]; 5] = [b"eval", b"sudo", b"cmd", b"powershell", b"pwsh"];
const CAPPED: [&[u8]; 8] = [
    b"curl", b"wget", b"ssh", b"xargs", b"env", b"bash", b"python3", b"chmod",
];

fn policy() -> Policy {
    Policy {
        roots: vec![b"/home/x/Code".to_vec()],
        chat: b"/home/x/Code/app".to_vec(),
        deny_folders: vec![b"/home/x/.config/gnomish-relay".to_vec()],
        desktop_paths: vec![b".ssh".to_vec(), b".env*".to_vec()],
        desktop_writes: vec![b".git/hooks".to_vec()],
        allow: vec![vec![b"cargo".to_vec(), b"test".to_vec()]],
    }
}

/// The name of a program: no folder, lower case, and no last `.exe`.
fn name(word: &[u8]) -> Vec<u8> {
    let start = word
        .iter()
        .rposition(|&b| b == b'/' || b == b'\\')
        .map_or(0, |i| i + 1);
    let lower = word[start..].to_ascii_lowercase();
    match lower.strip_suffix(b".exe") {
        Some(stem) => stem.to_vec(),
        None => lower,
    }
}

fn parts(path: &[u8]) -> Vec<&[u8]> {
    path.split(|&b| b == b'/').filter(|p| !p.is_empty()).collect()
}

fn inside(root: &[u8], path: &[u8]) -> bool {
    let clean = parts(path).iter().all(|&p| p != b"." && p != b"..");
    clean && parts(path).starts_with(&parts(root))
}

fn check_command(raw: &[u8], cwd: &[u8], rules: &[Vec<Vec<u8>>]) {
    let policy = policy();
    let call = ToolCall::Command {
        raw: raw.to_vec(),
        cwd: cwd.to_vec(),
    };
    let got = rank(classify(&call, &policy, rules));
    assert!(got <= rank(ceiling(&call, &policy)));
    let substitution = raw.windows(2).any(|w| w == b"$(") || raw.contains(&b'`');
    if substitution {
        assert_eq!(got, rank(Verdict::Desktop));
    }
    let Some(script) = split(raw) else {
        assert_eq!(got, rank(Verdict::Desktop));
        return;
    };
    for simple in &script.simples {
        for word in &simple.words {
            let n = name(word);
            if DESKTOP.contains(&n.as_slice()) {
                assert!(got <= rank(Verdict::Desktop));
            }
            if CAPPED.contains(&n.as_slice()) {
                assert!(got <= rank(Verdict::Ask));
            }
        }
    }
}

fn check_files(read: &[u8], write: &[u8], rules: &[Vec<Vec<u8>>]) {
    let policy = policy();
    let call = ToolCall::Files {
        reads: vec![read.to_vec()],
        writes: vec![write.to_vec()],
    };
    let got = rank(classify(&call, &policy, rules));
    assert_eq!(got, rank(ceiling(&call, &policy)));
    if got >= rank(Verdict::Ask) {
        assert!(inside(&policy.roots[0], read));
        assert!(inside(&policy.chat, write));
    }
}

fuzz_target!(|data: &[u8]| {
    let mut fields = data.split(|&b| b == 0);
    let raw = fields.next().unwrap_or_default();
    let cwd = fields.next().unwrap_or_default();
    let rules: Vec<Vec<Vec<u8>>> = fields
        .map(|rule| rule.split(|&b| b == b' ').map(<[u8]>::to_vec).collect())
        .collect();
    check_command(raw, cwd, &rules);
    check_files(raw, cwd, &rules);
    let unknown = classify(&ToolCall::Unknown, &policy(), &rules);
    assert_eq!(rank(unknown), rank(Verdict::Desktop));
});
