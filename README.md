# Supervisor – Student Tracker

A desktop app for keeping track of supervision work:

- **Final Year Project students**, in three groups:
  - **BEng** (MEC526 + MEC528)
  - **MEng** (EEE840, or EEE802 for the semester-in-Germany route)
  - **SUST** (China, co-supervised)
- **Placement students**: two visits, with requests, replies and line-manager attendance tracked.
- **Studies advisees**: contact, reply and meetings for each semester, with as many follow-on meetings as you need.

It's written in Rust with [egui](https://github.com/emilk/egui) and runs as a native desktop app on Linux, Windows and macOS (no browser).

## Data & security

- Everything is stored in **one encrypted file**:
  - The key is derived from your password with Argon2id (64 MiB, 3 passes).
  - The data is sealed with XChaCha20-Poly1305, so tampering or a wrong password is detected.
- Default location:
  - Linux: `~/.local/share/supervisor/data.svt`
  - Windows: `%APPDATA%\Supervisor\data\data.svt`
  - macOS: `~/Library/Application Support/Supervisor/data.svt`

  You can create or open a file elsewhere (e.g. a synced folder) from the unlock screen, or pass a path on the command line: `supervisor /path/to/file.svt`.
- Changes **autosave** about a second after you stop typing, and again on exit. Each save is written atomically, and the previous version is kept as `data.svt.bak`.
- **There is no password recovery.** Keep a backup of the file and remember the password.
- **Lock** (button or `Ctrl+L`) wipes the key and the decrypted data from memory and returns to the unlock screen.
- The `.ics` calendar export is **not** encrypted.

## Using it

- **Dashboard**: what's coming up in the next N days (meetings, visits, coursework deadlines), plus a "Needs attention" list:
  - no initial contact
  - visit requests or advisee emails with no reply
  - overdue coursework
  - unknown line-manager attendance

  Click any item to open that student.
- **Lists**: one per category, with search (name, B number, email, course or company) and status chips. Click a row to edit it in the side panel. A student can hold several roles, e.g. an FYP student who is also your advisee.
- **Settings → Modules & coursework**:
  - Every module and coursework is editable. Week-based deadlines (e.g. MEC526 Week 8 / Week 12) are worked out from the semester start dates you enter. Give a coursework a fixed date to override that.
  - The SUST modules are separate entries, so their courseworks and dates can be filled in later without affecting the BEng cohort.
  - Per-student extensions are set in the student's coursework grid.
- **Export calendar (.ics)** on the Dashboard or in Settings. Re-importing updates events rather than duplicating them.

## Building

You need Rust 1.88 or newer (edition 2024).

```sh
cargo build --release          # binary: target/release/supervisor(.exe)
cargo test                     # unit tests + headless end-to-end UI test
SUPERVISOR_SHOTS=/some/dir cargo test end_to_end   # also write UI screenshots
```

- **Linux**: works on Wayland and X11. Debian/Ubuntu need `libxkbcommon-dev libwayland-dev` to build. On Linux you can install `packaging/supervisor.desktop` to `~/.local/share/applications/` and copy the binary into your `PATH`.
- **Windows / macOS**: run `cargo build --release` on that OS. The release build on Windows doesn't open a console window.
- **CI**: `.github/workflows/build.yml` builds all three platforms when a `v*` tag is pushed (or when run manually).

## Code layout

| File | Purpose |
| --- | --- |
| `src/model.rs` | Data types, default modules, deadline maths, dashboard queries |
| `src/storage.rs` | Encrypted file format, atomic save, password change |
| `src/ics.rs` | iCalendar export |
| `src/app.rs` | Lock/unlock state, navigation, autosave, dialogs, end-to-end UI test |
| `src/ui/*` | Screens and widgets (dashboard, lists, detail sections, settings, theme) |
