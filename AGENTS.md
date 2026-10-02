# AGENTS.md

Codex HUD is a Tauri 2 windows desktop app with a React 19/TypeScript frontend and Rust backend for checking Codex usage limits, token consumption, and other relevant data.

## Commands

| Action                 | Command                                                     |
| :--------------------- | :---------------------------------------------------------- |
| Build                  | `npm run tauri build -- --debug --no-bundle`                |
| Check the Rust backend | `cargo check --manifest-path src-tauri/Cargo.toml`          |
| Check Rust formatting  | `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` |

## Version Updates and Releases

Release versions are updated locally and committed before tagging. CI validates the versions; it does not change them or create a version bump commit.

For a release such as `0.2.0`:

1. Set the version to `0.2.0` in `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`.
2. Run `npm install --package-lock-only` and `cargo check --manifest-path src-tauri/Cargo.toml` to update `package-lock.json` and `src-tauri/Cargo.lock`. Include both lockfiles in the version update when they change.
3. Create or update `CHANGELOG.md` with release notes under `## 0.2.0`. CI uses the matching version section for the Release description, or a short default message if none exists.
4. Validate the release metadata locally with `$env:RELEASE_TAG = 'v0.2.0'; node scripts/prepare-release.mjs`. Leave the Rust formatting check and release executable build to CI; do not build the release locally for a version update unless requested.
5. Create a version bump commit with a message such as `build: bump version to v0.2.0`. The commit must include the version updates, changed lockfiles, and release notes.
6. When tagging and publishing are authorized, create an annotated tag on that commit and push the commit and tag:

   ```powershell
   git tag -a v0.2.0 -m "v0.2.0"
   git push --follow-tags
   ```

Pushing the tag triggers `.github/workflows/release.yml`; pushing only the commit does not. The tag version must match all three project versions and the root version entries in `package-lock.json`.

The workflow publishes a GitHub Release with one unsigned Windows x64 executable, named `CodexHUD-v0.2.0-win-x64.exe`. It does not attach an installer, SHA256 file, or signature file. The executable requires the system WebView2 Runtime and an installed Codex.

For a prerelease, use a version such as `0.2.0-beta.1` in all version files and a matching tag such as `v0.2.0-beta.1`. CI marks it as a prerelease automatically.

## Repository Structure

| Directory                 | Responsibility                                                                                      |
| :------------------------ | :-------------------------------------------------------------------------------------------------- |
| `src/`                    | React entry point (`main.tsx`), HUD component (`App.tsx`), and styles (`App.css`).                  |
| `src-tauri/`              | Rust crate and desktop configuration, including window and packaging settings in `tauri.conf.json`. |
| `src-tauri/src/`          | Rust startup (`main.rs`) and tray, window, and appearance behavior (`lib.rs`).                      |
| `src-tauri/capabilities/` | Tauri permission definitions.                                                                       |
| `src-tauri/icons/`        | Application and tray icon assets.                                                                   |
| `dist/`                   | Generated frontend build output.                                                                    |
