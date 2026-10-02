# Releasing Gnomish Relay

For maintainers only. A release needs push access to `main` and the repository secrets.

## Publish a release

A version tag (`v*`) does the whole release. `.github/workflows/release.yml` runs these
steps in order. Each step starts only when the step before it passed:

1. It checks that the tag, `Cargo.toml`, and `GnomishRelay.toc` name the same version, and
   that `CHANGELOG.md` has notes for that version.
2. It runs all of `.github/workflows/ci.yml` on the tagged commit: Linux, macOS, and
   Windows, with fmt, clippy, the tests, the doc tests, the Lua and WoW API checks, the
   fuzz smoke run, the model, and the proofs. No job skips.
3. It builds the desktop app for each OS and publishes the GitHub release. The notes of the
   release are the section of the version in `CHANGELOG.md`.
4. It calls `.github/workflows/curseforge.yml`. That workflow builds the addon zip and
   uploads it to CurseForge with the BigWigs packager. The zip holds only the
   `GnomishRelay` folder, with the shared transport files copied in: never a key or the
   desktop app's addon files. The changelog of the CurseForge file is the same section of
   `CHANGELOG.md`.

If a check fails, nothing is published: no binaries and no CurseForge upload. The addon
goes out last, because a new addon makes the desktop app update itself from the latest
GitHub release. A tag never starts `curseforge.yml` by itself. A run of it by hand only
builds the zip.

Before you tag, make sure that CI on `main` is green. A red check on the tag stops the
release. To try again, fix `main`, delete the tag (`git push origin :v0.3.0` and
`git tag -d v0.3.0`), and tag the fixed commit. If a draft release exists, delete it on
GitHub too.

To publish:

1. Set the version in `Cargo.toml` and in `addon/GnomishRelay/GnomishRelay.toc` (`## Version`).
   The release job refuses a tag that doesn't match both.
2. Write the changelog section. Add a `## 0.3.0` section at the top of `CHANGELOG.md`, with
   short bullets about what players get. Use the words of the game and of the README, and
   follow the "UI copy" rules of `CLAUDE.md`. Put bug fixes under `### Fixes`. Leave out
   work that players don't see, such as CI, proofs, and refactors. To see the notes as
   the release shows them, run `scripts/changelog-section.sh 0.3.0`. Without notes for
   the version in `Cargo.toml`, the script tests fail, and the release job refuses the tag.
3. Commit, then tag and push:
   ```sh
   git tag -a v0.3.0 -m "Release 0.3.0"
   git push origin main v0.3.0
   ```

## The Rust version of a release

A release checks and builds with one fixed Rust version, `RUST_TOOLCHAIN` in
`.github/workflows/release.yml`. CI on `main` uses the newest stable Rust. So a new clippy
lint fails on `main` first, and it does not stop a release by surprise. Rust 1.99 added
such a lint on the day of 0.4.0.

To move a release to a newer Rust:

1. Make sure that CI on `main` is green with that version. The Rust version is in the log
   of the rust job.
2. Set `RUST_TOOLCHAIN` to it, for example `"1.100.0"`, and commit.

## Live tests

`.github/workflows/live.yml` runs the live tests each Wednesday: the real Claude Code, its
ACP adapter, and the sandbox. A Claude Code update that breaks the desktop app shows up
within a week. SPEC.md 14.8 lists the tests that run and the ones that don't.

The job needs a Claude API key once:

1. Make a key at <https://console.anthropic.com/settings/keys>. A key with a low spend
   limit is enough: a run makes a few short calls.
2. On GitHub, open the repository, then Settings > Secrets and variables > Actions.
3. Add a repository secret with the name `ANTHROPIC_API_KEY` and the key as its value.

Without the secret, the Claude tests skip with a notice, and the other live tests still
run. When a run fails, the job opens the issue "The live tests failed", or adds a comment
to it if it is open. Close the issue when the fix is in.

To run the job now, open Actions > live > Run workflow. To find the Claude Code update
that broke a test, give an older version in `claude_version`, for example `2.1.287`.
To run the tests on your computer, use `scripts/live-tests.sh failed.txt --claude`. They
use the Claude login of your computer.

## CurseForge setup

This is done once, and it's done already:

- The CurseForge project ID is 1719624. It's in the `## X-Curse-Project-ID` line of
  `addon/GnomishRelay/GnomishRelay.toc`. A repository variable `CURSEFORGE_PROJECT_ID`
  overrides it.
- An API token from <https://authors.curseforge.com/#/settings/api-tokens> is the
  repository secret `CF_API_KEY`.

The packager takes the game versions of the upload from the `## Interface` line of
`GnomishRelay.toc`. The line lists one number for each client (SPEC.md 7.9). After a
release that adds a client, open the file on CurseForge and check that it lists the game
version of each client.

Without the ID or the secret, the job still builds the zip, keeps it as an artifact of the
run, and skips the upload. To test the zip locally, run `scripts/package-addon.sh dist`.
