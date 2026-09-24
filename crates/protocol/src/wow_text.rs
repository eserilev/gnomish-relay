//! Text for the WoW chat frame. WoW reads `|` as the start of an escape code, for
//! links (`|H`), colors (`|c`), and textures (`|T`). `||` shows one `|`.

/// Doubles every `|`, so agent text shows as plain text.
#[must_use]
pub fn chat_safe(text: &[u8]) -> Vec<u8> {
    todo!()
}
