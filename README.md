# Codex HUD

A small Windows app that keeps your Codex usage close at hand. Click its icon in the system tray to see how much of your allowance is left and where your tokens are going.

## Features

- **Remaining limits** — See your 5-hour and weekly allowance, with a countdown to each reset.
- **Account details** — Check which account and subscription you’re using.
- **Token usage** — View input and output tokens, cache hit rate, and usage by model.
- **Activity charts** — Explore today, the last 7 or 30 days, or your full local history.
- **Quick access** — Open the panel from the system tray. It stays above other windows and hides when you click away.

## Before you start

You’ll need Windows and Codex installed on your computer. Sign in to Codex with your ChatGPT account to view subscription limits. API key sign-in doesn’t provide those limits.

Token usage comes from the Codex session history saved on your computer. Use Codex first so there’s some activity to show.

## How to use

1. Launch Codex HUD. It starts in the system tray, near the clock on your taskbar.
2. Click the tray icon to open the panel. If you don’t see it, check the tray’s hidden icons.
3. Open **Limits** to see your remaining allowance and reset times. The bars and percentages show how much you have **left**.
4. Open **Analysis** to explore your token usage. Pick a time range or click the refresh button for an update.
5. Click outside the panel to hide it. To exit, right-click the tray icon and choose **Quit**.

The panel checks for updates when you open it and every minute while it’s visible.

## About your usage data

Subscription limits come from your signed-in Codex account and need an internet connection to refresh. If a refresh fails, the panel may keep showing the last successful reading with an error message.

Token charts use session history stored on this computer, including archived sessions. **Lifetime** means all available local history; activity from other devices won’t appear unless its session files are also present here. Dates and times follow your computer’s local time.

By default, Codex HUD reads history from `%USERPROFILE%\.codex`. If you use a custom Codex folder, start the app with your usual `CODEX_HOME` environment variable set.

## Troubleshooting

- **No window appears:** Click the Codex HUD icon in the system tray. The panel starts hidden.
- **Codex isn’t found:** Make sure Codex is installed. The app looks for `codex.exe` on your PATH and in common Codex installation folders.
- **Limits aren’t available:** Check that you’re signed in to Codex with your ChatGPT account. If the app asks you to update Codex, install a newer version.
- **A refresh fails:** Check your internet connection and Codex sign-in, then reopen the panel.
- **Charts are empty:** Try a longer time range or start a Codex session, then refresh. If you use a custom data folder, check your `CODEX_HOME` setting.
- **Some sessions couldn’t be read:** The charts may be missing usage from those files.

## Build from source

If you’d like to build the app yourself, you’ll need Node.js and npm, Rust, Visual Studio Build Tools with the C++ desktop workload and Windows SDK, and the WebView2 Runtime. The current frontend requires Node.js 20.19+ (20.x) or 22.12+.

Run these commands in the project folder:

```powershell
npm install
npm run build
```

The app will be available at `src-tauri\target\debug\codex-hud.exe`.

To run it while making changes:

```powershell
npm run tauri dev
```

Built with Tauri, React, TypeScript, and Rust.

## Releases

Pushing a version tag triggers the release workflow. It builds an unsigned Windows x64 executable and publishes it directly to GitHub Releases as `CodexHUD-vX.Y.Z-win-x64.exe`. The executable uses the system WebView2 Runtime; Codex must also be installed as described above.

To publish a release:

1. Update the version in `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json` to the same value. Run `npm install --package-lock-only` and `cargo check --manifest-path src-tauri/Cargo.toml` to update the lockfiles.
2. Add release notes to `CHANGELOG.md` under a heading such as `## 0.2.0`. If no matching section exists, CI uses a short default message.
3. Commit the version changes, lockfiles, and notes with a message such as `build: bump version to v0.2.0`.
4. Create an annotated tag on that commit and push the commit and tag:

```powershell
git tag -a v0.2.0 -m "v0.2.0"
git push --follow-tags
```

CI rejects tags that do not match the project versions. Tags such as `v0.2.0-beta.1` publish a prerelease. Only the executable is attached to the Release; GitHub displays its SHA256 digest automatically.
