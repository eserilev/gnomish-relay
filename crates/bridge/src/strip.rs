//! Finds the strip in a screenshot and reads its bytes (SPEC.md 7.1).

use anyhow::{Context, Result, bail};
use protocol::cell::decode_cells;
use protocol::frame::decode_frame;

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

    /// Bit 2 is red, bit 1 is green, bit 0 is blue. Each channel is on at 128 or more.
    fn cell(&self, x: usize, y: usize) -> Option<u8> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let at = (y * self.width + x) * 3;
        let [red, green, blue] = self.rgb.get(at..at + 3)? else {
            return None;
        };
        let on = |channel: u8| u8::from(channel >= 128);
        Some(on(*red) << 2 | on(*green) << 1 | on(*blue))
    }
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

/// The bytes of the data rows. The calibration rows fix the cell width but not the
/// row height, so the frame checksum picks the grid. The bytes run past the end of
/// the frame, and the frame header gives the real length.
pub fn read(image: &Image) -> Option<Vec<u8>> {
    grids(image).find_map(|grid| data(image, grid).filter(|bytes| decode_frame(bytes).is_ok()))
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use protocol::frame::{decode_frame, encode_frame};

    /// Draws cells the way WoW does at a fractional cell size, on a gray game scene.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    pub fn render(rows: &[Vec<u8>], pitch_x: f64, pitch_y: f64) -> Image {
        let (width, height) = (1280, 720);
        let mut rgb = vec![90; width * height * 3];
        for (r, row) in rows.iter().enumerate() {
            for (c, &cell) in row.iter().enumerate() {
                let (x0, x1) = (
                    (c as f64 * pitch_x) as usize,
                    ((c + 1) as f64 * pitch_x) as usize,
                );
                let (y0, y1) = (
                    (r as f64 * pitch_y) as usize,
                    ((r + 1) as f64 * pitch_y) as usize,
                );
                for y in y0..y1 {
                    for x in x0..x1 {
                        let at = (y * width + x) * 3;
                        rgb[at] = 255 * (cell >> 2 & 1);
                        rgb[at + 1] = 255 * (cell >> 1 & 1);
                        rgb[at + 2] = 255 * (cell & 1);
                    }
                }
            }
        }
        Image { width, height, rgb }
    }

    pub fn strip_rows(frame: &[u8]) -> Vec<Vec<u8>> {
        let mut rows: Vec<Vec<u8>> = (0..2)
            .map(|r| (0..CELLS_PER_ROW).map(|c| calibration(r, c)).collect())
            .collect();
        let cells = protocol::cell::encode_cells(frame);
        rows.extend(cells.chunks(CELLS_PER_ROW).map(<[u8]>::to_vec));
        rows
    }

    fn frame(payload: &[u8]) -> Vec<u8> {
        encode_frame(1_790_211_079, 7, payload, [9; 8]).unwrap()
    }

    #[test]
    fn a_strip_at_a_fractional_cell_size_reads_back() {
        let wire = frame(b"hello from the game");
        let image = render(&strip_rows(&wire), 3.875, 4.0);
        let bytes = read(&image).expect("the strip is found");
        let decoded = decode_frame(&bytes).ok().expect("the frame decodes");
        assert_eq!(decoded.payload, b"hello from the game");
    }

    #[test]
    fn the_largest_frame_reads_back() {
        let payload = vec![b'x'; 3200];
        let image = render(&strip_rows(&frame(&payload)), 4.0, 4.0);
        let bytes = read(&image).unwrap();
        assert_eq!(decode_frame(&bytes).ok().unwrap().payload, payload);
    }

    #[test]
    fn a_normal_screenshot_has_no_strip() {
        assert!(read(&render(&[], 4.0, 4.0)).is_none());
    }

    #[test]
    fn a_strip_with_one_wrong_calibration_cell_is_not_found() {
        let mut rows = strip_rows(&frame(b"x"));
        rows[1][150] ^= 1;
        assert!(read(&render(&rows, 4.0, 4.0)).is_none());
    }

    #[test]
    fn a_png_screenshot_decodes() {
        let image = render(&strip_rows(&frame(b"png")), 3.875, 4.0);
        let mut png_bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut png_bytes, 1280, 720);
        encoder.set_color(png::ColorType::Rgb);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&image.rgb)
            .unwrap();
        let read_back = Image::from_png(&png_bytes).unwrap();
        assert_eq!(
            decode_frame(&read(&read_back).unwrap())
                .ok()
                .unwrap()
                .payload,
            b"png"
        );
    }

    fn encode(
        width: u32,
        height: u32,
        color: png::ColorType,
        depth: png::BitDepth,
        data: &[u8],
    ) -> Vec<u8> {
        let mut png_bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut png_bytes, width, height);
        encoder.set_color(color);
        encoder.set_depth(depth);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(data)
            .unwrap();
        png_bytes
    }

    fn with_alpha(image: &Image) -> Vec<u8> {
        image
            .rgb
            .chunks(3)
            .flat_map(|px| [px[0], px[1], px[2], 255])
            .collect()
    }

    #[test]
    fn an_rgba_screenshot_decodes() {
        let image = render(&strip_rows(&frame(b"rgba")), 3.875, 4.0);
        let png_bytes = encode(
            1280,
            720,
            png::ColorType::Rgba,
            png::BitDepth::Eight,
            &with_alpha(&image),
        );
        let read_back = Image::from_png(&png_bytes).unwrap();
        assert_eq!(
            decode_frame(&read(&read_back).unwrap())
                .ok()
                .unwrap()
                .payload,
            b"rgba"
        );
    }

    #[test]
    fn a_16_bit_screenshot_decodes() {
        let image = render(&strip_rows(&frame(b"deep")), 4.0, 4.0);
        let wide: Vec<u8> = image.rgb.iter().flat_map(|&c| [c, c]).collect();
        let png_bytes = encode(
            1280,
            720,
            png::ColorType::Rgb,
            png::BitDepth::Sixteen,
            &wide,
        );
        let read_back = Image::from_png(&png_bytes).unwrap();
        assert_eq!(
            decode_frame(&read(&read_back).unwrap())
                .ok()
                .unwrap()
                .payload,
            b"deep"
        );
    }

    #[test]
    fn a_grayscale_image_is_an_error() {
        let png_bytes = encode(
            4,
            4,
            png::ColorType::Grayscale,
            png::BitDepth::Eight,
            &[0; 16],
        );
        assert!(Image::from_png(&png_bytes).is_err());
    }

    #[test]
    fn a_cut_off_png_is_an_error_not_a_panic() {
        let image = render(&strip_rows(&frame(b"cut")), 4.0, 4.0);
        let png_bytes = encode(
            1280,
            720,
            png::ColorType::Rgb,
            png::BitDepth::Eight,
            &image.rgb,
        );
        for len in [0, 8, 33, 100, png_bytes.len() / 2] {
            assert!(Image::from_png(&png_bytes[..len]).is_err(), "length {len}");
        }
    }

    #[test]
    fn an_image_over_the_size_limit_is_refused_before_decoding() {
        let mut png_bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut png_bytes, MAX_SIDE + 1, 1);
        encoder.set_color(png::ColorType::Rgb);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&vec![0; (MAX_SIDE as usize + 1) * 3])
            .unwrap();
        assert!(Image::from_png(&png_bytes).is_err());
    }
}
