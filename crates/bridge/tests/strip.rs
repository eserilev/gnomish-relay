//! Reading the strip out of a screenshot, through PNG bytes as WoW writes them.

// Clippy sees helper functions outside `#[test]` as normal code, so its test exceptions miss them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

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
