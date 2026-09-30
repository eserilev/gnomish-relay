//! The line test: the addon draws a beacon and one test line for each mode with an old
//! strip, and the bridge judges the lines (SPEC.md 7.1.4). `Strip.lua` draws the same.

use protocol::frame::{CHECKSUM_LEN, HEADER_LEN, TAG_LEN};

use crate::calibration::{self, Verdict};
use crate::line::{CELLS_PER_ROW, MODES, Mode};
use crate::line_choice::LineChoice;
use crate::strip::Image;

pub const BEACON_MAGIC: [u8; 2] = [0x4C, 0x54];
const BEACON_LEN: usize = 8;
/// 8 pixels right of the old strip, which is 200 cells of 3 pixels.
pub const LEFT: usize = 608;
/// The top of one test line to the top of the next. The gap keeps the lines apart.
pub const ROW_STEP: usize = 4;

/// The physical screen size of the game, from `GetPhysicalScreenSize()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Screen {
    pub width: u32,
    pub height: u32,
}

/// Flat runs show a color shift. The mid levels `55` and `AA` are the ones that gamma
/// moves. The rest gives edges in every channel.
#[allow(clippy::cast_possible_truncation)] // the value is below 256
pub fn payload() -> Vec<u8> {
    let mut bytes = Vec::new();
    for value in [0x00, 0xFF, 0x55, 0xAA] {
        bytes.extend([value; 12]);
    }
    bytes.extend((0..48usize).map(|i| ((i * 37 + 11) % 256) as u8));
    bytes
}

fn fletcher16(bytes: &[u8]) -> [u8; 2] {
    let (mut s1, mut s2) = (0u16, 0u16);
    for &byte in bytes {
        s1 = (s1 + u16::from(byte)) % 255;
        s2 = (s2 + s1) % 255;
    }
    [s1, s2].map(|s| u8::try_from(s).unwrap_or(0))
}

/// The 8 bytes that the addon puts right after the frame.
pub fn beacon(screen: Screen) -> Vec<u8> {
    let side = |value: u32| u16::try_from(value).unwrap_or(0).to_be_bytes();
    let mut bytes = BEACON_MAGIC.to_vec();
    bytes.extend(side(screen.width));
    bytes.extend(side(screen.height));
    let sum = fletcher16(&bytes);
    bytes.extend(sum);
    bytes
}

/// The screen of the beacon after the frame in `bytes`. `None` for a strip with no test.
pub fn beacon_screen(bytes: &[u8]) -> Option<Screen> {
    let len = usize::from(u16::from_be_bytes([*bytes.get(9)?, *bytes.get(10)?]));
    let start = HEADER_LEN + len + CHECKSUM_LEN + TAG_LEN;
    let found = bytes.get(start..start + BEACON_LEN)?;
    if found[..2] != BEACON_MAGIC || found[6..] != fletcher16(&found[..6]) {
        return None;
    }
    let side = |at: usize| u32::from(u16::from_be_bytes([found[at], found[at + 1]]));
    Some(Screen {
        width: side(2),
        height: side(4),
    })
}

/// The top-left pixel of the test line of `mode`.
pub fn origin(mode: Mode) -> (usize, usize) {
    (LEFT, ROW_STEP * (usize::from(mode.id()) - 1))
}

/// A screenshot of another size than the screen is scaled, so no mode can read exactly.
#[allow(clippy::cast_precision_loss)] // a screen side is at most 65535
fn scaled(image: &Image, screen: Screen) -> Option<Vec<(Mode, Verdict)>> {
    let (width, height) = image.size();
    if (width, height) == (screen.width as usize, screen.height as usize) {
        return None;
    }
    let ratio = width as f64 / f64::from(screen.width.max(1));
    let verdict = |mode: Mode| Verdict::Scale {
        across: mode.pixels() as f64 * ratio,
    };
    Some(MODES.iter().map(|&mode| (mode, verdict(mode))).collect())
}

/// The verdict of each test line in the screenshot.
pub fn judge(image: &Image, screen: Screen) -> Vec<(Mode, Verdict)> {
    if let Some(verdicts) = scaled(image, screen) {
        return verdicts;
    }
    let bytes = payload();
    let judge_one = |mode: Mode| {
        let (x, y) = origin(mode);
        let cut = image.crop(x, y, CELLS_PER_ROW * mode.pixels(), ROW_STEP);
        calibration::judge(&cut, mode, &bytes)
    };
    MODES.iter().map(|&mode| (mode, judge_one(mode))).collect()
}

/// The result of the test in the screenshot of a strip, or `None` with no test in it.
pub fn result(image: &Image, bytes: &[u8]) -> Option<LineChoice> {
    let screen = beacon_screen(bytes)?;
    let verdicts = judge(image, screen);
    Some(LineChoice::from_verdicts(
        &verdicts,
        screen.width,
        screen.height,
    ))
}

