//! The self-test of the line modes (SPEC.md 14.3.1): a verdict for each mode from the
//! screenshot of its line, and the smallest mode that reads exactly.

use crate::line::{self, CELLS_PER_ROW, Color, MARKER, MODES, Mode};
use crate::strip::{Image, full_color_cell};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Verdict {
    Clean {
        max_error: u8,
    },
    ColorShift {
        wrong: usize,
        max_error: u8,
    },
    Blur {
        wrong: usize,
        max_error: u8,
    },
    /// The cell width that the marker shows, in pixels.
    Scale {
        across: f64,
    },
    NotFound,
}

impl Verdict {
    /// Higher is better. Collect keeps the best verdict of all screenshots.
    fn rank(self) -> u8 {
        match self {
            Verdict::Clean { .. } => 3,
            Verdict::ColorShift { .. } | Verdict::Blur { .. } | Verdict::Scale { .. } => 2,
            Verdict::NotFound => 1,
        }
    }

    #[must_use]
    pub fn better(self, other: Verdict) -> Verdict {
        if other.rank() > self.rank() {
            other
        } else {
            self
        }
    }

    /// For the player: what went wrong, and what causes it.
    pub fn describe(self) -> String {
        match self {
            Verdict::Clean { max_error } => format!("clean (largest error {max_error})"),
            Verdict::ColorShift { wrong, max_error } => format!(
                "color shift: {wrong} cells off by up to {max_error}. The game changes colors: gamma, brightness, or a color filter."
            ),
            Verdict::Blur { wrong, max_error } => format!(
                "blur: {wrong} cells mix with their neighbors (up to {max_error}). Anti-aliasing, a render scale, or an upscaler does this."
            ),
            Verdict::Scale { across } => format!(
                "scale: a cell shows {across:.2} pixels wide. The screenshot is scaled: a render scale below 100%, or a screen size that is not the screenshot size."
            ),
            Verdict::NotFound => {
                "not found: no screenshot shows this line. It did not draw, or blur or scale hides it.".into()
            }
        }
    }
}

/// The verdict of one screenshot for `mode`, whose line holds `frame`.
pub fn judge(image: &Image, mode: Mode, frame: &[u8]) -> Verdict {
    if line::marker_mode(image, mode.pixels()) == Some(mode) {
        return compare(image, mode, frame);
    }
    match scaled_marker(image, mode) {
        Some(across) => Verdict::Scale { across },
        None => Verdict::NotFound,
    }
}

/// The first clean mode in the order of preference.
pub fn chosen(verdicts: &[(Mode, Verdict)]) -> Option<Mode> {
    MODES.into_iter().find(|mode| {
        verdicts
            .iter()
            .any(|(m, v)| m == mode && matches!(v, Verdict::Clean { .. }))
    })
}

/// A marker of `mode` in the top pixel row, read with cells `across` pixels wide.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn marker_fits(image: &Image, mode: Mode, across: f64) -> bool {
    let mut wanted = MARKER.to_vec();
    wanted.push(mode.id());
    wanted.push(7 - mode.id());
    wanted.iter().enumerate().all(|(c, &cell)| {
        let x = ((c as f64 + 0.5) * across) as usize;
        image.pixel(x, 0).map(full_color_cell) == Some(cell)
    })
}

/// The middle of the cell widths from 0.5 to 4 pixels at which the marker of `mode`
/// shows. The marker is one row, so it gives no height.
fn scaled_marker(image: &Image, mode: Mode) -> Option<f64> {
    let fits: Vec<f64> = (16..=128)
        .map(|a| f64::from(a) / 32.0)
        .filter(|&across| marker_fits(image, mode, across))
        .collect();
    let (low, high) = (fits.first()?, fits.last()?);
    Some((low + high) / 2.0)
}

/// The error of each channel of one cell. A cell outside the image is off by 255.
fn errors(got: Option<Color>, want: Color) -> [u8; 3] {
    let Some(got) = got else {
        return [255; 3];
    };
    [0, 1, 2].map(|ch| got[ch].abs_diff(want[ch]))
}

