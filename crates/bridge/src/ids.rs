//! Hex text and random IDs.

use std::fmt::Write;

use anyhow::Result;

/// Lowercase, two digits for each byte.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, b| {
        let _ = write!(text, "{b:02x}");
        text
    })
}

pub fn random_bytes(len: usize) -> Result<Vec<u8>> {
    let mut bytes = vec![0u8; len];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("no random bytes from the OS: {e}"))?;
    Ok(bytes)
}

/// `len` random bytes, so the text has `2 * len` digits.
pub fn random_hex(len: usize) -> Result<String> {
    Ok(hex(&random_bytes(len)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_writes_two_lowercase_digits_for_each_byte() {
        assert_eq!(hex(&[0x00, 0x0f, 0xab, 0xff]), "000fabff");
        assert_eq!(hex(&[]), "");
    }

    #[test]
    fn random_hex_has_two_digits_for_each_byte_and_never_repeats() {
        let (a, b) = (random_hex(16).unwrap(), random_hex(16).unwrap());
        assert_eq!(a.len(), 32);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }
}
