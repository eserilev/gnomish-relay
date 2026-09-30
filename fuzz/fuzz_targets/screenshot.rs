//! Any bytes in the Screenshots folder: the PNG decoder, the strip reader, and the line
//! test never panic (SPEC.md 14.4 and 7.1.4). A local program can write any file there.
#![no_main]

use bridge::line_test::{self, Screen};
use bridge::strip::{Image, read_with};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(image) = Image::from_png(data) else {
        return;
    };
    if let Some(bytes) = read_with(&image, |_| false) {
        let _ = line_test::result(&image, &bytes);
    }
    // A screen of the image size reaches the cut of each test line.
    let (width, height) = image.size();
    let side = |value: usize| u32::try_from(value).unwrap_or(0);
    let _ = line_test::judge(
        &image,
        Screen {
            width: side(width),
            height: side(height),
        },
    );
});