/// The neighbor cells of cell `index` in the line: left, right, up, and down.
fn neighbors(index: usize, count: usize) -> Vec<usize> {
    let col = index % CELLS_PER_ROW;
    let mut found = Vec::new();
    if col > 0 {
        found.push(index - 1);
    }
    if col + 1 < CELLS_PER_ROW && index + 1 < count {
        found.push(index + 1);
    }
    if index >= CELLS_PER_ROW {
        found.push(index - CELLS_PER_ROW);
    }
    if index + CELLS_PER_ROW < count {
        found.push(index + CELLS_PER_ROW);
    }
    found
}

/// A blur changes a value only next to another value. So a wrong value in a flat part
/// of the line is a color shift.
fn is_flat(cells: &[Color], index: usize, channel: usize) -> bool {
    let want = cells[index][channel];
    neighbors(index, cells.len())
        .into_iter()
        .all(|n| cells[n][channel] == want)
}

struct Tally {
    wrong_cells: usize,
    wrong_in_flat_parts: usize,
    max_error: u8,
}

fn tally(image: &Image, mode: Mode, cells: &[Color]) -> Tally {
    let margin = mode.step() / 4;
    let mut tally = Tally {
        wrong_cells: 0,
        wrong_in_flat_parts: 0,
        max_error: 0,
    };
    for (index, &want) in cells.iter().enumerate() {
        let (x, y) = line::cell_pixel(index, mode.pixels());
        let errors = errors(image.pixel(x, y), want);
        tally.max_error = tally.max_error.max(errors.into_iter().max().unwrap_or(0));
        let wrong: Vec<usize> = (0..3).filter(|&ch| errors[ch] > margin).collect();
        tally.wrong_cells += usize::from(!wrong.is_empty());
        tally.wrong_in_flat_parts += wrong
            .into_iter()
            .filter(|&ch| is_flat(cells, index, ch))
            .count();
    }
    tally
}

