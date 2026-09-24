//! Small writers for ASCII output, shared by the modules that build text.

pub fn push_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    let mut i = 0;
    while i < bytes.len() {
        out.push(bytes[i]);
        i += 1;
    }
}

/// Decimal digits, most significant first, no leading zero.
pub fn push_decimal(out: &mut Vec<u8>, n: u32) {
    if n >= 10 {
        push_decimal(out, n / 10);
    }
    out.push(b'0' + (n % 10) as u8);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decimal(n: u32) -> Vec<u8> {
        let mut out = Vec::new();
        push_decimal(&mut out, n);
        out
    }

    #[test]
    fn zero_is_one_digit() {
        assert_eq!(decimal(0), b"0");
    }

    #[test]
    fn digits_come_most_significant_first() {
        assert_eq!(decimal(1203), b"1203");
    }

    #[test]
    fn the_largest_u32_has_ten_digits() {
        assert_eq!(decimal(u32::MAX), b"4294967295");
    }

    #[test]
    fn push_bytes_appends() {
        let mut out = b"ab".to_vec();
        push_bytes(&mut out, b"cd");
        assert_eq!(out, b"abcd");
    }
}
