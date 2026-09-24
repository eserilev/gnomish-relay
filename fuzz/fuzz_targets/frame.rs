//! S1 and C2 on the compiled code: a frame that decodes encodes back to the same bytes.
#![no_main]

use libfuzzer_sys::fuzz_target;
use protocol::frame::{decode_frame, encode_frame, signed_len};

fuzz_target!(|data: &[u8]| {
    let Ok(frame) = decode_frame(data) else {
        return;
    };
    let again = encode_frame(frame.time, frame.frame_id, &frame.payload, frame.tag)
        .expect("a decoded payload is short enough to encode");
    assert_eq!(&data[..again.len()], &again[..]);
    assert!(signed_len(&frame) <= data.len());
});
