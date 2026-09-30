//! Finds the strip in a screenshot and reads its bytes (SPEC.md 7.1 and 7.1.3).

use anyhow::{Context, Result, bail};
use protocol::cell::decode_cells;
use protocol::frame::decode_frame;

use crate::line;

/// Checked in the PNG header, before the pixels are decoded (SPEC.md 6.2, rule 9).
pub const MAX_SIDE: u32 = 4096;
const CELLS_PER_ROW: usize = 200;
const MAX_ROWS: usize = 48;
const CALIBRATION_ROWS: usize = 2;

pub struct Image {
    width: usize,
    height: usize,
    rgb: Vec<u8>,
}

impl Image {
    pub fn from_png(bytes: &[u8]) -> Result<Image> {
        let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let header = decoder.read_header_info()?;
        if header.width > MAX_SIDE || header.height > MAX_SIDE {
            bail!(
                "image is {}x{}, over the limit",
                header.width,
                header.height
            );
        }
        let mut reader = decoder.read_info()?;
        let mut buf = vec![0; reader.output_buffer_size().unwrap_or(0)];
        let info = reader.next_frame(&mut buf)?;
        let channels = match info.color_type {
            png::ColorType::Rgb => 3,
            png::ColorType::Rgba => 4,
            other => bail!("screenshot has color type {other:?}"),
        };
        let (width, height) = (info.width as usize, info.height as usize);
        let pixels = buf
            .get(..info.buffer_size())
            .context("the PNG frame is larger than its buffer")?;
        let rgb = pixels
            .chunks_exact(channels)
            .flat_map(|px| [px[0], px[1], px[2]])
            .collect();
        Ok(Image { width, height, rgb })
    }

    /// Width and height in pixels.
    pub fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    /// `None` when `rgb` does not hold 3 bytes for each pixel.
    pub fn from_rgb(width: usize, height: usize, rgb: Vec<u8>) -> Option<Image> {
        (rgb.len() == width * height * 3).then_some(Image { width, height, rgb })
    }

    pub fn pixel(&self, x: usize, y: usize) -> Option<[u8; 3]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let at = (y * self.width + x) * 3;
        let [red, green, blue] = self.rgb.get(at..at + 3)? else {
            return None;
        };
        Some([*red, *green, *blue])
    }

    fn cell(&self, x: usize, y: usize) -> Option<u8> {
        self.pixel(x, y).map(full_color_cell)
    }
}

/// Bit 2 is red, bit 1 is green, bit 0 is blue. Each channel is on at 128 or more.
pub fn full_color_cell([red, green, blue]: [u8; 3]) -> u8 {
    let on = |channel: u8| u8::from(channel >= 128);
    on(red) << 2 | on(green) << 1 | on(blue)
}

/// The cell size in pixels. UI scaling makes it fractional, for example 3.875.
#[derive(Clone, Copy)]
struct Grid {
    pitch_x: f64,
    pitch_y: f64,
}

impl Grid {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    fn cell(self, image: &Image, row: usize, col: usize) -> Option<u8> {
        let x = (col as f64 + 0.5) * self.pitch_x;
        let y = (row as f64 + 0.5) * self.pitch_y;
        image.cell(x as usize, y as usize)
    }
}

/// The second row runs backwards, so a grid that is one cell off fails.
#[allow(clippy::cast_possible_truncation)]
fn calibration(row: usize, col: usize) -> u8 {
    let step = (col % 8) as u8;
    if row == 0 { step } else { 7 - step }
}

fn fits(image: &Image, grid: Grid) -> bool {
    (0..CALIBRATION_ROWS).all(|row| {
        (0..CELLS_PER_ROW).all(|col| grid.cell(image, row, col) == Some(calibration(row, col)))
    })
}

/// Every cell size from 3 to 8 pixels that matches both calibration rows exactly.
/// A normal screenshot has none. The width steps by 1/200 pixel, so the last of
/// 200 cells lands at most half a pixel off. The height steps by 1/8 pixel.
fn grids(image: &Image) -> impl Iterator<Item = Grid> + '_ {
    (24..=64).flat_map(move |y| {
        (600..=1600)
            .map(move |x| Grid {
                pitch_x: f64::from(x) / 200.0,
                pitch_y: f64::from(y) / 8.0,
            })
            .filter(move |&grid| fits(image, grid))
    })
}

fn data(image: &Image, grid: Grid) -> Option<Vec<u8>> {
    let mut cells = Vec::with_capacity(CELLS_PER_ROW * MAX_ROWS);
    for row in CALIBRATION_ROWS..MAX_ROWS {
        let line: Option<Vec<u8>> = (0..CELLS_PER_ROW)
            .map(|col| grid.cell(image, row, col))
            .collect();
        match line {
            Some(line) => cells.extend(line),
            None => break,
        }
    }
    decode_cells(&cells)
}

/// The bytes of the strip: the line at the corner first (SPEC.md 7.1.3), then the grid
/// of the old strip. The bytes run past the end of the frame, and the frame header gives
/// the real length.
///
/// The calibration rows fix the cell width but not the row height. The frame checksum
/// rules out most heights, but it does not cover the tag: a wrong height can read the
/// payload right and a tag alone in the last row wrong. So `accept`, the tag check,
/// picks among the readings with a good checksum. With none accepted, the first one
/// comes back, so the caller can log why it fails.
pub fn read_with(image: &Image, accept: impl Fn(&[u8]) -> bool) -> Option<Vec<u8>> {
    let mut first = None;
    let line = line::read(image).map(|(_, bytes)| bytes);
    let grid = grids(image).filter_map(|grid| data(image, grid));
    let readings = line
        .into_iter()
        .chain(grid)
        .filter(|bytes| decode_frame(bytes).is_ok());
    for bytes in readings {
        if accept(&bytes) {
            return Some(bytes);
        }
        first.get_or_insert(bytes);
    }
    first
}
