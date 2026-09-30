//! The line: the strip of 1- or 2-pixel cells at the top-left corner (SPEC.md 7.1.3).
//! `addon/transport/Codec.lua` draws the same cells, and `tests/addon_codec.rs` checks
//! that both sides agree.

use protocol::frame::{CHECKSUM_LEN, HEADER_LEN, MAX_PAYLOAD, TAG_LEN};

use crate::strip::{Image, full_color_cell};

pub const CELLS_PER_ROW: usize = 200;
pub const MARKER: [u8; 8] = [7, 0, 4, 2, 1, 6, 5, 3];
/// Every level of every channel, in each of the three bit counts.
pub const CHECK: [u8; 12] = [
    0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF, 0xFE, 0xDC, 0xBA, 0x98,
];
/// The marker, then the mode, then 7 minus the mode.
pub const MARKER_CELLS: usize = MARKER.len() + 2;
const MAX_FRAME: usize = HEADER_LEN + MAX_PAYLOAD + CHECKSUM_LEN + TAG_LEN;
const MAX_GROUPS: usize = MAX_FRAME.div_ceil(3);

pub type Color = [u8; 3];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellSize {
    One,
    Two,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Depth {
    Bits24,
    Bits12,
    Bits6,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mode {
    pub size: CellSize,
    pub depth: Depth,
}

/// The order of preference: every 1-pixel mode comes before the 2-pixel modes.
pub const MODES: [Mode; 6] = [
    Mode::new(CellSize::One, Depth::Bits24),
    Mode::new(CellSize::One, Depth::Bits12),
    Mode::new(CellSize::One, Depth::Bits6),
    Mode::new(CellSize::Two, Depth::Bits24),
    Mode::new(CellSize::Two, Depth::Bits12),
    Mode::new(CellSize::Two, Depth::Bits6),
];

impl Mode {
    const fn new(size: CellSize, depth: Depth) -> Mode {
        Mode { size, depth }
    }

    /// Mode 1 to 6, as the marker and `strip-line.json` name it.
    pub fn from_id(id: u8) -> Option<Mode> {
        let index = usize::from(id).checked_sub(1)?;
        MODES.get(index).copied()
    }

    #[allow(clippy::cast_possible_truncation)] // MODES has 6 entries
    pub fn id(self) -> u8 {
        let index = MODES.iter().position(|&m| m == self).unwrap_or(0);
        index as u8 + 1
    }

    pub fn pixels(self) -> usize {
        match self.size {
            CellSize::One => 1,
            CellSize::Two => 2,
        }
    }

    pub fn bits(self) -> u32 {
        match self.depth {
            Depth::Bits24 => 24,
            Depth::Bits12 => 12,
            Depth::Bits6 => 6,
        }
    }

    fn channel_bits(self) -> u32 {
        self.bits() / 3
    }

    fn top_level(self) -> u32 {
        (1 << self.channel_bits()) - 1
    }

    /// The distance between two levels of a channel: 1, 17, or 85.
    #[allow(clippy::cast_possible_truncation)] // at most 85
    pub fn step(self) -> u8 {
        (255 / self.top_level()) as u8
    }

    fn cells_per_group(self) -> usize {
        match self.depth {
            Depth::Bits24 => 1,
            Depth::Bits12 => 2,
            Depth::Bits6 => 4,
        }
    }

    /// For the player: "1 px, 24 bits".
    pub fn name(self) -> String {
        format!("{} px, {} bits", self.pixels(), self.bits())
    }
}

pub fn full_color(cell: u8) -> Color {
    [
        255 * (cell >> 2 & 1),
        255 * (cell >> 1 & 1),
        255 * (cell & 1),
    ]
}

#[allow(clippy::cast_possible_truncation)] // each level times its step is at most 255
fn cell_color(cell: u32, mode: Mode) -> Color {
    let k = mode.channel_bits();
    let top = mode.top_level();
    let step = u32::from(mode.step());
    [
        ((cell >> (2 * k) & top) * step) as u8,
        ((cell >> k & top) * step) as u8,
        ((cell & top) * step) as u8,
    ]
}

fn group_of(chunk: &[u8]) -> u32 {
    let byte = |i: usize| u32::from(chunk.get(i).copied().unwrap_or(0));
    byte(0) << 16 | byte(1) << 8 | byte(2)
}

/// Zero bytes pad the last group of three.
fn data_colors(bytes: &[u8], mode: Mode) -> Vec<Color> {
    let mask = (1 << mode.bits()) - 1;
    let mut colors = Vec::new();
    for chunk in bytes.chunks(3) {
        let group = group_of(chunk);
        for i in (0..mode.cells_per_group()).rev() {
            let shift = u32::try_from(i).unwrap_or(0) * mode.bits();
            colors.push(cell_color(group >> shift & mask, mode));
        }
    }
    colors
}

/// Every cell of the line in order, before it wraps into rows.
pub fn cells(frame: &[u8], mode: Mode) -> Vec<Color> {
    let id = mode.id();
    let mut colors: Vec<Color> = MARKER.iter().map(|&cell| full_color(cell)).collect();
    colors.push(full_color(id));
    colors.push(full_color(7 - id));
    colors.extend(data_colors(&CHECK, mode));
    colors.extend(data_colors(frame, mode));
    colors
}

/// Rows of 200 cells. Black cells fill the last row, so the width never changes.
pub fn rows(frame: &[u8], mode: Mode) -> Vec<Vec<Color>> {
    cells(frame, mode)
        .chunks(CELLS_PER_ROW)
        .map(|row| {
            let mut row = row.to_vec();
            row.resize(CELLS_PER_ROW, [0, 0, 0]);
            row
        })
        .collect()
}

/// The pixel that cell `index` of the line is read at.
pub fn cell_pixel(index: usize, pixels: usize) -> (usize, usize) {
    let (row, col) = (index / CELLS_PER_ROW, index % CELLS_PER_ROW);
    (col * pixels + pixels / 2, row * pixels + pixels / 2)
}

fn read_cell(image: &Image, index: usize, pixels: usize) -> Option<Color> {
    let (x, y) = cell_pixel(index, pixels);
    image.pixel(x, y)
}

/// The mode of a marker that is drawn with cells of `pixels`.
pub fn marker_mode(image: &Image, pixels: usize) -> Option<Mode> {
    let marker: Option<Vec<u8>> = (0..MARKER_CELLS)
        .map(|i| read_cell(image, i, pixels).map(full_color_cell))
        .collect();
    let marker = marker?;
    if marker[..MARKER.len()] != MARKER || marker[8] + marker[9] != 7 {
        return None;
    }
    Mode::from_id(marker[8]).filter(|mode| mode.pixels() == pixels)
}

fn level(value: u8, mode: Mode) -> u32 {
    (u32::from(value) * mode.top_level() + 127) / 255
}

fn cell_value([red, green, blue]: Color, mode: Mode) -> u32 {
    let k = mode.channel_bits();
    level(red, mode) << (2 * k) | level(green, mode) << k | level(blue, mode)
}

/// The group of three bytes that starts at cell `first`. `None` past the image edge.
#[allow(clippy::cast_possible_truncation)] // a group holds 24 bits
fn read_group(image: &Image, mode: Mode, first: usize) -> Option<[u8; 3]> {
    let mut group = 0;
    for i in 0..mode.cells_per_group() {
        let color = read_cell(image, first + i, mode.pixels())?;
        group = group << mode.bits() | cell_value(color, mode);
    }
    Some([(group >> 16) as u8, (group >> 8) as u8, group as u8])
}

/// Up to `groups` groups from cell `first` on. It stops at the image edge.
fn read_bytes(image: &Image, mode: Mode, first: usize, groups: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    for g in 0..groups {
        let Some(group) = read_group(image, mode, first + g * mode.cells_per_group()) else {
            break;
        };
        bytes.extend(group);
    }
    bytes
}

/// The line at the top-left corner: its mode and bytes. The bytes run past the end of
/// the frame, and the frame header gives the real length.
pub fn read(image: &Image) -> Option<(Mode, Vec<u8>)> {
    let mode = [1, 2].into_iter().find_map(|p| marker_mode(image, p))?;
    let check_groups = CHECK.len() / 3;
    if read_bytes(image, mode, MARKER_CELLS, check_groups) != CHECK {
        return None;
    }
    let first = MARKER_CELLS + check_groups * mode.cells_per_group();
    Some((mode, read_bytes(image, mode, first, MAX_GROUPS)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rows on a grey scene of `width` by `height`, as the game draws them.
    fn render(rows: &[Vec<Color>], mode: Mode, width: usize, height: usize) -> Image {
        let mut rgb = vec![70; width * height * 3];
        let p = mode.pixels();
        for (r, row) in rows.iter().enumerate() {
            for (c, color) in row.iter().enumerate() {
                for y in (r * p..(r + 1) * p).take_while(|&y| y < height) {
                    for x in c * p..(c + 1) * p {
                        let at = (y * width + x) * 3;
                        rgb[at..at + 3].copy_from_slice(color);
                    }
                }
            }
        }
        Image::from_rgb(width, height, rgb).unwrap()
    }

    #[allow(clippy::cast_possible_truncation)] // the value is below 256
    fn frame(len: usize) -> Vec<u8> {
        (0..len).map(|i| ((i * 37 + 11) % 256) as u8).collect()
    }

    #[test]
    fn every_mode_reads_back_its_frame() {
        for mode in MODES {
            let bytes = frame(500);
            let image = render(&rows(&bytes, mode), mode, 640, 360);

            let (read_mode, read) = read(&image).unwrap();

            assert_eq!(read_mode, mode);
            assert_eq!(&read[..bytes.len()], bytes, "{}", mode.name());
        }
    }

    #[test]
    fn a_frame_of_500_bytes_is_one_row_of_200_cells_at_24_bits() {
        let rows = rows(&frame(500), MODES[0]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].len(), CELLS_PER_ROW);
    }

    #[test]
    fn the_largest_frame_takes_6_11_and_22_rows() {
        let bytes = frame(MAX_FRAME);
        let heights: Vec<usize> = MODES[..3].iter().map(|&m| rows(&bytes, m).len()).collect();
        assert_eq!(heights, [6, 11, 22]);
    }

    #[test]
    fn the_largest_frame_reads_back_in_every_mode() {
        for mode in MODES {
            let bytes = frame(MAX_FRAME);
            let image = render(&rows(&bytes, mode), mode, 640, 360);
            assert_eq!(&read(&image).unwrap().1[..MAX_FRAME], bytes);
        }
    }

    #[test]
    fn a_level_reads_as_the_nearest_step() {
        let mode = Mode::from_id(2).unwrap();
        assert_eq!(level(17 * 3 + 8, mode), 3);
        assert_eq!(level(17 * 3 + 9, mode), 4);
        assert_eq!(level(255, mode), 15);
    }

    #[test]
    fn the_mode_ids_run_from_1_to_6_in_the_order_of_preference() {
        for (i, mode) in MODES.iter().enumerate() {
            assert_eq!(usize::from(mode.id()), i + 1);
            assert_eq!(Mode::from_id(mode.id()), Some(*mode));
        }
        assert_eq!(Mode::from_id(0), None);
        assert_eq!(Mode::from_id(7), None);
    }

    #[test]
    fn a_line_one_pixel_to_the_right_is_not_read() {
        let mode = MODES[0];
        let mut shifted = vec![vec![[70, 70, 70]]];
        shifted[0].extend(rows(&frame(20), mode).remove(0));
        let image = render(&shifted, mode, 640, 360);
        assert!(read(&image).is_none());
    }

    #[test]
    fn a_line_with_a_wrong_check_is_not_read() {
        let mode = MODES[1];
        let mut rows = rows(&frame(20), mode);
        rows[0][MARKER_CELLS + 3][0] ^= 0x80;
        assert!(read(&render(&rows, mode, 640, 360)).is_none());
    }

    #[test]
    fn a_marker_that_names_another_cell_size_is_not_read() {
        let mode = MODES[0];
        let mut rows = rows(&frame(20), mode);
        rows[0][8] = full_color(4);
        rows[0][9] = full_color(3);
        assert!(read(&render(&rows, mode, 640, 360)).is_none());
    }

    #[test]
    fn a_plain_scene_has_no_line() {
        let image = Image::from_rgb(8, 8, vec![70; 8 * 8 * 3]).unwrap();
        assert!(read(&image).is_none());
    }

    #[test]
    fn a_line_cut_by_the_image_edge_reads_what_it_holds() {
        let mode = MODES[2];
        let bytes = frame(3000);
        let image = render(&rows(&bytes, mode), mode, 200, 3);
        let (_, read) = read(&image).unwrap();
        assert!(read.len() < bytes.len());
        assert_eq!(read, bytes[..read.len()]);
    }
}
