//! Bytes to color cells and back.
//!
//! A cell carries 3 bits, one per color channel. Three bytes are 24 bits, which
//! fill exactly eight cells, so we work in groups of three bytes. The last group
//! is padded with zero bytes. The frame header carries the real length.

// Aeneas has no model for `try_from` yet, so we narrow with `as`.
// Every narrowing cast below masks or shifts the value into range first.
#![allow(clippy::cast_possible_truncation)]

pub const BYTES_PER_GROUP: usize = 3;
pub const CELLS_PER_GROUP: usize = 8;

fn group_bits(a: u8, b: u8, c: u8) -> u32 {
    (a as u32) << 16 | (b as u32) << 8 | c as u32
}

fn cell_at(bits: u32, shift: u32) -> u8 {
    (bits >> shift & 7) as u8
}

fn append_cell(bits: u32, cell: u8) -> u32 {
    bits << 3 | cell as u32
}

fn byte_at(bits: u32, shift: u32) -> u8 {
    (bits >> shift) as u8
}

/// Most significant bits first, so the first cell holds the top 3 bits of `a`.
#[must_use]
pub fn encode_group(a: u8, b: u8, c: u8) -> [u8; 8] {
    let bits = group_bits(a, b, c);
    [
        cell_at(bits, 21),
        cell_at(bits, 18),
        cell_at(bits, 15),
        cell_at(bits, 12),
        cell_at(bits, 9),
        cell_at(bits, 6),
        cell_at(bits, 3),
        cell_at(bits, 0),
    ]
}

/// Returns `None` if any cell is out of range. A cell from a real image is
/// always 0 to 7, so a bad cell means a bug or a forged strip.
#[must_use]
pub fn decode_group(cells: [u8; 8]) -> Option<[u8; 3]> {
    let mut bits = 0;
    let mut i = 0;
    while i < CELLS_PER_GROUP {
        if cells[i] > 7 {
            return None;
        }
        bits = append_cell(bits, cells[i]);
        i += 1;
    }
    Some([byte_at(bits, 16), byte_at(bits, 8), byte_at(bits, 0)])
}

fn byte_or_zero(bytes: &[u8], i: usize) -> u8 {
    if i < bytes.len() { bytes[i] } else { 0 }
}

fn push_group(cells: &mut Vec<u8>, group: [u8; 8]) {
    let mut i = 0;
    while i < CELLS_PER_GROUP {
        cells.push(group[i]);
        i += 1;
    }
}

#[must_use]
pub fn encode_cells(bytes: &[u8]) -> Vec<u8> {
    let mut cells = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let group = encode_group(
            bytes[i],
            byte_or_zero(bytes, i + 1),
            byte_or_zero(bytes, i + 2),
        );
        push_group(&mut cells, group);
        i += BYTES_PER_GROUP;
    }
    cells
}

/// Returns `None` if the cell count is not a whole number of groups, or if any
/// cell is out of range. The result keeps the zero padding of the last group.
#[must_use]
pub fn decode_cells(cells: &[u8]) -> Option<Vec<u8>> {
    if !cells.len().is_multiple_of(CELLS_PER_GROUP) {
        return None;
    }
    let mut bytes = Vec::new();
    let mut i = 0;
    while i < cells.len() {
        let group = [
            cells[i],
            cells[i + 1],
            cells[i + 2],
            cells[i + 3],
            cells[i + 4],
            cells[i + 5],
            cells[i + 6],
            cells[i + 7],
        ];
        // Not `?`: Aeneas has no model for it and would add an axiom.
        #[allow(clippy::question_mark)]
        let Some([a, b, c]) = decode_group(group) else {
            return None;
        };
        bytes.push(a);
        bytes.push(b);
        bytes.push(c);
        i += CELLS_PER_GROUP;
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_cell_holds_the_top_bits_of_the_first_byte() {
        assert_eq!(encode_group(0b1010_0000, 0, 0), [5, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn last_cell_holds_the_low_bits_of_the_last_byte() {
        assert_eq!(encode_group(0, 0, 0b0000_0110), [0, 0, 0, 0, 0, 0, 0, 6]);
    }

    #[test]
    fn group_round_trip_for_every_first_byte() {
        for a in 0..=u8::MAX {
            assert_eq!(
                decode_group(encode_group(a, 0x5A, 0xC3)),
                Some([a, 0x5A, 0xC3])
            );
        }
    }

    #[test]
    fn decode_group_rejects_a_cell_above_seven() {
        assert_eq!(decode_group([0, 0, 0, 8, 0, 0, 0, 0]), None);
    }

    #[test]
    fn empty_input_gives_no_cells() {
        assert_eq!(encode_cells(&[]), Vec::<u8>::new());
    }

    #[test]
    fn last_group_is_padded_with_zero_bytes() {
        let cells = encode_cells(&[0xFF]);
        assert_eq!(cells.len(), CELLS_PER_GROUP);
        assert_eq!(decode_cells(&cells), Some(vec![0xFF, 0, 0]));
    }

    #[test]
    fn round_trip_keeps_the_bytes_and_pads_the_tail() {
        let bytes: Vec<u8> = (0..=u8::MAX).collect();
        let decoded = decode_cells(&encode_cells(&bytes)).unwrap();
        assert_eq!(&decoded[..bytes.len()], &bytes[..]);
        assert!(decoded[bytes.len()..].iter().all(|&b| b == 0));
    }

    #[test]
    fn decode_cells_rejects_a_partial_group() {
        assert_eq!(decode_cells(&[0; 7]), None);
    }

    #[test]
    fn decode_cells_rejects_a_bad_cell_in_a_later_group() {
        let mut cells = vec![0; 16];
        cells[12] = 9;
        assert_eq!(decode_cells(&cells), None);
    }
}
