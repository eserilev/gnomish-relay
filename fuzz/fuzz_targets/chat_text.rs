//! S10 on the compiled code: WoW shows `chat_safe` output as the original text.
#![no_main]

use libfuzzer_sys::fuzz_target;
use protocol::wow_text::chat_safe;

/// How the WoW chat frame reads text: `||` is one `|`, and any other `|` starts a code.
fn wow_plain(text: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut bytes = text.iter();
    while let Some(&b) = bytes.next() {
        if b == b'|' && bytes.next() != Some(&b'|') {
            return None;
        }
        out.push(b);
    }
    Some(out)
}

fuzz_target!(|data: &[u8]| {
    assert_eq!(wow_plain(&chat_safe(data)), Some(data.to_vec()));
});
