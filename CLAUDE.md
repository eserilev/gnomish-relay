# Gnomish Relay: rules for agents

Read `SPEC.md` before you change code. It is the source of truth for the design.

The two priorities of this project are readability and test coverage.
If a change makes the code harder to read or less tested, do not make it.

## Comments

A comment says why, never what. If a comment repeats the code, delete it.

- Most functions get no comment. A good name does the job.
- A doc comment is one line. Add more only for a surprise, a trap, or a contract that the types do not show.
- Do not write "This function...", "Note that...", "It is important to...", "robust", "ensures", "handles".
- A `TODO` says what is missing and when it goes away: `// TODO: remove when Forever gets C_File`.
- Write comments in simple English: short sentences, active voice, no "should", "may", "might", "could", "would".

Bad:

```rust
// Increment the retry counter to track the number of attempts made
retries += 1;

/// This function is responsible for decoding the pixel strip.
/// It takes a frame as input and returns a decoded message,
/// ensuring robust handling of edge cases.
fn decode(frame: &Frame) -> Result<Message, DecodeError>
```

Good:

```rust
retries += 1;

/// Returns `NoStrip` for plain game UI, which is most frames.
fn decode(frame: &Frame) -> Result<Message, DecodeError>

// WoW only sees addon files that exist at launch, so we make every slot up front.
```

## Readability

- Names over comments: `slot_is_unread(slot)`, not `check(slot) // true if unread`.
- One job per function. If you need "and" to describe it, split it.
- Flat control flow: early returns and `?`. No deep nesting.
- No clever code. No macro where a function works. No generic with one caller. No iterator chain longer than three steps.
- Newtypes for IDs (`ChatId`, `MessageId`, `SessionId`). Enums in place of `bool` flags.
- One module, one idea. The file name says what is inside.
- No `unwrap()` or `expect()` outside tests.
- Errors: `thiserror` in library crates, `anyhow` only in the `bridge` binary.

## Tests

- Every rule in `SPEC.md` has at least one named test.
- Test names are sentences: `decode_rejects_strip_with_bad_mac`, not `test_decode_3`.
- Each test reads top to bottom: arrange, act, assert. No shared magic fixtures across files.
- Use the fake agent and the fake capture for bridge tests. No test needs the game or a real LLM, except tests marked `#[ignore]` for live runs.
- Coverage gates (`cargo llvm-cov`): `protocol` 95% of lines, `bridge` and `agents` 80%. `capture` has no gate. Golden screenshots cover it.
- A bug fix starts with a failing test.

## The verified core (`crates/protocol`)

We prove `protocol` correct with Aeneas and Lean. So `protocol` stays inside the Rust subset that Aeneas supports:

- Safe Rust only. `#![forbid(unsafe_code)]`.
- No `async`, no threads, no I/O, no `dyn` traits, no closures stored in structs.
- No `return` inside nested loops. No `break` or `continue` to an outer loop.
- Do not call a generic function with a `&mut` type argument (for example `id(&mut x)`).
- Keep data simple: integers, arrays, slices, `Vec<u8>`, plain structs and enums. Do UTF-8 and string work outside the core.
- Keep each function small. A small function gives a small proof.
- No local variable with the same name as a module. In Lean, `cells.X` then means a field of the variable, and the build fails.
- Byte-string constants are arrays (`const X: [u8; 6] = *b"...";`), not `&[u8]`. Aeneas cannot translate a reference in a constant.
- A `return` inside a loop works only when the loop is the last statement of its function. Move such a loop into its own function.
- No `loop { ... break }` with array updates. Aeneas could not translate it. A `while` loop or plain recursion works.
- No `?` operator. Aeneas has no model for it. Use `let ... else`.
- The Lean code that Aeneas generates must contain no `axiom`. An axiom means that Aeneas did not know a function, and the proof then trusts it blindly.
- Do not cast `bool` to an integer. Use integer bit operations, for example `(v >> 2) & 1`.
- No `native_decide`, `bv_decide`, or `bv_tac` in proofs. They add a native-code axiom. Prove bit facts bit by bit: `ext i hi`, `interval_cases i`, `simp`.
- Add each top theorem to `proofs/Axioms.lean`. Only `propext`, `Classical.choice`, and `Quot.sound` are allowed.
- `cases` on an enum fails when the goal uses `⦃ ⦄`. Use `induction x` instead.
- Put bit arithmetic in tiny helper functions that take and return integers. Large functions make the proof time out.
- Every function that reads untrusted input returns a value or a defined error for every input. It never panics. Prove it.

If a change to `protocol` breaks the Aeneas translation, fix the code, not the proof tooling.
Before every commit, run `scripts/check-all.sh`. It runs fmt, clippy, the tests, and `scripts/check-proofs.sh`. It regenerates the Lean code, builds the proofs, and checks the axioms of every `check_` theorem.

## Tooling

- `cargo fmt` and `cargo clippy --all-targets -- -D warnings` pass before every commit.
- `stylua` and `luacheck` pass for the addon.
- `#![forbid(unsafe_code)]` in every crate except `capture`. In `capture`, each `unsafe` block has a `// SAFETY:` comment.

## Commits

- One change per commit.
- A commit message is one line, in simple English, in the imperative. No body.
- No AI credit in commits or PRs: no `Co-Authored-By` line, no "Generated with" line.
