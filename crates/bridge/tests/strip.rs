//! Reading the strip out of a screenshot, through PNG bytes as WoW writes them.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use bridge::line::{self, Color, MODES, Mode};
use bridge::strip::{Image, MAX_SIDE, read_with};
use common::{HEIGHT, WIDTH, encode_png, scene, screenshot_png, strip_rows};
use png::{BitDepth, ColorType};
use protocol::frame::{decode_frame, encode_frame};

fn frame(payload: &[u8]) -> Vec<u8> {
    encode_frame(1_790_211_079, 7, payload, [9; 8]).unwrap()
}

fn payload_of(png_bytes: &[u8]) -> Option<Vec<u8>> {
    let bytes = read_with(&Image::from_png(png_bytes).ok()?, |_| true)?;
    decode_frame(&bytes).ok().map(|f| f.payload)
}

#[test]
fn a_strip_at_a_fractional_cell_size_reads_back() {
    let png_bytes = screenshot_png(&strip_rows(&frame(b"hello from the game")));
    assert_eq!(payload_of(&png_bytes).unwrap(), b"hello from the game");
}

#[test]
fn the_largest_frame_reads_back() {
    let payload = vec![b'x'; 3200];
    let rgb = scene(&strip_rows(&frame(&payload)), 4.0, 4.0);
    let png_bytes = encode_png(WIDTH, HEIGHT, ColorType::Rgb, BitDepth::Eight, &rgb);
    assert_eq!(payload_of(&png_bytes).unwrap(), payload);
}

/// The checksum does not cover the tag. So a wrong row height can read the payload right
/// and the tag wrong, when the tag sits alone in the last row.
/// The addon draws 3-pixel cells. A screenshot scaled down to two thirds shows 2 pixels.
#[test]
fn the_largest_frame_reads_back_at_cell_sizes_from_2_pixels_up() {
    let payload = vec![b'z'; 3200];
    // A strip wider than 6.4 pixels a cell does not fit the scene of 1280 pixels.
    for pitch in [2.0, 2.25, 2.5, 2.875, 3.0, 3.5, 4.0, 5.0, 6.25] {
        let rgb = scene(&strip_rows(&frame(&payload)), pitch, pitch);
        let png_bytes = encode_png(WIDTH, HEIGHT, ColorType::Rgb, BitDepth::Eight, &rgb);
        assert!(
            payload_of(&png_bytes) == Some(payload.clone()),
            "pitch {pitch}"
        );
    }
}

#[test]
fn a_short_frame_reads_back_in_3_pixel_cells_with_a_tag_alone_in_the_last_row() {
    let tag_checks = |bytes: &[u8]| decode_frame(bytes).is_ok_and(|f| f.tag == [9; 8]);
    for len in 55..70 {
        let rgb = scene(&strip_rows(&frame(&vec![b'q'; len])), 3.0, 3.0);
        let png_bytes = encode_png(WIDTH, HEIGHT, ColorType::Rgb, BitDepth::Eight, &rgb);

        let bytes = read_with(&Image::from_png(&png_bytes).unwrap(), tag_checks).unwrap();

        let tag = decode_frame(&bytes).ok().map(|f| f.tag);
        assert_eq!(tag, Some([9; 8]), "payload of {len}");
    }
}

#[test]
fn a_tag_alone_in_the_last_row_reads_back() {
    let tag_checks = |bytes: &[u8]| decode_frame(bytes).is_ok_and(|f| f.tag == [9; 8]);
    for len in 55..70 {
        let payload = vec![b'p'; len];
        let png_bytes = screenshot_png(&strip_rows(&frame(&payload)));

        let bytes = read_with(&Image::from_png(&png_bytes).unwrap(), tag_checks).unwrap();

        let tag = decode_frame(&bytes).ok().map(|f| f.tag);
        assert_eq!(tag, Some([9; 8]), "payload of {len}");
    }
}

#[test]
fn a_strip_whose_tag_never_checks_still_reads_so_the_bridge_can_log_it() {
    let png_bytes = screenshot_png(&strip_rows(&frame(b"a strip of another key")));

    let bytes = read_with(&Image::from_png(&png_bytes).unwrap(), |_| false);

    let payload = decode_frame(&bytes.unwrap()).ok().map(|f| f.payload);
    assert_eq!(
        payload.as_deref(),
        Some(b"a strip of another key".as_slice())
    );
}

