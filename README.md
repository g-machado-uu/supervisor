# Supervisor – Student Tracker

A desktop app for keeping track of supervision work:

- **Project students**, by course. The defaults are:
  - **BEng** (MEC526 + MEC528)
  - **MEng** (EEE840, or EEE802 for the semester-in-Germany route)
  - **SUST** (China, co-supervised)
  - **MSc** Electronics and MSc Mechatronics

  Courses can be added or renamed in Settings, e.g. for an MSc student from another school.
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
- The `.ics` calendar export and invite files are **not** encrypted.

## Using it

- **Dashboard**: what's coming up in the next N days (meetings, visits, coursework deadlines), plus a "Needs attention" list:
  - no initial contact
  - visit requests or advisee emails with no reply
  - overdue coursework
  - unknown line-manager attendance

  Click any item to open that student.
- **Calendar**: meetings, visits and deadlines as a **list** grouped by week or a **month** grid, both labelled with teaching weeks. Filter by semester (list) or month, and by type. Tick items in the list (or a whole week), or click them in the month grid (double-click opens the student), then **Export selected** to an `.ics`. Selections are kept across months and views.
  - **Repeating meetings as one series**: ticking any week exports the whole series as a repeating event that ends when the series ends, with cancelled weeks left out and moved weeks moved.
  - **Students as guests** (on by default, remembered): adds you as organiser and the students (and line managers for visits) as guests, to send from Outlook. The export message says if any student has no email.
- **Meetings** can be one-off or **🔁 repeat** every 1–4 weeks, until the end of teaching for that semester or a date you choose. Under **Weeks**, each week can be marked held, cancelled or moved, and has its own notes. Each meeting is either **💻 Teams** or has a **📍 room / location**.
- **Lists**: one per category, with search (name, B number, email, course or company) and status chips. Click a row to edit it in the side panel. A student can hold several roles, e.g. an FYP student who is also your advisee.
- **Settings → Academic year**: week 1 of each semester, and when **teaching ends** (Friday of week 12 unless you set it). Repeating meetings stop there, not at the end of exams.
- **Settings → Courses**: name, short name and the sidebar list each course appears in. Courses that still have students can't be removed.
- **Settings → Modules & coursework**:
  - Every module and coursework is editable. Week-based deadlines (e.g. MEC526 Week 8 / Week 12) are worked out from the semester start dates you enter. Give a coursework a fixed date to override that.
  - The SUST modules are separate entries, so their courseworks and dates can be filled in later without affecting the BEng cohort.
  - Per-student extensions are set in the student's coursework grid.
- **Export calendar (.ics)** on the Dashboard or in Settings exports everything. Re-importing updates events rather than duplicating them.
- **Invite files**: each meeting and visit has a **📨 Invite file…** button that saves an `.ics` with you as organiser and the student as attendee (plus the line manager for visits, unless they're marked as not attending). For a repeating meeting it's the whole series.
  - Set your name, email and an optional reusable Teams link in **Settings → Invitations**. They're stored in the encrypted data file.
  - In Outlook on the web: **Add calendar → Upload from file**, then open the event and **Send**. For a meeting ticked **💻 Teams** with no saved link, turn on **Teams meeting** in Outlook before sending.
  - Rooms are included; meeting notes never are. The invite file itself is not encrypted and contains the email addresses.

## Building

You need Rust 1.88 or newer (edition 2024).

```sh
cargo build --release          # binary: target/release/supervisor(.exe)
cargo test                     # unit tests + headless end-to-end UI test
SUPERVISOR_SHOTS=/some/dir cargo test end_to_end   # also write UI screenshots
```

- **Linux**: works on Wayland and X11. Debian/Ubuntu need `libxkbcommon-dev libwayland-dev` to build.
- **Windows / macOS**: run `cargo build --release` on that OS. The release build on Windows doesn't open a console window.
- **CI**: `.github/workflows/build.yml` builds all three platforms when a `v*` tag is pushed (or when run manually).

## Installing on Linux

```sh
./install.sh                         # build and install for the current user
./install.sh --path=~/Documents/     # keep the data file at ~/Documents/data.svt
./install.sh --path=~/sync/work.svt  # ...or at a specific file
./install.sh --uninstall             # remove binary, launcher and icon (data is kept)
```

This installs `~/.local/bin/supervisor`, a launcher in `~/.local/share/applications/` (shows up in Rofi `drun`, with its icon) and the icon in `~/.local/share/icons/hicolor/`. At the end it prints where the encrypted data file is and, if `~/.local/bin` isn't on your `PATH`, how to add it for bash, zsh and fish. `--path` sets the file the app opens by default (from the launcher or the terminal); you can still switch files from the unlock screen.

## Code layout

| File | Purpose |
| --- | --- |
| `src/model.rs` | Data types, default modules, deadline maths, dashboard queries |
| `src/storage.rs` | Encrypted file format, atomic save, password change |
| `src/ics.rs` | iCalendar export |
| `src/app.rs` | Lock/unlock state, navigation, autosave, dialogs, end-to-end UI test |
| `src/ui/*` | Screens and widgets (dashboard, lists, detail sections, settings, theme) |
