//! S1 and C2 on the compiled code: a frame that decodes encodes back to the same bytes.
//! Then S29 with two keys: the frame, signed again by the key that the input picks, goes
//! only to the app of that key (SPEC.md 9.7, decision 2).
#![no_main]

use bridge::receive::{KeySet, Rejected, StripKey, receive};
use libfuzzer_sys::fuzz_target;
use protocol::apps::App;
use protocol::frame::{decode_frame, encode_frame, signed_len};

const RELAY: &str = "11";
const TIMEWAYS: &str = "22";
const OTHER: &str = "33";

fn key(byte: &str) -> StripKey {
    StripKey::from_hex(&byte.repeat(32)).expect("a key of 32 hex bytes")
}

fuzz_target!(|data: &[u8]| {
    let Ok(frame) = decode_frame(data) else {
        return;
    };
    let mut again = encode_frame(frame.time, frame.frame_id, &frame.payload, frame.tag)
        .expect("a decoded payload is short enough to encode");
    assert_eq!(&data[..again.len()], &again[..]);
    assert!(signed_len(&frame) <= data.len());

    let (signer, expected) = match frame.frame_id % 3 {
        0 => (key(RELAY), Some(App::Relay)),
        1 => (key(TIMEWAYS), Some(App::Timeways)),
        _ => (key(OTHER), None),
    };
    let signed = signed_len(&frame);
    let tag = signer.tag(&again[..signed]);
    again[signed..].copy_from_slice(&tag);
    let keys = KeySet::new(key(RELAY), Some(key(TIMEWAYS))).expect("two different keys");
    match (receive(&again, &keys, frame.time), expected) {
        (Ok((app, _)), Some(signed_by)) => assert_eq!(app, signed_by),
        (Err(Rejected::BadRecords), Some(_)) => {}
        (Err(Rejected::BadTag), None) => {}
        (result, expected) => panic!("{expected:?} gave {:?}", result.map(|(app, _)| app)),
    }
    let relay_only = KeySet::new(key(RELAY), None).expect("one key");
    if let Ok((app, _)) = receive(&again, &relay_only, frame.time) {
        assert_eq!(app, App::Relay, "no Timeways key, no Timeways strip");
    }
});
