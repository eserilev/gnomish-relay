//! The permission popup. A malicious agent can label `rm -rf ~` as "run tests",
//! so the popup shows the raw command first and the label of the agent below it.
//! See `SPEC.md` 6.4.

/// Raw command bytes shown in full. A longer command shows its start and end.
pub const COMMAND_BUDGET: usize = 300;
pub const LABEL_BUDGET: usize = 120;

/// Every byte outside printable ASCII shows as `\xHH`. This also hides bidi
/// and zero-width tricks.
#[must_use]
pub fn popup_text(command: &[u8], label: &[u8]) -> Vec<u8> {
    todo!()
}
