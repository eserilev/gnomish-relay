# Contributing to Gnomish Relay

Thanks for helping. This guide covers how to build the project, run the checks, keep the
tests in step with the real game, and publish a release.

Before you change code, read two files:

- [`SPEC.md`](SPEC.md): the design, and the source of truth for every rule.
- [`CLAUDE.md`](CLAUDE.md): the rules for code, comments, tests, and commits.

[`VERIFICATION.md`](VERIFICATION.md) covers the proofs of the protocol core.

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

## Publish a release

A version tag (`v*`) does the whole release:

- `.github/workflows/release.yml` tests the tagged commit on Linux, macOS, and Windows,
  builds the desktop app for each OS, and publishes the GitHub release.
- `.github/workflows/curseforge.yml` builds the addon zip and uploads it to CurseForge with
  the BigWigs packager. The zip holds only the `GnomishRelay` folder, with the shared
  transport files copied in: never a key or the desktop app's addon files.

To publish:

1. Set the version in `Cargo.toml` and in `addon/GnomishRelay/GnomishRelay.toc` (`## Version`).
   The release job refuses a tag that doesn't match both.
2. Commit, then tag and push:
   ```sh
   git tag -a v0.3.0 -m "Release 0.3.0"
   git push origin main v0.3.0
   ```

### CurseForge setup

This is done once, and it's done already:

- The CurseForge project ID is 1719624. It's in the `## X-Curse-Project-ID` line of
  `addon/GnomishRelay/GnomishRelay.toc`. A repository variable `CURSEFORGE_PROJECT_ID`
  overrides it.
- An API token from <https://authors.curseforge.com/#/settings/api-tokens> is the
  repository secret `CF_API_KEY`.

Without the ID or the secret, the job still builds the zip, keeps it as an artifact of the
run, and skips the upload. To test the zip locally, run `scripts/package-addon.sh dist`.
