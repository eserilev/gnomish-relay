//! Records cross the link unchanged (SPEC.md 11.7.11): any payload that the strip parser
//! takes reads back as the same records after the trip through the link.
#![no_main]

use bridge::link::{read_records, records_text};
use libfuzzer_sys::fuzz_target;
use protocol::record::{Record, parse_records};

fn fields(r: &Record) -> [&[u8]; 6] {
    [&r.token, &r.chat, &r.cwd, &r.flags, &r.name, &r.text]
}

fuzz_target!(|data: &[u8]| {
    let Ok(records) = parse_records(data) else {
        return;
    };
    let back = read_records(&records_text(&records)).unwrap();
    assert_eq!(back.len(), records.len());
    for (a, b) in records.iter().zip(&back) {
        assert_eq!(a.id, b.id);
        assert_eq!(fields(a), fields(b));
    }
});
