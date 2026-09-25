//! The flags field of a record (SPEC.md 7.1.1). Unknown flags are ignored.

use crate::config::Permission;

#[derive(Debug, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // each one is a flag on the wire
pub struct Flags {
    pub hello: bool,
    pub new_session: bool,
    pub restored: bool,
    pub stop: bool,
    pub next: Option<usize>,
    pub read: Vec<u32>,
    pub agent: Option<String>,
    pub level: Option<Permission>,
    /// The client build from `GetBuildInfo`, digits only.
    pub build: Option<String>,
    pub out: Option<Channel>,
    pub inbound: Option<Channel>,
}

/// The last result of a channel in the self-test of the addon (SPEC.md 7.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Works,
    Fails,
}

fn channel(word: &str, works: &str, fails: &str) -> Option<Channel> {
    match word {
        w if w == works => Some(Channel::Works),
        w if w == fails => Some(Channel::Fails),
        _ => None,
    }
}

fn is_build(word: &str) -> bool {
    !word.is_empty() && word.len() <= 12 && word.bytes().all(|b| b.is_ascii_digit())
}

pub fn parse(bytes: &[u8]) -> Flags {
    let mut flags = Flags::default();
    for flag in String::from_utf8_lossy(bytes).split(';') {
        match flag.split_once('=') {
            None => match flag {
                "h" => flags.hello = true,
                "n" => flags.new_session = true,
                "restored" => flags.restored = true,
                "stop" => flags.stop = true,
                _ => {}
            },
            Some(("next", n)) => flags.next = n.parse().ok(),
            Some(("read", ids)) => {
                flags.read = ids.split(',').filter_map(|id| id.parse().ok()).collect();
            }
            Some(("level", word)) => flags.level = Some(Permission::from_game(word)),
            Some(("build", word)) if is_build(word) => flags.build = Some(word.to_owned()),
            Some(("out", word)) => flags.out = channel(word, "shot", "fail"),
            Some(("in", word)) => flags.inbound = channel(word, "slots", "missing"),
            Some(("agent", name)) if protocol::record::is_valid_id(name.as_bytes()) => {
                flags.agent = Some(name.to_owned());
            }
            _ => {}
        }
    }
    flags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_known_flag_parses() {
        let f = parse(
            b"agent=claude;level=auto-edit;n;next=42;read=7,9;restored;h;stop;build=70009;out=shot;in=missing",
        );
        assert_eq!(
            f,
            Flags {
                hello: true,
                new_session: true,
                restored: true,
                stop: true,
                next: Some(42),
                read: vec![7, 9],
                agent: Some("claude".into()),
                level: Some(Permission::AutoEdit),
                build: Some("70009".into()),
                out: Some(Channel::Works),
                inbound: Some(Channel::Fails),
            }
        );
    }

    #[test]
    fn unknown_and_broken_flags_are_ignored() {
        let f = parse(b"x=1;next=many;read=1,a,3;agent=../../bin;build=7|cff;out=maybe;;");
        assert_eq!(f.next, None);
        assert_eq!(f.read, [1, 3]);
        assert_eq!(f.agent, None);
        assert_eq!(f.build, None);
        assert_eq!(f.out, None);
    }
}
