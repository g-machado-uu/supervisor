# Supervisor – Student Tracker

A desktop app for keeping track of supervision work:

- **Project students**, by course. The defaults are:
  - **BEng** (MEC526 + MEC528)
  - **MEng** (EEE840, or EEE802 for the semester-in-Germany route)
  - **SUST** (China, co-supervised)
  - **MSc** Electronics and MSc Mechatronics

  Courses can be added or renamed in Settings, e.g. for an MSc student from another school.
- **PhD students**: project details, the deadlines from the university system (assessments, annual reports, examination) with reminders, the viva and supervision meetings.
- **Placement students**: two visits, with requests, replies and line-manager attendance tracked.
- **Studies advisees**: contact, reply and meetings for each semester, with as many follow-on meetings as you need.

It's written in Rust with [egui](https://github.com/emilk/egui) and runs as a native desktop app on Linux, Windows and macOS (no browser).

## Look and feel

- A calm, academic palette: cream background, powder-blue sidebar, with soft peach, sage, taupe and deep rosewood marking status (upcoming, done, waiting, overdue), and dusky mauve for PhD students.
- **Light and dark** themes built from the same palette. Switch with the sun/moon button at the bottom of the sidebar; the choice is remembered.
- Fonts and icons are bundled in the binary, so the app looks the same on every machine with nothing to install:
  - [Inter](https://rsms.me/inter/) for text
  - [DM Serif Display](https://fonts.google.com/specimen/DM+Serif+Display) for page titles
  - [Phosphor](https://phosphoricons.com) line icons

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
- **Lock** (button at the bottom of the sidebar, or `Ctrl+L`) wipes the key and the decrypted data from memory and returns to the unlock screen.
- The `.ics` calendar export and invite files are **not** encrypted.

## Using it

- **Dashboard**: a greeting with today's date and teaching week, then:
  - a **tile per list** with the number of students and how many meetings or visits fall in the next 7 days (click a tile to open the list)
  - **Upcoming**: meetings, visits and coursework deadlines in the next N days (set in Settings)
  - **Needs attention**, each marked *overdue* or *pending*:
    - no initial contact
    - visit requests or advisee emails with no reply
    - overdue coursework
    - unknown line-manager attendance
    - PhD deadlines whose reminder date has passed, or that are overdue
    - PhD student visas ending within 90 days
  - on wide windows, a side column with **this month's calendar** (dots on busy days; click to open it in the Calendar), **teaching-week progress** and a **coursework ring** (submitted / still to come / overdue)

  Click any item to open that student.
- **Calendar**: meetings, visits and deadlines as a **list** grouped by week or a **month** grid, both labelled with teaching weeks. Switch views with the **List / Month** toggle. Filter by semester (list) or month, and by type with the **Meetings / Visits / Deadlines / Reminders** buttons. Tick items in the list (or a whole week), or click them in the month grid (double-click opens the student), then **Export N selected** to an `.ics`. Selections are kept across months and views.
  - **Repeating meetings as one series**: ticking any week exports the whole series as a repeating event that ends when the series ends, with cancelled weeks left out and moved weeks moved.
  - **Students as guests** (on by default, remembered): adds you as organiser and the students (and line managers for visits) as guests, to send from Outlook. The export message says if any student has no email.
- **Meetings** can be one-off or **repeat** every 1–4 weeks, until the end of teaching for that semester or a date you choose. Under **Weeks**, each week can be marked held, cancelled or moved, and has its own notes. Each meeting is either **Teams** (online) or has a **room / location** (face-to-face).
- **Lists**: one per category, with search (name, B number, email, course or company), initials avatars and status chips. Click a row to open the student in the side panel, which shows their roles and a section for each one. A student can hold several roles, e.g. an FYP student who is also your advisee.
- **PhD students** (sidebar → *Research → PhD*):
  - **Details**: title, type (PhD, MPhil, professional doctorate, MRes), status, full or part time, chair of supervisors and the rest of the team, fee status (National / Overseas), UK visa type with start/end dates and CAS number, funder (DfE QR, DfE, VCRS, Marie Curie, Erasmus, self-funded, or another you type) and funding source.
  - **Project dates**: start and end. For a full-time student with no end date, **36 months from start** fills it in.
  - **Assessments & deadlines**: initial, confirmation and final assessment, one annual report per year (**+ Annual report** adds the next one a year after the last), appoint examiners, intention to submit, earliest submission and thesis submission. Type each deadline from the university system and tick it when it's done.
  - **Reminders** are worked out from each deadline, as in the university system: 3 months before the initial assessment, 4 before confirmation, 7 before the final assessment, and 1 month before the annual report, examiners, intention to submit, submission and amendments. They appear in the calendar, the .ics export and the dashboard. Deadlines and reminders stop once the status is *Awarded* or *Withdrawn*.
  - **Viva & award**: viva date, time and room (with an invite file), outcome, amendments deadline (shown when the outcome needs corrections) and award date.
  - **Supervision meetings** work like project meetings, including repeats.
- **Settings → Academic year**: week 1 of each semester, and when **teaching ends** (Friday of week 12 unless you set it). Repeating meetings stop there, not at the end of exams.
- **Settings → Courses**: name, short name and the sidebar list each course appears in. Courses that still have students can't be removed.
- **Settings → Modules & coursework**:
  - Every module and coursework is editable. Week-based deadlines (e.g. MEC526 Week 8 / Week 12) are worked out from the semester start dates you enter. Give a coursework a fixed date to override that.
  - The SUST modules are separate entries, so their courseworks and dates can be filled in later without affecting the BEng cohort.
  - Per-student extensions are set in the student's coursework grid.
- **Settings → About**: the app version (also shown under the name in the sidebar) and the font and icon licences.
- **Export calendar (.ics)** on the Dashboard or in Settings exports everything. Re-importing updates events rather than duplicating them.
- **Invite files**: each meeting and visit has a **Invite file…** button that saves an `.ics` with you as organiser and the student as attendee (plus the line manager for visits, unless they're marked as not attending). For a repeating meeting it's the whole series.
  - Set your name, email and an optional reusable Teams link in **Settings → Invitations**. They're stored in the encrypted data file.
  - In Outlook on the web: **Add calendar → Upload from file**, then open the event and **Send**. For a meeting ticked **Teams** with no saved link, turn on **Teams meeting** in Outlook before sending.
  - Rooms are included; meeting notes never are. The invite file itself is not encrypted and contains the email addresses.

## Building

You need Rust 1.88 or newer (edition 2024).

```sh
cargo build --release          # binary: target/release/supervisor(.exe)
cargo test                     # unit tests + headless end-to-end UI test
SUPERVISOR_SHOTS=/some/dir cargo test end_to_end   # also write UI screenshots
```

- **Linux**: works on Wayland and X11. Debian/Ubuntu need `libxkbcommon-dev libwayland-dev` to build.
- **Windows**: see [Windows](#windows) below. The release build doesn't open a console window.
- **macOS**: run `cargo build --release` on a Mac.
- **CI**: `.github/workflows/build.yml` builds all three platforms when a `v*` tag is pushed (or when run manually).

## Installing on Linux

```sh
./install.sh                         # build and install for the current user
./install.sh --path=~/Documents/     # keep the data file at ~/Documents/data.svt
./install.sh --path=~/sync/work.svt  # ...or at a specific file
./install.sh --uninstall             # remove binary, launcher and icon (data is kept)
```

This installs `~/.local/bin/supervisor`, a launcher in `~/.local/share/applications/` (shows up in Rofi `drun`, with its icon) and the icon in `~/.local/share/icons/hicolor/`. At the end it prints where the encrypted data file is and, if `~/.local/bin` isn't on your `PATH`, how to add it for bash, zsh and fish. `--path` sets the file the app opens by default (from the launcher or the terminal); you can still switch files from the unlock screen.

## Windows

### Building

You need Rust (from [rustup.rs](https://rustup.rs)) and the Visual Studio Build Tools with the **Desktop development with C++** workload, which rustup offers to install. Then:

```powershell
cargo build --release
```

This builds `target\release\supervisor.exe` with the app icon embedded; `build.rs` uses `rc.exe` from the Windows SDK, which comes with the Build Tools. Nothing is installed system-wide, so copy the exe somewhere permanent, e.g. `%LOCALAPPDATA%\Programs\Supervisor\supervisor.exe`. Then right-click it → *Show more options* → *Pin to Start*, or *Send to → Desktop (create shortcut)*. To update, build again and copy the new exe over the old one.

The data file lives in `%APPDATA%\Supervisor\data\data.svt`, not next to the exe, so moving or replacing the exe doesn't touch your data.

### Running on another PC

Only `supervisor.exe` is needed. Rust and the Build Tools are not.

- **Visual C++ Redistributable.** The exe needs the Microsoft Visual C++ runtime (v14, the "2015–2022" / "2017–2026" package). Most PCs already have it. If the app fails with *"VCRUNTIME140.dll was not found"*, install the x64 version from [aka.ms/vc14/vc_redist.x64.exe](https://aka.ms/vc14/vc_redist.x64.exe) (see Microsoft's [latest supported downloads](https://learn.microsoft.com/cpp/windows/latest-supported-vc-redist) page).
- **SmartScreen.** The exe isn't signed, so Windows may show *"Windows protected your PC"*. Click *More info* → *Run anyway*. Sending it inside a `.zip` helps it get past email filters.
- **Graphics.** The UI draws with OpenGL, which normal graphics drivers provide. It may not start in some virtual machines or Remote Desktop sessions.
- **Separate data.** Each user starts with an empty data file and sets their own password. No data travels with the exe.

## Code layout

| File | Purpose |
| --- | --- |
| `src/model.rs` | Data types, default modules, deadline maths, dashboard queries |
| `src/storage.rs` | Encrypted file format, atomic save, password change |
| `src/ics.rs` | iCalendar export |
| `src/app.rs` | Lock/unlock state, navigation, autosave, dialogs, end-to-end UI test |
| `src/ui/theme.rs` | Palette, fonts, light/dark visuals and shared widgets (cards, chips, sidebar items, toggles, progress ring) |
| `src/ui/icons.rs` | Phosphor icon codepoints |
| `src/ui/*` | Screens (dashboard, calendar, lists, detail sections such as `phd.rs`, settings, unlock screen) |
| `assets/fonts/` | Bundled fonts with their licences (Inter is subset; see `README.txt` there) |
| `packaging/` | App icon (SVG, PNG, ICO) and the Linux launcher |
