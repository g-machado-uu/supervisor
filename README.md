# Supervisor

A desktop app for keeping track of the students you supervise: final-year and MSc projects, PhD students, placement visits and studies advisees. It runs on Linux, Windows and macOS, and keeps everything in one encrypted file on your computer.

![Dashboard](docs/screenshots/dashboard.png)

## What it does

- **Dashboard**: what's coming up, and what needs chasing (no reply, overdue coursework, PhD deadlines, visas about to end).
- **Lists** for each group of students. Click a student to open their details on the right.
- **Meetings**, one-off or repeating, online (Teams) or in a room. Mark each week as held, cancelled or moved.
- **Coursework deadlines** are worked out from the teaching weeks you set in Settings.
- **PhD students**: assessments, annual reports, submission and viva. Type in the deadlines from the university system and the app reminds you in time.
- **Placement visits**: requests, replies and whether the line manager is coming.
- **Calendar** as a list or a month grid. Export any items to an `.ics` file for Outlook or Google Calendar, or save an invite file for one meeting to send to the student.

![Student details](docs/screenshots/student.png)

## Your data

- Everything is in **one encrypted file**, protected by your password. It saves automatically and keeps the previous version as a `.bak`.
- **There is no password recovery.** Remember the password and keep a backup of the file.
- Default location:
  - Linux: `~/.local/share/supervisor/data.svt`
  - Windows: `%APPDATA%\Supervisor\data\data.svt`
  - macOS: `~/Library/Application Support/Supervisor/data.svt`

  You can open or create a file somewhere else (e.g. a synced folder) from the unlock screen.
- **Lock** (bottom of the sidebar, or `Ctrl+L`) clears your data from memory.
- Exported `.ics` files are **not** encrypted.

## Getting started

1. Set a password on first launch.
2. In **Settings**, enter the academic year and the date week 1 starts in each semester.
3. Add students with **Add student** (`Ctrl+N`) and fill in the rest from their details panel.

## Install

You need [Rust](https://rustup.rs) 1.88 or newer on every platform.

### Linux

On Debian/Ubuntu, first install `libxkbcommon-dev libwayland-dev`. Then:

```sh
./install.sh                       # build and install for your user
./install.sh --path=~/Documents/   # ...and keep the data file in ~/Documents
./install.sh --uninstall           # remove the app (your data is kept)
```

This adds `supervisor` to `~/.local/bin` and puts it in your app launcher with its icon.

### Windows

1. Install Rust from [rustup.rs](https://rustup.rs). When it asks, install the Visual Studio Build Tools with **Desktop development with C++**.
2. Build:

   ```powershell
   cargo build --release
   ```

3. Copy `target\release\supervisor.exe` somewhere permanent, e.g. `%LOCALAPPDATA%\Programs\Supervisor\`. Right-click it to pin it to Start or make a desktop shortcut.

To update, build again and replace the exe. Your data is stored separately, so it isn't affected.

To run it on another PC, only the exe is needed. If Windows warns *"Windows protected your PC"*, click *More info* → *Run anyway*. If it says *VCRUNTIME140.dll was not found*, install the [Visual C++ Redistributable](https://aka.ms/vc14/vc_redist.x64.exe).

### macOS

1. Install the developer tools with `xcode-select --install`, and Rust from [rustup.rs](https://rustup.rs).
2. Build and install:

   ```sh
   cargo build --release
   sudo cp target/release/supervisor /usr/local/bin/
   ```

3. Run `supervisor` from the Terminal.

## Development

```sh
cargo test                                         # unit tests + headless UI test
SUPERVISOR_SHOTS=/some/dir cargo test end_to_end   # also save screenshots
```

Pushing a `v*` tag builds Linux, Windows and macOS binaries on GitHub Actions.
