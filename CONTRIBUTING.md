# Contributing to Gnomish Relay

Thanks for helping. This guide covers how to report a bug, build the project, run the
checks, and submit a change.

Before you change code, read two files:

- [`SPEC.md`](SPEC.md): the design, and the source of truth for every rule.
- [`CLAUDE.md`](CLAUDE.md): the rules for code, comments, tests, and commits.

[`VERIFICATION.md`](VERIFICATION.md) covers the proofs of the protocol core.

## Report a bug

[Open an issue](https://github.com/eserilev/gnomish-relay/issues). Include:

- what you did, what you expected, and what happened instead;
- the output of `gnomish-relay status`;
- your OS, and the lines of the log around the problem (see "Where the log is" in the README).

## Build from source

1. Install Rust stable.
2. Run the desktop app's setup from the repo:
   ```sh
   cargo run -q --bin gnomish-relay -- setup
   ```
3. For addon work, link the addon into the game, so an edit plus `/reload` loads your code:
   ```sh
   scripts/dev-link.sh
   ```

## Run the checks

| Check | Command | When |
|---|---|---|
| Quick loop: format, lints, tests | `scripts/check-fast.sh` | While you work |
| Everything, with the proofs and the models | `scripts/check-all.sh` | Before each commit |

The checks use these tools:

| Tool | For | Script |
|---|---|---|
| Rust stable, with `rustfmt` and `clippy` | the build, the lints, and the tests | `check-fast.sh`, `check-all.sh` |
| `stylua` and `selene` | the format and the lints of the addon | `check-fast.sh`, `check-all.sh` |
| `python3` and `git` | the WoW API gate | `wow-api.sh`, `selftest-api.sh`, `check-all.sh` |
| `cargo-deny` | the licenses and advisories of the dependencies | `check-all.sh` |
| Charon and Aeneas, at the commits in `proofs/TOOLS`, in `~/verif` or in `CHARON_DIR` and `AENEAS_DIR` | the translation of `protocol` to Lean | `extract.sh`, `check-proofs.sh` |
| Lean through `elan`, at the version in `proofs/lean-toolchain` | the proofs | `check-proofs.sh` |
| Quint (`npm install -g @informalsystems/quint`) | the models of the transport | `check-model.sh` |
| Rust nightly and `cargo-fuzz` | the fuzz targets | `fuzz.sh` |
| `cargo-llvm-cov` | the coverage gates | `check-coverage.sh` |

## Update the tests after a game patch

The tests run the addon in a fake game. A self-test addon measures the real game, so the
fake game keeps acting like the real one (`SPEC.md` 14.3). After each client patch:

1. Close the game, and run `scripts/selftest-link.sh`.
2. Start the game and log in. Stay out of combat until the chat says "done".
3. Type `/reload`.
4. In the repo, run:
   ```sh
   cargo run -q --bin gnomish-relay -- selftest collect
   cargo test
   ```
5. Commit `tests/fixtures` and `tests/vectors`.

The first time, collect asks for one more `/reload`: the first session has no saved file
yet, so it can't see the load order. To take the self-test out of the game, run
`scripts/selftest-link.sh --remove`.

## Submit a change

1. For a bigger change, open an issue first, so we can agree on the design.
2. Fork the repo, and make a branch for your change.
3. Write a failing test first, then the change. Every rule in `SPEC.md` has a named test.
4. If the change affects behavior, update `SPEC.md` in the same commit.
5. Run `scripts/check-all.sh`, and make it pass.
6. Write one change per commit, with a one-line message in the imperative, for example
   "Refuse a strip with a bad tag". See `CLAUDE.md` for the full rules.
7. Open a pull request against `main`, and say what the change does and how you tested it.
