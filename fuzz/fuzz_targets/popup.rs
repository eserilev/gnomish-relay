//! S15 on the compiled code: the popup is printable ASCII plus the one line break
//! before the label. The input is `command NUL label`.
#![no_main]

use libfuzzer_sys::fuzz_target;
use protocol::popup::popup_text;

fuzz_target!(|data: &[u8]| {
    let (command, label) = match data.iter().position(|&b| b == 0) {
        Some(i) => (&data[..i], &data[i + 1..]),
        None => (data, &b""[..]),
    };
    let text = popup_text(command, label);
    assert_eq!(text.iter().filter(|&&b| b == b'\n').count(), 1);
    assert!(
        text.iter()
            .all(|&b| b == b'\n' || (b' '..=b'~').contains(&b))
    );
});
