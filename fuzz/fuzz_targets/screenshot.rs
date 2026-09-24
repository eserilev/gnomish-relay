//! Any bytes in the Screenshots folder: the PNG decoder and the strip reader never
//! panic (SPEC.md 14.4). A local program can write any file there.
#![no_main]

use bridge::strip::{Image, read};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(image) = Image::from_png(data) {
        let _ = read(&image);
    }
});
