# Releasing Gnomish Relay

For maintainers only. A release needs push access to `main` and the repository secrets.

## Publish a release

A version tag (`v*`) does the whole release. `.github/workflows/release.yml` runs these
steps in order. Each step starts only when the step before it passed:

1. It checks that the tag, `Cargo.toml`, and `GnomishRelay.toc` name the same version.
2. It runs all of `.github/workflows/ci.yml` on the tagged commit: Linux, macOS, and
   Windows, with fmt, clippy, the tests, the doc tests, the Lua and WoW API checks, the
   fuzz smoke run, the model, and the proofs. No job skips.
3. It builds the desktop app for each OS and publishes the GitHub release.
4. It calls `.github/workflows/curseforge.yml`. That workflow builds the addon zip and
   uploads it to CurseForge with the BigWigs packager. The zip holds only the
   `GnomishRelay` folder, with the shared transport files copied in: never a key or the
   desktop app's addon files.

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
2. Commit, then tag and push:
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

## CurseForge setup

This is done once, and it's done already:

- The CurseForge project ID is 1719624. It's in the `## X-Curse-Project-ID` line of
  `addon/GnomishRelay/GnomishRelay.toc`. A repository variable `CURSEFORGE_PROJECT_ID`
  overrides it.
- An API token from <https://authors.curseforge.com/#/settings/api-tokens> is the
  repository secret `CF_API_KEY`.

Without the ID or the secret, the job still builds the zip, keeps it as an artifact of the
run, and skips the upload. To test the zip locally, run `scripts/package-addon.sh dist`.
