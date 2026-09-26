//! The flags field of a record (SPEC.md 7.1.1). Unknown flags are ignored.
//!
//! The flags split in two (SPEC.md 9.7, decision 6). Every app sends the transport flags.
//! Only the relay reads the coding flags, so a coding flag from another app does nothing.

use crate::config::Permission;

/// The hello and the report of the addon on its transport.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct TransportFlags {
    pub hello: bool,
    pub restored: bool,
    pub next: Option<usize>,
    pub read: Vec<u32>,
    /// The protocol version of the addon (SPEC.md 7.7).
    pub version: Option<u32>,
    /// The client build from `GetBuildInfo`, digits only.
    pub build: Option<String>,
    pub out: Option<Channel>,
    pub inbound: Option<Channel>,
}

/// What a message asks of the coding agents.
#[derive(Debug, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // each one is a flag on the wire
pub struct CodingFlags {
    pub new_session: bool,
    pub stop: bool,
    pub delete: bool,
    /// The game asks for the sessions that it can resume.
    pub list: bool,
    /// The session of an agent that a new chat continues.
    pub attach: Option<String>,
    pub agent: Option<String>,
    pub level: Option<Permission>,
    pub perm: Option<PermAnswer>,
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

/// The ids of agents are UUIDs. The limit keeps `;` and `=` out of a flag.
pub fn is_session_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn each_flag(bytes: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(bytes)
        .split(';')
        .map(str::to_owned)
        .collect()
}

pub fn transport(bytes: &[u8]) -> TransportFlags {
    let mut flags = TransportFlags::default();
    for flag in each_flag(bytes) {
        match flag.split_once('=') {
            None if flag == "h" => flags.hello = true,
            None if flag == "restored" => flags.restored = true,
            Some(("next", n)) => flags.next = n.parse().ok(),
            Some(("read", ids)) => {
                flags.read = ids.split(',').filter_map(|id| id.parse().ok()).collect();
            }
            Some(("ver", n)) => flags.version = n.parse().ok(),
            Some(("build", word)) if is_build(word) => flags.build = Some(word.to_owned()),
            Some(("out", word)) => flags.out = channel(word, "shot", "fail"),
            Some(("in", word)) => flags.inbound = channel(word, "slots", "missing"),
            _ => {}
        }
    }
    flags
}

pub fn coding(bytes: &[u8]) -> CodingFlags {
    let mut flags = CodingFlags::default();
    for flag in each_flag(bytes) {
        match flag.split_once('=') {
            None => match flag.as_str() {
                "n" => flags.new_session = true,
                "stop" => flags.stop = true,
                "d" => flags.delete = true,
                "list" => flags.list = true,
                _ => {}
            },
            Some(("level", word)) => flags.level = Some(Permission::from_game(word)),
            Some(("perm", value)) => flags.perm = perm_answer(value),
            Some(("attach", id)) if is_session_id(id) => flags.attach = Some(id.to_owned()),
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

    const EVERY_FLAG: &[u8] = b"agent=claude;level=auto-edit;n;next=42;read=7,9;restored;h;stop;build=70009;out=shot;in=missing;ver=1;d;list;attach=3f2a-9c_1";

    #[test]
    fn every_transport_flag_parses() {
        assert_eq!(
            transport(EVERY_FLAG),
            TransportFlags {
                hello: true,
                restored: true,
                next: Some(42),
                read: vec![7, 9],
                version: Some(1),
                build: Some("70009".into()),
                out: Some(Channel::Works),
                inbound: Some(Channel::Fails),
            }
        );
    }

    #[test]
    fn every_coding_flag_parses() {
        assert_eq!(
            coding(EVERY_FLAG),
            CodingFlags {
                new_session: true,
                stop: true,
                delete: true,
                list: true,
                attach: Some("3f2a-9c_1".into()),
                agent: Some("claude".into()),
                level: Some(Permission::AutoEdit),
                perm: None,
            }
        );
    }

    #[test]
    fn the_transport_parser_ignores_every_coding_flag() {
        let only_coding =
            b"perm=p5f3a1:o2:0123456789abcdef;level=full-auto;agent=claude;attach=s1;list;d;n;stop";
        assert_eq!(transport(only_coding), TransportFlags::default());
    }

    #[test]
    fn the_coding_parser_ignores_every_transport_flag() {
        let only_transport = b"h;next=3;read=1;ver=1;build=7;out=shot;in=slots;restored";
        assert_eq!(coding(only_transport), CodingFlags::default());
    }

    #[test]
    fn a_permission_answer_parses_and_a_broken_one_is_ignored() {
        let f = coding(b"perm=p5f3a1:o2:0123456789abcdef");
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
            assert_eq!(coding(bad.as_bytes()).perm, None, "{bad}");
        }
    }

    #[test]
    fn unknown_and_broken_flags_are_ignored() {
        let bytes = b"x=1;next=many;read=1,a,3;agent=../../bin;build=7|cff;out=maybe;;";
        let t = transport(bytes);
        assert_eq!(t.next, None);
        assert_eq!(t.read, [1, 3]);
        assert_eq!(t.build, None);
        assert_eq!(t.out, None);
        assert_eq!(coding(bytes).agent, None);
    }

    #[test]
    fn an_attach_with_a_bad_session_id_is_ignored() {
        assert_eq!(coding(b"attach=../x").attach, None);
        assert_eq!(coding(b"attach=").attach, None);
        assert_eq!(
            coding(&[b"attach=".as_slice(), &[b'a'; 65]].concat()).attach,
            None
        );
        assert_eq!(coding(b"attach=a b").attach, None);
    }
}