#[cfg(test)]
mod tests {
    use protocol::frame::encode_frame;

    use super::*;
    use crate::line::{self, Color};
    use crate::line_choice::Reason;

    const SCREEN: Screen = Screen {
        width: 1280,
        height: 720,
    };

    fn frame_with_beacon(screen: Screen) -> Vec<u8> {
        let mut bytes = encode_frame(1_790_211_079, 7, b"hi", [9; 8]).unwrap();
        bytes.extend(beacon(screen));
        bytes.extend([0; 5]);
        bytes
    }

    /// The test lines on a grey scene, with `change` applied to each cell color.
    fn drawn(width: usize, height: usize, change: impl Fn(Color) -> Color) -> Image {
        let mut rgb = vec![70; width * height * 3];
        for mode in MODES {
            let (x0, y0) = origin(mode);
            let p = mode.pixels();
            for (c, &color) in line::rows(&payload(), mode)[0].iter().enumerate() {
                for y in y0..y0 + p {
                    for x in (x0 + c * p..x0 + (c + 1) * p).filter(|&x| x < width) {
                        let at = (y * width + x) * 3;
                        rgb[at..at + 3].copy_from_slice(&change(color));
                    }
                }
            }
        }
        Image::from_rgb(width, height, rgb).unwrap()
    }

    #[test]
    fn the_payload_fits_one_row_in_every_mode() {
        for mode in MODES {
            assert_eq!(line::rows(&payload(), mode).len(), 1, "{}", mode.name());
        }
    }

    #[test]
    fn the_beacon_after_the_frame_gives_the_screen() {
        assert_eq!(beacon_screen(&frame_with_beacon(SCREEN)), Some(SCREEN));
    }

    #[test]
    fn a_strip_with_no_beacon_or_a_damaged_one_has_no_test() {
        let plain = encode_frame(1_790_211_079, 7, b"hi", [9; 8]).unwrap();
        let mut damaged = frame_with_beacon(SCREEN);
        let at = damaged.len() - 5 - 3;
        damaged[at] ^= 1;
        let mut padded = plain.clone();
        padded.extend([0; 8]);

        assert_eq!(beacon_screen(&plain), None);
        assert_eq!(beacon_screen(&damaged), None);
        assert_eq!(beacon_screen(&padded), None);
        assert_eq!(beacon_screen(&[]), None);
    }

    #[test]
    fn sharp_test_lines_choose_mode_1() {
        let image = drawn(1280, 720, |c| c);

        let choice = result(&image, &frame_with_beacon(SCREEN)).unwrap();

        assert_eq!((choice.mode, choice.width, choice.height), (1, 1280, 720));
    }

    #[test]
    fn a_small_color_shift_chooses_the_first_mode_that_it_does_not_reach() {
        let image = drawn(1280, 720, |c| c.map(|v| v.saturating_add(3)));

        let choice = result(&image, &frame_with_beacon(SCREEN)).unwrap();

        assert_eq!(choice.mode, 2);
    }

    #[test]
    fn a_large_color_shift_keeps_the_old_strip_and_says_so() {
        let image = drawn(1280, 720, |c| c.map(|v| v.saturating_add(40)));

        let choice = result(&image, &frame_with_beacon(SCREEN)).unwrap();

        assert_eq!((choice.mode, choice.reason), (0, Some(Reason::ColorShift)));
    }

    #[test]
    fn a_screenshot_of_another_size_than_the_screen_is_scale() {
        let image = drawn(1280, 720, |c| c);
        let bigger = Screen {
            width: 2560,
            height: 1440,
        };

        let verdicts = judge(&image, bigger);

        assert!(
            verdicts
                .iter()
                .all(|(_, v)| matches!(v, Verdict::Scale { .. }))
        );
        let choice = result(&image, &frame_with_beacon(bigger)).unwrap();
        assert_eq!(choice.reason, Some(Reason::Scale));
    }

    #[test]
    fn a_screen_too_narrow_for_the_longest_test_line_still_reads_the_1_pixel_lines() {
        let image = drawn(900, 64, |c| c);
        let narrow = Screen {
            width: 900,
            height: 64,
        };

        let verdicts = judge(&image, narrow);

        assert!(matches!(verdicts[0].1, Verdict::Clean { .. }));
        assert!(!matches!(verdicts[5].1, Verdict::Clean { .. }));
    }

    #[test]
    fn a_screenshot_with_no_test_lines_is_not_found() {
        let image = Image::from_rgb(1280, 720, vec![70; 1280 * 720 * 3]).unwrap();

        let choice = result(&image, &frame_with_beacon(SCREEN)).unwrap();

        assert_eq!((choice.mode, choice.reason), (0, Some(Reason::NotFound)));
    }
}