#[test]
fn a_normal_screenshot_has_no_strip() {
    assert!(payload_of(&screenshot_png(&[])).is_none());
}

#[test]
fn a_strip_with_one_wrong_calibration_cell_is_not_found() {
    let mut rows = strip_rows(&frame(b"x"));
    rows[1][150] ^= 1;
    assert!(payload_of(&screenshot_png(&rows)).is_none());
}

#[test]
fn an_rgba_screenshot_decodes() {
    let rgb = scene(&strip_rows(&frame(b"rgba")), 3.875, 4.0);
    let rgba: Vec<u8> = rgb
        .chunks(3)
        .flat_map(|px| [px[0], px[1], px[2], 255])
        .collect();
    let png_bytes = encode_png(WIDTH, HEIGHT, ColorType::Rgba, BitDepth::Eight, &rgba);
    assert_eq!(payload_of(&png_bytes).unwrap(), b"rgba");
}

#[test]
fn a_16_bit_screenshot_decodes() {
    let rgb = scene(&strip_rows(&frame(b"deep")), 4.0, 4.0);
    let wide: Vec<u8> = rgb.iter().flat_map(|&c| [c, c]).collect();
    let png_bytes = encode_png(WIDTH, HEIGHT, ColorType::Rgb, BitDepth::Sixteen, &wide);
    assert_eq!(payload_of(&png_bytes).unwrap(), b"deep");
}

#[test]
fn a_grayscale_image_is_an_error() {
    let png_bytes = encode_png(4, 4, ColorType::Grayscale, BitDepth::Eight, &[0; 16]);
    assert!(Image::from_png(&png_bytes).is_err());
}

#[test]
fn a_cut_off_png_is_an_error_not_a_panic() {
    let png_bytes = screenshot_png(&strip_rows(&frame(b"cut")));
    for len in [0, 8, 33, 100, png_bytes.len() / 2] {
        assert!(Image::from_png(&png_bytes[..len]).is_err(), "length {len}");
    }
}

#[test]
fn an_image_over_the_size_limit_is_refused_before_decoding() {
    let data = vec![0; (MAX_SIDE as usize + 1) * 3];
    let png_bytes = encode_png(MAX_SIDE + 1, 1, ColorType::Rgb, BitDepth::Eight, &data);
    assert!(Image::from_png(&png_bytes).is_err());
}

/// A 1280x720 scene with the line of `frame` drawn at the corner, as a PNG.
fn line_png(frame: &[u8], mode: Mode) -> Vec<u8> {
    let (width, p) = (WIDTH as usize, mode.pixels());
    let mut rgb = vec![70u8; width * HEIGHT as usize * 3];
    for (r, row) in line::rows(frame, mode).iter().enumerate() {
        for (c, color) in row.iter().enumerate() {
            paint(&mut rgb, width, (c * p, r * p), p, *color);
        }
    }
    encode_png(WIDTH, HEIGHT, ColorType::Rgb, BitDepth::Eight, &rgb)
}

fn paint(rgb: &mut [u8], width: usize, (x0, y0): (usize, usize), size: usize, color: Color) {
    for y in y0..y0 + size {
        for x in x0..x0 + size {
            let at = (y * width + x) * 3;
            rgb[at..at + 3].copy_from_slice(&color);
        }
    }
}

#[test]
fn a_line_in_every_mode_reads_back_through_a_png() {
    for mode in MODES {
        let png_bytes = line_png(&frame(b"a line at the corner"), mode);
        assert_eq!(
            payload_of(&png_bytes).as_deref(),
            Some(b"a line at the corner".as_slice()),
            "{}",
            mode.name()
        );
    }
}

#[test]
fn the_largest_frame_reads_back_as_a_line_in_every_mode() {
    let payload = vec![b'y'; 3200];
    for mode in MODES {
        let png_bytes = line_png(&frame(&payload), mode);
        assert_eq!(payload_of(&png_bytes).unwrap(), payload, "{}", mode.name());
    }
}

#[test]
fn a_line_with_a_broken_frame_is_not_a_strip() {
    let mut wire = frame(b"damaged");
    wire[12] ^= 1;
    assert!(payload_of(&line_png(&wire, MODES[0])).is_none());
}
