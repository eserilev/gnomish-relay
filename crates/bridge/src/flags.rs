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
    pub perm: Option<PermAnswer>,
    /// The protocol version of the addon (SPEC.md 7.7).
    pub version: Option<u32>,
}

/// `perm=<request>:<option>:<hash>`: the answer to a permission request (SPEC.md 9.3).
#[derive(Debug, PartialEq, Eq)]
pub struct PermAnswer {
    pub request: String,
    /// 0 for `o1`.
    pub option: usize,
    /// The first 8 bytes of SHA-256 of the popup text that the game showed, in hex.
    pub hash: String,
}

fn perm_answer(value: &str) -> Option<PermAnswer> {
    let mut parts = value.split(':');
    let (request, option, hash) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || !protocol::record::is_valid_id(request.as_bytes()) {
        return None;
    }
    let option = option
        .strip_prefix('o')?
        .parse::<usize>()
        .ok()?
        .checked_sub(1)?;
    let hex = hash.len() == 16
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    hex.then(|| PermAnswer {
        request: request.to_owned(),
        option,
        hash: hash.to_owned(),
    })
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
            Some(("perm", value)) => flags.perm = perm_answer(value),
            Some(("ver", n)) => flags.version = n.parse().ok(),
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
            b"agent=claude;level=auto-edit;n;next=42;read=7,9;restored;h;stop;build=70009;out=shot;in=missing;ver=1",
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
                perm: None,
                version: Some(1),
            }
        );
    }

    #[test]
    fn a_permission_answer_parses_and_a_broken_one_is_ignored() {
        let f = parse(b"perm=p5f3a1:o2:0123456789abcdef");
        assert_eq!(
            f.perm,
            Some(PermAnswer {
                request: "p5f3a1".into(),
                option: 1,
                hash: "0123456789abcdef".into(),
            })
        );
        for bad in [
            "perm=p1:o0:0123456789abcdef",
            "perm=p1:o1:0123456789ABCDEF",
            "perm=p1:o1:0123",
            "perm=../x:o1:0123456789abcdef",
            "perm=p1:o1:0123456789abcdef:extra",
        ] {
            assert_eq!(parse(bad.as_bytes()).perm, None, "{bad}");
        }
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
