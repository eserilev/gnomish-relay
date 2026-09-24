//! Records inside a frame payload. See `SPEC.md` 7.1.
//!
//! Records are divided by RS (0x1E). The fields of a record are divided by US (0x1F):
//! `token US chat US id US cwd US flags US name US text`. The text is last, so it can
//! hold US, but no field can hold RS.

use crate::ascii::{push_bytes, push_decimal, push_range};

pub const RS: u8 = 0x1E;
pub const US: u8 = 0x1F;
pub const MAX_ID_LEN: usize = 32;
pub const MAX_RECORDS: usize = 16;

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
pub struct Record {
    pub token: Vec<u8>,
    pub chat: Vec<u8>,
    pub id: u32,
    pub cwd: Vec<u8>,
    pub flags: Vec<u8>,
    pub name: Vec<u8>,
    pub text: Vec<u8>,
}

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
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

/// The first `target` in `bytes[from..end]`, or `end` if there is none.
fn find_byte(bytes: &[u8], from: usize, end: usize, target: u8) -> usize {
    let mut i = from;
    while i < end {
        if bytes[i] == target {
            return i;
        }
        i += 1;
    }
    end
}

fn copy_field(bytes: &[u8], start: usize, end: usize) -> Vec<u8> {
    let mut field = Vec::new();
    push_range(&mut field, bytes, start, end);
    field
}

/// The value of the digits in `bytes[start..end]`, or `None` for a non-digit.
fn digits_value(bytes: &[u8], start: usize, end: usize) -> Option<u64> {
    let mut value: u64 = 0;
    let mut i = start;
    while i < end {
        let b = bytes[i];
        if b < b'0' || b > b'9' {
            return None;
        }
        value = value * 10 + (b - b'0') as u64;
        i += 1;
    }
    Some(value)
}

/// Plain decimal: no sign, no leading zero, at most `u32::MAX`.
#[allow(clippy::cast_possible_truncation)] // checked against u32::MAX first
fn parse_decimal(bytes: &[u8], start: usize, end: usize) -> Option<u32> {
    let n = end - start;
    if n == 0 || n > 10 {
        return None;
    }
    if n > 1 && bytes[start] == b'0' {
        return None;
    }
    let Some(value) = digits_value(bytes, start, end) else {
        return None;
    };
    if value > u32::MAX as u64 {
        return None;
    }
    Some(value as u32)
}

/// One record in `bytes[start..end]`. The text is everything after the sixth US.
fn parse_record(bytes: &[u8], start: usize, end: usize) -> Result<Record, RecordError> {
    let u1 = find_byte(bytes, start, end, US);
    if u1 == end {
        return Err(RecordError::MissingField);
    }
    let u2 = find_byte(bytes, u1 + 1, end, US);
    if u2 == end {
        return Err(RecordError::MissingField);
    }
    let u3 = find_byte(bytes, u2 + 1, end, US);
    if u3 == end {
        return Err(RecordError::MissingField);
    }
    let u4 = find_byte(bytes, u3 + 1, end, US);
    if u4 == end {
        return Err(RecordError::MissingField);
    }
    let u5 = find_byte(bytes, u4 + 1, end, US);
    if u5 == end {
        return Err(RecordError::MissingField);
    }
    let u6 = find_byte(bytes, u5 + 1, end, US);
    if u6 == end {
        return Err(RecordError::MissingField);
    }
    let token = copy_field(bytes, start, u1);
    if !is_valid_id(&token) {
        return Err(RecordError::BadToken);
    }
    let chat = copy_field(bytes, u1 + 1, u2);
    if !is_valid_id(&chat) {
        return Err(RecordError::BadChat);
    }
    let Some(id) = parse_decimal(bytes, u2 + 1, u3) else {
        return Err(RecordError::BadId);
    };
    Ok(Record {
        token,
        chat,
        id,
        cwd: copy_field(bytes, u3 + 1, u4),
        flags: copy_field(bytes, u4 + 1, u5),
        name: copy_field(bytes, u5 + 1, u6),
        text: copy_field(bytes, u6 + 1, end),
    })
}