fn compare(image: &Image, mode: Mode, frame: &[u8]) -> Verdict {
    let cells = line::cells(frame, mode);
    let tally = tally(image, mode, &cells);
    let (wrong, max_error) = (tally.wrong_cells, tally.max_error);
    if wrong == 0 {
        return Verdict::Clean { max_error };
    }
    if tally.wrong_in_flat_parts == 0 {
        return Verdict::Blur { wrong, max_error };
    }
    Verdict::ColorShift { wrong, max_error }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDTH: usize = 640;
    const HEIGHT: usize = 360;

    /// As the self-test draws it: gradients, then flat runs of four values.
    fn frame() -> Vec<u8> {
        let mut bytes: Vec<u8> = (0..=255u8)
            .flat_map(|k| [k, 255 - k, k.wrapping_mul(97).wrapping_add(13)])
            .collect();
        for value in [0x00, 0xFF, 0x55, 0xAA] {
            bytes.extend([value; 24]);
        }
        bytes
    }

    /// The line on a grey scene, each cell `across` by `down` pixels, with `change`
    /// applied to each cell color.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    fn drawn(mode: Mode, across: f64, down: f64, change: impl Fn(Color) -> Color) -> Image {
        let mut rgb = vec![70; WIDTH * HEIGHT * 3];
        let span =
            |i: usize, size: f64| (i as f64 * size) as usize..((i + 1) as f64 * size) as usize;
        for (r, row) in line::rows(&frame(), mode).iter().enumerate() {
            for (c, &color) in row.iter().enumerate() {
                for y in span(r, down) {
                    for x in span(c, across) {
                        let at = (y * WIDTH + x) * 3;
                        rgb[at..at + 3].copy_from_slice(&change(color));
                    }
                }
            }
        }
        Image::from_rgb(WIDTH, HEIGHT, rgb).unwrap()
    }

    #[allow(clippy::cast_precision_loss)] // 1 or 2
    fn sharp(mode: Mode) -> Image {
        let p = mode.pixels() as f64;
        drawn(mode, p, p, |c| c)
    }

    /// Each pixel takes a quarter from its left and its right neighbor, as a soft scaler does.
    fn blurred(image: &Image) -> Image {
        let mut rgb = Vec::with_capacity(WIDTH * HEIGHT * 3);
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let [left, here, right] = [x.saturating_sub(1), x, (x + 1).min(WIDTH - 1)]
                    .map(|at| image.pixel(at, y).unwrap().map(u16::from));
                let mix = |ch: usize| (left[ch] + 2 * here[ch] + right[ch]) / 4;
                rgb.extend([0, 1, 2].map(|ch| u8::try_from(mix(ch)).unwrap()));
            }
        }
        Image::from_rgb(WIDTH, HEIGHT, rgb).unwrap()
    }

    #[test]
    fn a_sharp_line_is_clean_in_every_mode() {
        for mode in MODES {
            assert_eq!(
                judge(&sharp(mode), mode, &frame()),
                Verdict::Clean { max_error: 0 },
                "{}",
                mode.name()
            );
        }
    }

    #[test]
    fn a_small_shift_is_clean_at_6_bits_and_a_color_shift_at_24_bits() {
        let shifted = |mode| drawn(mode, 1.0, 1.0, |c| c.map(|v| v.saturating_add(3)));
        let (full, six) = (MODES[0], MODES[2]);

        let at_six = judge(&shifted(six), six, &frame());
        let at_full = judge(&shifted(full), full, &frame());

        assert_eq!(at_six, Verdict::Clean { max_error: 3 });
        assert!(
            matches!(at_full, Verdict::ColorShift { max_error: 3, .. }),
            "{at_full:?}"
        );
    }

    #[test]
    fn a_blurred_line_of_2_pixel_cells_is_blur() {
        let mode = MODES[3];
        let verdict = judge(&blurred(&sharp(mode)), mode, &frame());
        assert!(matches!(verdict, Verdict::Blur { .. }), "{verdict:?}");
    }

    /// The marker of 1-pixel cells mixes with its neighbors, so no marker is left to find.
    #[test]
    fn a_blurred_line_of_1_pixel_cells_is_not_found() {
        let mode = MODES[0];
        assert_eq!(
            judge(&blurred(&sharp(mode)), mode, &frame()),
            Verdict::NotFound
        );
    }

    #[test]
    fn a_line_shown_at_another_cell_size_is_scale() {
        let mode = MODES[0];
        let verdict = judge(&drawn(mode, 1.5, 1.5, |c| c), mode, &frame());
        let Verdict::Scale { across } = verdict else {
            panic!("{verdict:?}");
        };
        assert!((across - 1.5).abs() < 0.2, "{across}");
    }

    #[test]
    fn a_screenshot_with_no_line_is_not_found() {
        let image = Image::from_rgb(WIDTH, HEIGHT, vec![70; WIDTH * HEIGHT * 3]).unwrap();
        assert_eq!(judge(&image, MODES[0], &frame()), Verdict::NotFound);
    }

    #[test]
    fn the_line_of_another_mode_is_not_found_for_this_mode() {
        assert_eq!(
            judge(&sharp(MODES[1]), MODES[0], &frame()),
            Verdict::NotFound
        );
    }

    #[test]
    fn the_chosen_mode_is_the_first_clean_mode_in_the_order_of_preference() {
        let blur = Verdict::Blur {
            wrong: 3,
            max_error: 90,
        };
        let clean = Verdict::Clean { max_error: 0 };
        let verdicts = [
            (MODES[4], clean),
            (MODES[0], blur),
            (MODES[3], clean),
            (MODES[1], Verdict::NotFound),
        ];
        assert_eq!(chosen(&verdicts), Some(MODES[3]));
        assert_eq!(chosen(&verdicts[1..2]), None);
    }

    #[test]
    fn a_clean_verdict_beats_a_failure_and_a_failure_beats_not_found() {
        let clean = Verdict::Clean { max_error: 0 };
        let blur = Verdict::Blur {
            wrong: 1,
            max_error: 9,
        };
        assert_eq!(Verdict::NotFound.better(blur), blur);
        assert_eq!(blur.better(clean), clean);
        assert_eq!(clean.better(Verdict::NotFound), clean);
    }

    #[test]
    fn each_verdict_says_what_causes_it() {
        let shift = Verdict::ColorShift {
            wrong: 1,
            max_error: 3,
        };
        let blur = Verdict::Blur {
            wrong: 1,
            max_error: 3,
        };
        assert!(
            Verdict::Clean { max_error: 0 }
                .describe()
                .starts_with("clean")
        );
        assert!(shift.describe().contains("gamma"));
        assert!(blur.describe().contains("Anti-aliasing"));
        assert!(
            Verdict::Scale { across: 1.5 }
                .describe()
                .contains("render scale")
        );
        assert!(Verdict::NotFound.describe().starts_with("not found"));
    }
}
