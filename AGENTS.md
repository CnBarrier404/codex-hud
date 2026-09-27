# AGENTS.md

Codex HUD is a Tauri 2 windows desktop app with a React 19/TypeScript frontend and Rust backend for checking Codex usage limits, token consumption, and other relevant data.

## Commands

| Action                 | Command                                                     |
| :--------------------- | :---------------------------------------------------------- |
| Build                  | `npm run tauri build -- --debug --no-bundle`                |
| Check the Rust backend | `cargo check --manifest-path src-tauri/Cargo.toml`          |
| Check Rust formatting  | `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` |

## Repository Structure

| Directory                 | Responsibility                                                                                      |
| :------------------------ | :-------------------------------------------------------------------------------------------------- |
| `src/`                    | React entry point (`main.tsx`), HUD component (`App.tsx`), and styles (`App.css`).                  |
| `src-tauri/`              | Rust crate and desktop configuration, including window and packaging settings in `tauri.conf.json`. |
| `src-tauri/src/`          | Rust startup (`main.rs`) and tray, window, and appearance behavior (`lib.rs`).                      |
| `src-tauri/capabilities/` | Tauri permission definitions.                                                                       |
| `src-tauri/icons/`        | Application and tray icon assets.                                                                   |
| `dist/`                   | Generated frontend build output.                                                                    |