/// # Errors
///
/// A `RecordError` if the payload is empty, has too many records, or has a record
/// with a missing field or a bad token, chat, or id.
pub fn parse_records(payload: &[u8]) -> Result<Vec<Record>, RecordError> {
    if payload.is_empty() {
        return Err(RecordError::Empty);
    }
    let mut records = Vec::new();
    let mut start = 0;
    let mut more = true;
    // One record per pass. The last record ends at the end of the payload.
    // `end + 1` only runs when `end` is before the end, so it cannot overflow.
    while more {
        let end = find_byte(payload, start, payload.len(), RS);
        if records.len() == MAX_RECORDS {
            return Err(RecordError::TooMany);
        }
        match parse_record(payload, start, end) {
            Ok(r) => records.push(r),
            Err(e) => return Err(e),
        }
        if end == payload.len() {
            more = false;
        } else {
            start = end + 1;
        }
    }
    Ok(records)
}

fn push_record(out: &mut Vec<u8>, r: &Record) {
    push_bytes(out, &r.token);
    out.push(US);
    push_bytes(out, &r.chat);
    out.push(US);
    push_decimal(out, r.id);
    out.push(US);
    push_bytes(out, &r.cwd);
    out.push(US);
    push_bytes(out, &r.flags);
    out.push(US);
    push_bytes(out, &r.name);
    out.push(US);
    push_bytes(out, &r.text);
}

/// An RS goes between records, not before the first one.
fn push_separator(out: &mut Vec<u8>, i: usize) {
    if i > 0 {
        out.push(RS);
    }
}

#[must_use]
pub fn serialize_records(records: &[Record]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < records.len() {
        push_separator(&mut out, i);
        push_record(&mut out, &records[i]);
        i += 1;
    }
    out
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

    fn record(token: &[u8], id: u32, text: &[u8]) -> Record {
        Record {
            token: token.to_vec(),
            chat: b"c1".to_vec(),
            id,
            cwd: b"/home/x".to_vec(),
            flags: b"n".to_vec(),
            name: b"my chat".to_vec(),
            text: text.to_vec(),
        }
    }

    #[test]
    fn records_parse_back_to_themselves() {
        let records = vec![
            record(b"tok", 7, b"hello"),
            record(b"tok", 4_294_967_295, b""),
        ];
        assert_eq!(parse_records(&serialize_records(&records)), Ok(records));
    }

    #[test]
    fn the_text_can_hold_a_field_separator() {
        let records = vec![record(b"tok", 1, b"a\x1Fb")];
        assert_eq!(parse_records(&serialize_records(&records)), Ok(records));
    }

    #[test]
    fn a_missing_field_is_rejected() {
        assert_eq!(
            parse_records(b"tok\x1Fc1\x1F7"),
            Err(RecordError::MissingField)
        );
    }

    #[test]
    fn a_bad_token_is_rejected() {
        assert_eq!(
            parse_records(b"../x\x1Fc1\x1F7\x1F\x1F\x1F\x1Ftext"),
            Err(RecordError::BadToken)
        );
    }

    #[test]
    fn a_leading_zero_in_the_id_is_rejected() {
        assert_eq!(
            parse_records(b"t\x1Fc\x1F07\x1F\x1F\x1F\x1F"),
            Err(RecordError::BadId)
        );
    }

    #[test]
    fn an_id_above_u32_is_rejected() {
        assert_eq!(
            parse_records(b"t\x1Fc\x1F4294967296\x1F\x1F\x1F\x1F"),
            Err(RecordError::BadId)
        );
    }

    #[test]
    fn an_empty_payload_is_rejected() {
        assert_eq!(parse_records(b""), Err(RecordError::Empty));
    }

    #[test]
    fn seventeen_records_are_too_many() {
        let records: Vec<Record> = (0..17).map(|i| record(b"t", i, b"")).collect();
        assert_eq!(
            parse_records(&serialize_records(&records)),
            Err(RecordError::TooMany)
        );
        assert!(parse_records(&serialize_records(&records[..16])).is_ok());
    }
}
