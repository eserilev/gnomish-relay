//! Records inside a frame payload. See `SPEC.md` 7.1.
//!
//! Records are divided by RS (0x1E). The fields of a record are divided by US (0x1F):
//! `token US chat US id US cwd US flags US name US text`. The text is last, so it can
//! hold US, but no field can hold RS.

pub const RS: u8 = 0x1E;
pub const US: u8 = 0x1F;
pub const MAX_ID_LEN: usize = 32;
pub const MAX_RECORDS: usize = 16;

pub struct Record {
    pub token: Vec<u8>,
    pub chat: Vec<u8>,
    pub id: u32,
    pub cwd: Vec<u8>,
    pub flags: Vec<u8>,
    pub name: Vec<u8>,
    pub text: Vec<u8>,
}

pub enum RecordError {
    Empty,
    TooMany,
    MissingField,
    BadToken,
    BadChat,
    BadId,
}

fn is_id_byte(b: u8) -> bool {
    (b'a' <= b && b <= b'z') || (b'0' <= b && b <= b'9') || b == b'_' || b == b'-'
}

/// 1 to 32 bytes of `a-z`, `0-9`, `_`, `-`. Safe in a file name and a state key.
#[must_use]
pub fn is_valid_id(bytes: &[u8]) -> bool {
    if bytes.is_empty() || bytes.len() > MAX_ID_LEN {
        return false;
    }
    let mut i = 0;
    while i < bytes.len() {
        if !is_id_byte(bytes[i]) {
            return false;
        }
        i += 1;
    }
    true
}

/// The id is plain decimal: no sign, no leading zero, at most `u32::MAX`.
pub fn parse_records(payload: &[u8]) -> Result<Vec<Record>, RecordError> {
    todo!()
}

#[must_use]
pub fn serialize_records(records: &[Record]) -> Vec<u8> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_ids_are_valid() {
        assert!(is_valid_id(b"chat_1"));
        assert!(is_valid_id(b"a-b-c"));
    }

    #[test]
    fn an_empty_id_is_invalid() {
        assert!(!is_valid_id(b""));
    }

    #[test]
    fn an_id_of_33_bytes_is_invalid() {
        assert!(is_valid_id(&[b'a'; 32]));
        assert!(!is_valid_id(&[b'a'; 33]));
    }

    #[test]
    fn path_characters_are_invalid() {
        assert!(!is_valid_id(b"../x"));
        assert!(!is_valid_id(b"a/b"));
        assert!(!is_valid_id(b"a.b"));
    }

    #[test]
    fn upper_case_is_invalid() {
        assert!(!is_valid_id(b"Chat"));
    }
}
