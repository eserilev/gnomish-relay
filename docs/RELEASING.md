# Releasing Gnomish Relay

For maintainers only. A release needs push access to `main` and the repository secrets.

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

## CurseForge setup

This is done once, and it's done already:

- The CurseForge project ID is 1719624. It's in the `## X-Curse-Project-ID` line of
  `addon/GnomishRelay/GnomishRelay.toc`. A repository variable `CURSEFORGE_PROJECT_ID`
  overrides it.
- An API token from <https://authors.curseforge.com/#/settings/api-tokens> is the
  repository secret `CF_API_KEY`.

Without the ID or the secret, the job still builds the zip, keeps it as an artifact of the
run, and skips the upload. To test the zip locally, run `scripts/package-addon.sh dist`.
