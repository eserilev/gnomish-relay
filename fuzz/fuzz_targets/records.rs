//! S3 and C3 on the compiled code: parsed records serialize and parse back the same.
#![no_main]

use libfuzzer_sys::fuzz_target;
use protocol::record::{Record, parse_records, serialize_records};

fn same(a: &Record, b: &Record) -> bool {
    a.token == b.token
        && a.chat == b.chat
        && a.id == b.id
        && a.cwd == b.cwd
        && a.flags == b.flags
        && a.name == b.name
        && a.text == b.text
}

fuzz_target!(|data: &[u8]| {
    let Ok(records) = parse_records(data) else {
        return;
    };
    let Ok(again) = parse_records(&serialize_records(&records)) else {
        panic!("serialized records do not parse");
    };
    assert_eq!(records.len(), again.len());
    assert!(records.iter().zip(&again).all(|(a, b)| same(a, b)));
});
