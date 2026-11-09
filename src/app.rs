//! Application shell: lock state, navigation, autosave and dialogs.

use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use chrono::NaiveDate;
use egui::{Id, RichText, Ui};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::ics;
use crate::model::{
    AdviseeRecord, Attention, CalItem, Database, Event, FypRecord, MarkingKind, MarkingRecord, MeetingChange, PlacementRecord,
    ResearchGroup, Student,
};
use crate::storage::{self, KeyFile, Vault, VaultError, Written};
use crate::ui::calendar::{self, CalAction, CalendarState};
use crate::ui::dashboard::{self, DashAction};
use crate::ui::lock::{LockScreen, Unlocked};
use crate::ui::settings::{self, KeyFileForm, PasswordForm, SettingsAction};
use crate::ui::student_form::{self, FormAction, Role, Shown};
use crate::ui::student_import::{self, StudentImport};
use crate::ui::student_list::{self, Category, ListAction, ListState};
use crate::ui::timetable::{self, TimetableDialog};
use crate::ui::file_dialog::{self, Dialog};
use crate::ui::{icons, theme, widgets};

const AUTOSAVE_DELAY: f64 = 1.0;

pub struct SupervisorApp {
    screen: Screen,
}

enum Screen {
    Locked(LockScreen),
    Unlocked(Box<Session>),
}

impl SupervisorApp {
    /// `data_file` overrides the last-used data file (e.g. from the command line).
    pub fn new(cc: &eframe::CreationContext<'_>, data_file: Option<std::path::PathBuf>) -> Self {
        theme::install(&cc.egui_ctx);
        // The saved preference is encrypted; follow the OS until unlocked.
        cc.egui_ctx.set_theme(egui::ThemePreference::System);
        Self {
            screen: Screen::Locked(LockScreen::new(data_file.unwrap_or_else(storage::last_used_path))),
        }
    }
}

impl SupervisorApp {
    /// Drop the session, which wipes the key, and egui's copies of what was
    /// on screen, such as the undo history every text field keeps.
    fn lock(&mut self, ctx: &egui::Context) {
        let Screen::Unlocked(s) = &self.screen else { return };
        let path = s.vault.path().to_path_buf();
        self.screen = Screen::Locked(LockScreen::new(path));
        ctx.data_mut(|d| d.clear());
        ctx.forget_all_images();
    }
}

impl eframe::App for SupervisorApp {
    // Also runs while the window is hidden, so idle locking works then too.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let Screen::Unlocked(s) = &mut self.screen else { return };
        let now = ctx.input(|i| i.time);
        s.poll_save(now, false);
        if ctx.input(|i| i.viewport().close_requested()) {
            if s.discard {
                s.leaving = Some(Leave::Quit);
            } else {
                s.detect_changes(now);
                s.leave(ctx, Leave::Quit);
            }
            if s.ready_to_leave().is_none() {
                // Not saved yet: stay open, and close once saved or say why not.
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
        }
        s.check_idle(ctx);
        if s.ready_to_leave() == Some(Leave::Lock) {
            self.lock(ctx);
        }
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        match &mut self.screen {
            Screen::Locked(lock) => {
                if let Some(unlocked) = lock.ui(ui) {
                    self.screen = Screen::Unlocked(Box::new(Session::new(unlocked)));
                }
            }
            Screen::Unlocked(session) => match session.ui(ui) {
                Some(Leave::Lock) => self.lock(ui.ctx()),
                Some(Leave::Quit) => ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close),
                None => {}
            },
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
enum View {
    Dashboard,
    Calendar,
    List(Category),
    Settings,
}

enum SaveStatus {
    Saved,
    Pending,
    Error(String),
    /// The file was written elsewhere since it was unlocked; saving waits for the user.
    Conflict,
}

/// Lock or quit, once nothing is left unsaved.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Leave {
    Lock,
    Quit,
}

/// Adding a key file: it's already in place on this computer; the data file
/// is re-keyed only after a backup copy has been saved.
struct KeyFileSetup {
    key: KeyFile,
    path: PathBuf,
    password: Zeroizing<String>,
    backup: Option<PathBuf>,
    /// The dialog asking where to save the backup copy, while it is open.
    choosing: Option<file_dialog::Pending>,
    error: Option<String>,
}

/// What to do with the path from the open file dialog.
enum OnPick {
    /// Write this calendar there, then say `done`.
    SaveIcs { text: String, done: String },
    ImportIcs,
    ImportSheet(crate::sheet_import::SheetKind, Category),
    BackupFolder,
}

enum Confirm {
    DeleteStudent(Uuid),
    RemoveRole(Uuid, Role),
}

/// What a password prompt is about to show.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reveal {
    Number,
    RarNotes,
}

/// Asks for the password before showing something sensitive of a student.
struct RevealPrompt {
    what: Reveal,
    student: Uuid,
    password: Zeroizing<String>,
    error: Option<String>,
}

struct NewStudent {
    student: Student,
    fyp: bool,
    programme: String,
    phd: bool,
    research: ResearchGroup,
    placement: bool,
    company: String,
    professional_email: String,
    advisee: bool,
    /// I mark their work without supervising them.
    marking: Option<MarkingKind>,
    /// For marking a project: `Programme::id`.
    marking_programme: String,
}

impl NewStudent {
    fn for_category(cat: Option<&Category>, settings: &crate::model::Settings) -> Self {
        let first = settings
            .programmes
            .first()
            .map(|p| p.id.clone())
            .unwrap_or_default();
        let mut n = Self {
            student: Student::default(),
            fyp: false,
            programme: first,
            phd: false,
            research: ResearchGroup::Phd,
            placement: false,
            company: String::new(),
            professional_email: String::new(),
            advisee: false,
            marking: None,
            marking_programme: settings
                .programmes
                .iter()
                .find(|p| !settings.is_sust(&p.id))
                .map(|p| p.id.clone())
                .unwrap_or_default(),
        };
        match cat {
            Some(Category::Projects(list)) => {
                n.fyp = true;
                if let Some(p) = settings.programmes.iter().find(|p| p.list_name() == list) {
                    n.programme = p.id.clone();
                }
            }
            Some(Category::Research(group)) => {
                n.phd = true;
                n.research = *group;
            }
            Some(Category::Placement) => n.placement = true,
            Some(Category::Advisee) => n.advisee = true,
            Some(Category::Marking(kind)) => n.marking = Some(*kind),
            _ => {}
        }
        n
    }

    fn build(self) -> Student {
        let mut s = self.student;
        if self.fyp {
            s.fyp = Some(FypRecord {
                programme: self.programme,
                ..Default::default()
            });
        }
        if self.phd {
            s.phd = Some(self.research.new_record());
        }
        if self.placement {
            s.placement = Some(PlacementRecord {
                company: self.company,
                professional_email: self.professional_email,
                ..Default::default()
            });
        }
        if self.advisee {
            s.advisee = Some(AdviseeRecord::default());
        }
        if let Some(kind) = self.marking {
            let programme = if kind == MarkingKind::Project { self.marking_programme } else { String::new() };
            s.marking = Some(MarkingRecord { kind, programme, ..Default::default() });
        }
        s
    }
}

/// A student's RAR notes, open after the password was confirmed.
struct RarDialog {
    student: Uuid,
    /// The cursor has been put in the text.
    focused: bool,
}

/// Lists the Dashboard and Calendar draw on every frame, rebuilt only when
/// the data or the date changes.
pub(crate) struct Derived {
    print: [u8; 32],
    today: NaiveDate,
    pub events: Rc<[Event]>,
    pub attention: Rc<[Attention]>,
}

impl Derived {
    /// The lists for `db`, whose fingerprint is `print`, from `cache` if
    /// that is still current.
    pub(crate) fn of<'c>(cache: &'c mut Option<Self>, db: &Database, print: [u8; 32], today: NaiveDate) -> &'c Self {
        let stale = cache.as_ref().is_none_or(|d| d.print != print || d.today != today);
        if stale {
            *cache = Some(Self {
                print,
                today,
                events: db.events().into(),
                attention: db.attention(today).into(),
            });
        }
        cache.as_ref().expect("just filled")
    }
}

struct Session {
    vault: Vault,
    db: Database,
    view: View,
    selected: Option<Uuid>,
    search: String,
    show_archived: bool,
    /// Students ticked in a list to copy their emails, and that list.
    /// Leaving the list unticks them.
    ticked: (Option<Category>, HashSet<Uuid>),
    dirty_since: Option<f64>,
    status: SaveStatus,
    new_student: Option<NewStudent>,
    /// Students from a spreadsheet, while they are being chosen.
    student_import: Option<(StudentImport, crate::sheet_import::SheetKind)>,
    /// A file dialog while it is open, and what to do with its answer.
    file_dialog: Option<(file_dialog::Pending, OnPick)>,
    /// The student whose hidden registration number is shown.
    revealed: Option<Uuid>,
    reveal_prompt: Option<RevealPrompt>,
    rar_notes: Option<RarDialog>,
    /// Privacy mode, switched on the dashboard: names and numbers are
    /// hidden there, in the lists, the calendar and every student's details.
    private: bool,
    /// The student whose details are shown in privacy mode.
    peek: Option<Uuid>,
    /// Students whose details are hidden outside privacy mode.
    hidden: HashSet<Uuid>,
    /// The student whose coursework marks are shown.
    marks_shown: Option<Uuid>,
    confirm: Option<Confirm>,
    /// A student's timetable, while it is open.
    timetable: Option<TimetableDialog>,
    password_form: PasswordForm,
    key_form: KeyFileForm,
    key_setup: Option<KeyFileSetup>,
    /// How the last thing asked of the backup copy went, for Settings.
    backup_message: Option<(bool, String)>,
    /// A problem with the backup copy has been announced, and not yet solved.
    backup_warned: bool,
    calendar: CalendarState,
    toast: Option<(String, f64)>,
    /// The list of keyboard shortcuts is open.
    shortcuts: bool,
    /// At the end of the last frame, something would take Esc before the
    /// student's details. It must be the last frame's: egui takes the focus
    /// from a field on Esc before this frame's code gets to see it.
    esc_taken: bool,
    applied_dark: Option<bool>,
    /// Fingerprint of `db` when last checked, to spot edits.
    db_print: [u8; 32],
    /// Events and attention items for `db_print` and today.
    derived: Option<Derived>,
    /// A save running in the background, and the `db_print` it started from.
    saving: Option<(Receiver<anyhow::Result<Written>>, [u8; 32])>,
    /// Save was clicked: save without waiting for a pause in typing, then
    /// make what is saved the restore point.
    save_requested: bool,
    last_active: Instant,
    leaving: Option<Leave>,
    /// Lock or quit even though the latest changes aren't saved.
    discard: bool,
    weak_password: bool,
    /// Shown once the session is on screen.
    notice: Option<String>,
}

impl Session {
    fn new(unlocked: Unlocked) -> Self {
        let Unlocked { vault, db, weak_password } = unlocked;
        Self {
            db_print: storage::fingerprint(&db),
            derived: None,
            saving: None,
            save_requested: false,
            vault,
            db,
            view: View::Dashboard,
            selected: None,
            search: String::new(),
            show_archived: false,
            ticked: (None, HashSet::new()),
            dirty_since: None,
            status: SaveStatus::Saved,
            new_student: None,
            student_import: None,
            file_dialog: None,
            revealed: None,
            reveal_prompt: None,
            rar_notes: None,
            private: false,
            peek: None,
            hidden: HashSet::new(),
            marks_shown: None,
            confirm: None,
            timetable: None,
            password_form: PasswordForm::default(),
            key_form: KeyFileForm::default(),
            key_setup: None,
            backup_message: None,
            backup_warned: false,
            calendar: CalendarState::default(),
            toast: None,
            shortcuts: false,
            esc_taken: false,
            applied_dark: None,
            last_active: Instant::now(),
            leaving: None,
            discard: false,
            weak_password,
            notice: weak_password.then(|| {
                "Your password is easier to guess than Supervisor now allows – please change it in Settings › Security."
                    .to_string()
            }),
        }
    }

    /// Start saving in the background, unless a save is already running.
    /// Serialising and encrypting happen here; only ciphertext goes to the
    /// worker, which writes it to disk. `poll_save` picks up the result.
    fn save(&mut self, ctx: &egui::Context) {
        if self.saving.is_some() {
            return;
        }
        let job = self.vault.seal(&self.db).map(|bytes| (self.vault.writer(), bytes));
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(job.and_then(|(writer, bytes)| writer.write(&bytes)));
            ctx.request_repaint();
        });
        self.saving = Some((rx, self.db_print));
    }

    /// Take the result of the background save if it has finished, or with
    /// `wait`, once it does.
    fn poll_save(&mut self, now: f64, wait: bool) {
        let Some((rx, started_from)) = &self.saving else { return };
        let received = if wait {
            rx.recv().map_err(|_| TryRecvError::Disconnected)
        } else {
            rx.try_recv()
        };
        let result = match received {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err(anyhow::anyhow!("saving stopped unexpectedly")),
        };
        let started_from = *started_from;
        self.saving = None;
        match result {
            Ok(written) => {
                self.vault.saved(written);
                // Edits made while it was saving stay pending for the next save.
                if self.db_print == started_from {
                    self.saved(Ok(()));
                    if self.save_requested {
                        self.mark_restore_point(now);
                    }
                }
            }
            Err(e) => {
                self.saved(Err(e));
                match self.status {
                    // Back off before retrying.
                    SaveStatus::Error(_) => self.dirty_since = Some(now + 4.0),
                    // The conflict dialog takes over.
                    SaveStatus::Conflict => self.save_requested = false,
                    _ => {}
                }
            }
        }
    }

    /// Save straight away (the Save button or Ctrl+S), and make what is saved
    /// the restore point: the next save keeps it as the `.bak`.
    fn save_now(&mut self, ctx: &egui::Context, now: f64) {
        if matches!(self.status, SaveStatus::Conflict) {
            return;
        }
        if self.dirty_since.is_none() && self.saving.is_none() {
            self.mark_restore_point(now);
        } else {
            self.save_requested = true;
            if self.dirty_since.is_some() {
                self.save(ctx);
            }
        }
    }

    fn mark_restore_point(&mut self, now: f64) {
        self.save_requested = false;
        self.vault.mark_restore_point();
        self.toast = Some(("Saved".into(), now));
    }

    /// Wait for a background save, before writing the file from this thread.
    fn wait_for_save(&mut self, ui: &Ui) {
        self.poll_save(ui.input(|i| i.time), true);
    }

    fn saved(&mut self, result: anyhow::Result<()>) {
        match result {
            Ok(()) => {
                self.status = SaveStatus::Saved;
                self.dirty_since = None;
            }
            // Written elsewhere: ask before replacing it.
            Err(e) if storage::vault_error(&e) == Some(VaultError::ChangedOnDisk) => {
                self.status = SaveStatus::Conflict;
            }
            Err(e) => {
                // Keep it dirty so the next autosave retries.
                self.status = SaveStatus::Error(format!("{e:#}"));
            }
        }
    }

    /// Mark the data unsaved if anything changed since the last check.
    fn detect_changes(&mut self, now: f64) {
        let print = storage::fingerprint(&self.db);
        if print != self.db_print {
            self.db_print = print;
            self.dirty_since = Some(now);
            if !matches!(self.status, SaveStatus::Conflict) {
                self.status = SaveStatus::Pending;
            }
        }
    }

    /// Ask to lock or quit. It happens once nothing is left unsaved; if
    /// saving fails, a dialog says so and offers to go ahead anyway.
    fn leave(&mut self, ctx: &egui::Context, leave: Leave) {
        if self.leaving.is_none() || leave == Leave::Quit {
            self.leaving = Some(leave);
        }
        if self.dirty_since.is_some() && !matches!(self.status, SaveStatus::Conflict) {
            self.save(ctx);
        }
    }

    fn ready_to_leave(&self) -> Option<Leave> {
        self.leaving
            .filter(|_| (self.dirty_since.is_none() || self.discard) && self.saving.is_none())
    }

    /// Lock after the chosen time without input. Uses the wall clock, as
    /// egui's time stands still while the window is hidden.
    fn check_idle(&mut self, ctx: &egui::Context) {
        let minutes = self.db.settings.auto_lock_minutes;
        if minutes == 0 || self.leaving.is_some() {
            return;
        }
        let limit = Duration::from_secs(u64::from(minutes) * 60);
        match limit.checked_sub(self.last_active.elapsed()) {
            Some(left) if !left.is_zero() => ctx.request_repaint_after(left),
            _ => self.leave(ctx, Leave::Lock),
        }
    }

    /// Events and attention items as of the last change check, which runs
    /// on every frame that can edit.
    fn derived(&mut self) -> &Derived {
        Derived::of(&mut self.derived, &self.db, self.db_print, widgets::today())
    }

    fn notify(&mut self, ui: &Ui, msg: impl Into<String>) {
        self.toast = Some((msg.into(), ui.input(|i| i.time)));
    }

    /// Puts the emails of these students on the clipboard, ready to paste
    /// into To, and says who has none.
    fn copy_emails(&mut self, ui: &Ui, ids: &[Uuid]) {
        let students: Vec<&Student> = ids.iter().filter_map(|id| self.db.student(*id)).collect();
        let (emails, missing) = student_list::email_list(&students);
        let copied = students.len() - missing.len();
        let mut msg = match copied {
            0 => "Nothing copied".to_string(),
            1 => "Copied 1 email".to_string(),
            n => format!("Copied {n} emails"),
        };
        if copied > 0 {
            ui.ctx().copy_text(emails);
            msg.push_str(" – paste them into To");
        }
        if !missing.is_empty() {
            let mut names: Vec<String> =
                missing.iter().take(3).map(|s| if s.name.trim().is_empty() { "(no name)".into() } else { s.name.trim().to_string() }).collect();
            if missing.len() > 3 {
                names.push(format!("{} more", missing.len() - 3));
            }
            msg.push_str(&match (missing.len(), self.private) {
                (1, true) => ". 1 student has no email".to_string(),
                (n, true) => format!(". {n} students have no email"),
                (_, false) => format!(". No email for {}", names.join(", ")),
            });
        }
        self.notify(ui, msg);
    }

    /// Says once when the backup copy stops working, and when it works again.
    fn announce_backup_problem(&mut self, ui: &Ui) {
        let problem = self.vault.backup_copy().and_then(|c| c.problem.clone());
        if problem.is_some() != self.backup_warned {
            self.backup_warned = problem.is_some();
            self.notify(ui, match problem {
                Some(problem) => format!("The backup copy couldn't be saved. {problem}"),
                None => "The backup copy is saving again".to_string(),
            });
        }
    }

    /// Draws the unlocked app. Returns what to do once nothing is left unsaved.
    fn ui(&mut self, ui: &mut Ui) -> Option<Leave> {
        let now = ui.input(|i| i.time);
        let mut lock = false;
        let mut save_now = false;
        if ui.input(|i| !i.events.is_empty()) {
            self.last_active = Instant::now();
        }
        if let Some(msg) = self.notice.take() {
            self.notify(ui, msg);
        }
        self.announce_backup_problem(ui);

        if ui.input_mut(|i| i.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::L))) {
            lock = true;
        }
        if ui.input_mut(|i| i.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::S))) {
            save_now = true;
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F1)) {
            self.shortcuts = !self.shortcuts;
        }

        // A revealed registration number, marks, or details in privacy mode
        // are hidden again once another student is opened.
        for shown in [&mut self.revealed, &mut self.marks_shown, &mut self.peek] {
            if shown.is_some() && *shown != self.selected {
                *shown = None;
            }
        }
        // Esc closes the student's details, once nothing else takes it. The
        // calendar's selection goes first, like a dialog or a field's cursor.
        let esc = ui.input(|i| i.key_pressed(egui::Key::Escape) && i.modifiers.is_none());
        let calendar_selection = self.view == View::Calendar && !self.calendar.selected.is_empty();
        if esc && !self.esc_taken && !calendar_selection && self.view != View::Settings {
            self.selected = None;
        }
        self.sidebar(ui, &mut lock, &mut save_now);
        if self.view != View::Settings && self.selected.is_some() {
            self.detail_panel(ui);
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::central_panel(ui.style())
                    .inner_margin(egui::Margin { left: 28, right: 28, top: 22, bottom: 12 }),
            )
            .show(ui, |ui| self.central(ui));
        self.toast(ui);
        self.shortcuts(ui);
        self.timetable_dialog(ui);
        self.student_import_dialog(ui);
        self.file_dialog(ui);
        self.dialogs(ui);
        let ctx = ui.ctx();
        // Anything floating over the app takes it (a date picker, a menu, a
        // dialog), except a message that is only passing by.
        let toast = Id::unique("toast");
        self.esc_taken = ctx.memory(|m| {
            m.focused().is_some()
                || m.areas().visible_layer_ids().iter().any(|l| l.order == egui::Order::Foreground && l.id != toast)
        });

        // Every edit needs a click, key press or drag; frames from just moving
        // the mouse or a blinking cursor skip the (whole-database) check.
        let may_edit = ui.input(|i| {
            i.pointer.any_down()
                || i.events.iter().any(|e| {
                    !matches!(e, egui::Event::PointerMoved(_) | egui::Event::MouseMoved(_))
                })
        });
        if may_edit {
            self.detect_changes(now);
        }
        if self.applied_dark != Some(self.db.settings.dark_mode) {
            self.applied_dark = Some(self.db.settings.dark_mode);
            theme::apply(ui.ctx(), self.db.settings.dark_mode);
        }
        if lock {
            self.leave(ui.ctx(), Leave::Lock);
        } else if save_now {
            self.save_now(ui.ctx(), now);
        }
        if let Some(since) = self.dirty_since
            && !matches!(self.status, SaveStatus::Conflict)
            && self.saving.is_none()
        {
            // Locking, quitting and the Save button don't wait for a pause in
            // typing, only for the back-off after a failed save.
            let wait = match self.status {
                SaveStatus::Pending if self.leaving.is_some() || self.save_requested => 0.0,
                _ => AUTOSAVE_DELAY - (now - since),
            };
            if wait <= 0.0 {
                self.save(ui.ctx());
            } else {
                ui.ctx().request_repaint_after(Duration::from_secs_f64(wait));
            }
        }
        self.ready_to_leave()
    }

    /// The latest message, floating at the bottom of the window for a few seconds.
    fn toast(&mut self, ui: &mut Ui) {
        let now = ui.input(|i| i.time);
        let Some((msg, at)) = &self.toast else { return };
        if now - at >= 5.0 {
            self.toast = None;
            return;
        }
        ui.ctx().request_repaint_after(Duration::from_millis(500));
        egui::Area::new(Id::unique("toast"))
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(115.0, -24.0))
            .order(egui::Order::Foreground)
            .interactable(false)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style())
                    .corner_radius(egui::CornerRadius::same(12))
                    .inner_margin(egui::Margin::symmetric(16, 10))
                    .show(ui, |ui| {
                        ui.set_max_width(560.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(icons::BELL).size(16.0).color(theme::fg(ui, theme::ACCENT)));
                            ui.label(msg);
                        });
                    });
            });
    }

    /// The list of keyboard shortcuts, from the keyboard button next to
    /// Settings or F1.
    fn shortcuts(&mut self, ui: &mut Ui) {
        if !self.shortcuts {
            return;
        }
        let ctx = ui.ctx().clone();
        let key = |modifiers, key| ctx.format_shortcut(&egui::KeyboardShortcut::new(modifiers, key));
        let groups = [
            (
                "Anywhere",
                vec![
                    (key(egui::Modifiers::COMMAND, egui::Key::S), "Save now"),
                    (key(egui::Modifiers::COMMAND, egui::Key::L), "Lock"),
                    ("Esc".to_string(), "Close a dialog"),
                    ("F1".to_string(), "Show this list"),
                ],
            ),
            (
                "Students",
                vec![
                    (key(egui::Modifiers::COMMAND, egui::Key::N), "Add a student"),
                    ("Esc".to_string(), "Leave a field, then close the student"),
                ],
            ),
            (
                "Calendar",
                vec![
                    ("Click".to_string(), "Select an item"),
                    ("Double-click".to_string(), "Open the student"),
                    ("Drag".to_string(), "Move an item to another day (month view)"),
                    ("Right-click".to_string(), "Edit, postpone or delete an item"),
                    ("Delete".to_string(), "Delete the selected items"),
                    ("Esc".to_string(), "Clear the selection"),
                ],
            ),
        ];
        let mut close = false;
        let resp = egui::Modal::new(Id::unique("shortcuts")).frame(theme::modal_frame(ui)).show(&ctx, |ui| {
            ui.set_width(440.0);
            ui.horizontal(|ui| {
                theme::icon_badge(ui, icons::KEYBOARD, theme::ACCENT, 40.0);
                ui.label(theme::serif("Keyboard shortcuts", 22.0));
            });
            for (title, rows) in groups {
                ui.add_space(10.0);
                ui.label(RichText::new(title.to_uppercase()).small().strong().color(theme::muted(ui)));
                egui::Grid::new(title).num_columns(2).spacing([16.0, 6.0]).min_col_width(110.0).show(ui, |ui| {
                    for (keys, what) in rows {
                        ui.label(RichText::new(keys).monospace());
                        ui.label(what);
                        ui.end_row();
                    }
                });
            }
            ui.add_space(12.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(theme::primary_button("Close")).clicked() {
                    close = true;
                }
            });
        });
        if close || resp.should_close() {
            self.shortcuts = false;
        }
    }

    fn sidebar(&mut self, ui: &mut Ui, lock: &mut bool, save_now: &mut bool) {
        let frame = egui::Frame::new()
            .fill(theme::sidebar_fill(ui))
            .inner_margin(egui::Margin { left: 14, right: 14, top: 18, bottom: 14 });
        egui::Panel::left("nav")
            .exact_size(232.0)
            .frame(frame)
            .show_separator_line(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(38.0, 38.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, egui::CornerRadius::same(12), theme::ACCENT);
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        icons::GRADUATION_CAP,
                        egui::FontId::proportional(21.0),
                        egui::Color32::WHITE,
                    );
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.label(theme::serif("Supervisor", 21.0));
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&self.db.settings.academic_year).small().color(theme::muted(ui)));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(RichText::new(theme::VERSION).small().color(theme::muted(ui)));
                            });
                        });
                    });
                });
                ui.add_space(18.0);

                ui.spacing_mut().item_spacing.y = 3.0;
                let section = |ui: &mut Ui, text: &str| {
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        ui.label(
                            theme::semibold(text)
                                .size(10.5)
                                .extra_letter_spacing(1.0)
                                .color(theme::muted(ui)),
                        );
                    });
                    ui.add_space(2.0);
                };
                let lists = self.db.settings.programme_lists();
                let mut nav = |ui: &mut Ui, view: View, icon: &str, label: &str, count: Option<usize>| {
                    if theme::nav_item(ui, icon, label, count, self.view == view).clicked() {
                        self.view = view;
                    }
                };
                nav(ui, View::Dashboard, icons::HOUSE, "Dashboard", None);
                nav(ui, View::Calendar, icons::CALENDAR_DOTS, "Calendar", None);
                // The lists scroll when the window is too short for them, so the
                // buttons at the bottom always show.
                let lists_height = (ui.available_height() - 96.0).max(120.0);
                egui::ScrollArea::vertical()
                    .max_height(lists_height)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        section(ui, "PROJECT STUDENTS");
                        for list in lists {
                            let cat = Category::Projects(list.clone());
                            let count = cat.count(&self.db);
                            nav(ui, View::List(cat), icons::BOOK_OPEN_TEXT, &list, Some(count));
                        }
                        section(ui, "RESEARCH");
                        for group in ResearchGroup::ALL {
                            let cat = Category::Research(group);
                            let count = cat.count(&self.db);
                            nav(ui, View::List(cat), icons::GRADUATION_CAP, group.label(), Some(count));
                        }
                        section(ui, "OTHER");
                        nav(ui, View::List(Category::Placement), icons::BUILDINGS, "Placement", Some(Category::Placement.count(&self.db)));
                        nav(ui, View::List(Category::Advisee), icons::CHAT_CIRCLE_TEXT, "Studies Advisees", Some(Category::Advisee.count(&self.db)));
                        section(ui, "MARKING");
                        for kind in MarkingKind::ALL {
                            let cat = Category::Marking(kind);
                            let count = cat.count(&self.db);
                            nav(ui, View::List(cat), icons::EXAM, kind.label(), Some(count));
                        }
                        ui.add_space(10.0);
                        nav(ui, View::List(Category::All), icons::USERS_THREE, "All Students", Some(Category::All.count(&self.db)));
                    });

                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    ui.horizontal(|ui| {
                        if ui
                            .add(egui::Button::new(format!("{}  Lock", icons::LOCK_SIMPLE)).corner_radius(egui::CornerRadius::same(10)))
                            .on_hover_text("Lock (Ctrl+L)")
                            .clicked()
                        {
                            *lock = true;
                        }
                        let dark = &mut self.db.settings.dark_mode;
                        if ui
                            .add(egui::Button::new(RichText::new(if *dark { icons::SUN } else { icons::MOON }).size(16.0)).corner_radius(egui::CornerRadius::same(10)))
                            .on_hover_text("Toggle light/dark")
                            .clicked()
                        {
                            *dark = !*dark;
                        }
                        // The save status doubles as the Save button.
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let (text, color, hover) = match &self.status {
                                SaveStatus::Conflict => {
                                    ui.label(RichText::new(format!("{} Not saved", icons::WARNING)).small().color(theme::fg(ui, theme::RED)))
                                        .on_hover_text("The data file was changed somewhere else");
                                    return;
                                }
                                // Saved, but not to the backup copy. The row has no room for more.
                                SaveStatus::Saved if let Some(problem) = self.vault.backup_copy().and_then(|c| c.problem.as_deref()) => (
                                    format!("{} Saved", icons::WARNING),
                                    theme::fg(ui, theme::AMBER),
                                    format!("Your data is saved, but the backup copy couldn't be. {problem}\nSee Settings › Backup copy."),
                                ),
                                SaveStatus::Saved => (
                                    format!("{} Saved", icons::FLOPPY_DISK),
                                    theme::fg(ui, theme::GREEN),
                                    "All changes are saved and encrypted.\nClick to save now and keep this version as the backup (Ctrl+S).".to_string(),
                                ),
                                SaveStatus::Pending => ("Saving…".to_string(), theme::muted(ui), "Click to save now (Ctrl+S)".to_string()),
                                SaveStatus::Error(e) => (
                                    format!("{} Save failed", icons::WARNING),
                                    theme::fg(ui, theme::RED),
                                    format!("{e}\nClick to try again."),
                                ),
                            };
                            let button = egui::Button::new(RichText::new(text).small().color(color))
                                .frame_when_inactive(false)
                                .corner_radius(egui::CornerRadius::same(10));
                            if ui.add(button).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(hover).clicked() {
                                *save_now = true;
                            }
                        });
                    });
                    ui.add_space(4.0);
                    // Only as tall as the row: in this bottom-up layout it would
                    // otherwise take the free space above and centre in it.
                    let row = egui::vec2(ui.available_width(), 36.0);
                    ui.allocate_ui_with_layout(row, egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let keys = egui::Button::new(RichText::new(icons::KEYBOARD).size(17.0))
                            .frame_when_inactive(false)
                            .corner_radius(egui::CornerRadius::same(10))
                            .min_size(egui::vec2(36.0, 36.0));
                        if ui.add(keys).on_hover_text("Keyboard shortcuts (F1)").clicked() {
                            self.shortcuts = true;
                        }
                        nav(ui, View::Settings, icons::GEAR_SIX, "Settings", None);
                    });
                });
            });
    }

    fn detail_panel(&mut self, ui: &mut Ui) {
        let Some(id) = self.selected else { return };
        let dup = self
            .db
            .student(id)
            .is_some_and(|s| self.db.b_number_taken(&s.b_number, Some(id)));
        let mut shown = Shown {
            details: if self.private { self.peek == Some(id) } else { !self.hidden.contains(&id) },
            marks: self.marks_shown == Some(id),
            number: self.revealed == Some(id),
        };
        let before = shown;
        let Database { settings, students, .. } = &mut self.db;
        let Some(student) = students.iter_mut().find(|s| s.id == id) else {
            self.selected = None;
            return;
        };
        let mut action = FormAction::None;
        let frame = egui::Frame::side_top_panel(ui.style())
            .fill(ui.visuals().window_fill)
            .inner_margin(egui::Margin { left: 20, right: 14, top: 20, bottom: 8 });
        egui::Panel::right("detail")
            .resizable(true)
            .default_size(500.0)
            .min_size(420.0)
            .frame(frame)
            .show(ui, |ui| {
                action = student_form::show(ui, student, settings, dup, &mut shown);
            });
        if shown.details != before.details {
            if self.private {
                self.peek = shown.details.then_some(id);
            } else if shown.details {
                self.hidden.remove(&id);
            } else {
                self.hidden.insert(id);
            }
            if !shown.details {
                self.marks_shown = None;
                self.revealed = None;
            }
        }
        if shown.marks != before.marks {
            self.marks_shown = shown.marks.then_some(id);
        }
        match action {
            FormAction::None => {}
            FormAction::Close => self.selected = None,
            FormAction::Delete => self.confirm = Some(Confirm::DeleteStudent(id)),
            FormAction::RemoveRole(role) => self.confirm = Some(Confirm::RemoveRole(id, role)),
            FormAction::Share(widgets::Share::File(uid)) => self.save_invite(ui, id, &uid),
            FormAction::Share(widgets::Share::Google(uid)) => self.open_in_google(ui, id, &uid),
            FormAction::Reveal if self.db.settings.reveal_needs_password => {
                self.reveal_prompt = Some(RevealPrompt {
                    what: Reveal::Number,
                    student: id,
                    password: Zeroizing::new(String::new()),
                    error: None,
                });
            }
            FormAction::Reveal => self.revealed = Some(id),
            FormAction::Hide => self.revealed = None,
            FormAction::RarNotes => {
                self.reveal_prompt = Some(RevealPrompt {
                    what: Reveal::RarNotes,
                    student: id,
                    password: Zeroizing::new(String::new()),
                    error: None,
                });
            }
            FormAction::Timetable => self.timetable = Some(TimetableDialog::new(id, &self.db.settings, widgets::today())),
        }
    }

    fn central(&mut self, ui: &mut Ui) {
        match self.view.clone() {
            View::Dashboard => {
                let d = self.derived();
                let (events, attention) = (Rc::clone(&d.events), Rc::clone(&d.attention));
                match dashboard::show(ui, &self.db, &events, &attention, self.private) {
                    DashAction::None => {}
                    DashAction::Privacy => {
                        self.private = !self.private;
                        self.peek = None;
                        self.marks_shown = None;
                        self.revealed = None;
                    }
                    DashAction::Open(id) => self.selected = Some(id),
                    DashAction::Goto(cat) => self.view = View::List(cat),
                    DashAction::ExportIcs => self.export_ics(ui),
                    DashAction::Month(month) => {
                        self.calendar.layout = calendar::Layout::Month;
                        self.calendar.month = month;
                        self.view = View::Calendar;
                    }
                }
            }
            View::Calendar => {
                let events = Rc::clone(&self.derived().events);
                let mut guests = self.db.settings.export_with_guests;
                self.calendar.private = self.private;
                let action = calendar::show(ui, &self.db, &events, &mut self.calendar, &mut guests);
                self.db.settings.export_with_guests = guests;
                match action {
                    CalAction::None => {}
                    CalAction::Open(id) => self.selected = Some(id),
                    CalAction::Export(uids) => self.export_selected(ui, &uids),
                    CalAction::Change(event, change) => {
                        if self.db.change_meeting(&event, change) {
                            let done = match change {
                                MeetingChange::Held(true) => "Marked as held".to_string(),
                                MeetingChange::Held(false) => "Marked as not held".to_string(),
                                MeetingChange::Postpone(date, _) => format!("Moved to {}", date.format("%a %d %b")),
                                MeetingChange::Cancelled(true) => "Week cancelled".to_string(),
                                MeetingChange::Cancelled(false) => "Week restored".to_string(),
                            };
                            self.notify(ui, done);
                        }
                    }
                    CalAction::Move(event, date) => {
                        if self.db.move_event(&event, date) {
                            let mut msg = format!("Moved to {}", date.format("%a %d %b"));
                            if let Some(block) = self.db.clash(event.student_id, date, event.time) {
                                msg.push_str(&format!(" – but it clashes with the student's timetable: busy {}", block.label()));
                            }
                            self.notify(ui, msg);
                        }
                    }
                    CalAction::Edit(event, details) => {
                        if self.db.edit_event(&event, &details) {
                            self.notify(ui, "Saved");
                        }
                    }
                    CalAction::Delete(events, whole_series) => {
                        let done = events.iter().filter(|e| self.db.delete_event(e, whole_series)).count();
                        let msg = match (done, events.first()) {
                            (1, Some(e)) if e.series.is_some() && !whole_series => "Week cancelled – right-click it to restore".to_string(),
                            (1, _) => "Deleted".to_string(),
                            (n, _) => format!("Deleted {n} items"),
                        };
                        self.notify(ui, msg);
                    }
                    CalAction::Google(event) => {
                        let uid = event.series.as_ref().map_or(event.uid.clone(), |(series, _)| series.clone());
                        self.open_in_google(ui, event.student_id, &uid);
                    }
                    CalAction::PickImport => self.pick_import(ui),
                    CalAction::Import(choices) => {
                        let n = self.db.import(choices);
                        self.notify(ui, match n {
                            0 => "Nothing imported".to_string(),
                            1 => "Imported 1 event".to_string(),
                            n => format!("Imported {n} events"),
                        });
                    }
                }
            }
            View::List(cat) => {
                if self.ticked.0.as_ref() != Some(&cat) {
                    self.ticked = (Some(cat.clone()), HashSet::new());
                }
                let action = student_list::show(
                    ui,
                    &self.db,
                    &cat,
                    ListState {
                        search: &mut self.search,
                        show_archived: &mut self.show_archived,
                        selected: &mut self.selected,
                        ticked: &mut self.ticked.1,
                        private: self.private,
                    },
                );
                if let ListAction::CopyEmails(ids) = &action {
                    self.copy_emails(ui, ids);
                }
                if action == ListAction::Import
                    && let Some(kind) = cat.import_kind()
                {
                    self.pick_sheet(ui, kind, &cat);
                }
                if action == ListAction::Add
                    || ui.input_mut(|i| {
                        i.consume_shortcut(&egui::KeyboardShortcut::new(
                            egui::Modifiers::COMMAND,
                            egui::Key::N,
                        ))
                    })
                {
                    self.new_student =
                        Some(NewStudent::for_category(Some(&cat), &self.db.settings));
                }
            }
            View::Settings => {
                let path = self.vault.path().display().to_string();
                let mut in_use = std::collections::HashMap::new();
                for f in self.db.students.iter().filter_map(|s| s.fyp.as_ref()) {
                    *in_use.entry(f.programme.clone()).or_insert(0) += 1;
                }
                let security = settings::Security {
                    data_path: &path,
                    key_file_on: self.vault.uses_key_file(),
                    weak_password: self.weak_password,
                    password: &mut self.password_form,
                    key_file: &mut self.key_form,
                    backup_copy: self.vault.backup_copy(),
                    backup_message: &self.backup_message,
                };
                match settings::show(ui, &mut self.db.settings, &in_use, security) {
                    SettingsAction::None => {}
                    SettingsAction::ExportIcs => self.export_ics(ui),
                    SettingsAction::ChangePassword => self.change_password(ui),
                    SettingsAction::AddKeyFile => self.start_key_file(),
                    SettingsAction::RemoveKeyFile => self.remove_key_file(ui),
                    SettingsAction::ChooseBackupFolder => self.open_file_dialog(
                        ui,
                        Dialog::new().set_title("Choose a folder for the backup copy").pick_folder(),
                        OnPick::BackupFolder,
                    ),
                    SettingsAction::BackupNow => self.backup_now(ui),
                    SettingsAction::StopBackup => self.stop_backup(ui),
                }
            }
        }
    }

    /// Ask where to save an .ics of `items`; once saved, say `done`.
    fn save_ics(&mut self, ui: &Ui, items: &[CalItem], invite: bool, file_name: &str, done: String) {
        let text = ics::calendar(
            items,
            &self.db.settings,
            invite,
            &jiff::tz::TimeZone::system(),
        );
        let dialog = Dialog::new().add_filter("iCalendar", &["ics"]).set_file_name(file_name).save_file();
        self.open_file_dialog(ui, dialog, OnPick::SaveIcs { text, done });
    }

    /// Open a file dialog without blocking the window; `file_dialog` acts
    /// on its answer. Only one is open at a time.
    fn open_file_dialog(
        &mut self,
        ui: &Ui,
        dialog: impl Future<Output = Option<rfd::FileHandle>> + Send + 'static,
        then: OnPick,
    ) {
        if self.file_dialog.is_none() {
            self.file_dialog = Some((file_dialog::Pending::open(ui.ctx(), dialog), then));
        }
    }

    /// Act on the file dialog's answer once it has closed.
    fn file_dialog(&mut self, ui: &Ui) {
        let Some(answer) = self.file_dialog.as_ref().and_then(|(d, _)| d.poll()) else { return };
        let Some((_, then)) = self.file_dialog.take() else { return };
        let Some(path) = answer else { return };
        match then {
            OnPick::SaveIcs { text, done } => match std::fs::write(&path, text) {
                Ok(()) => self.notify(ui, done),
                Err(e) => self.notify(ui, format!("Saving {} failed: {e}", path.display())),
            },
            OnPick::ImportIcs => self.import_ics(ui, &path),
            OnPick::ImportSheet(kind, cat) => self.import_sheet(ui, &path, kind, &cat),
            OnPick::BackupFolder => self.set_backup_folder(ui, &path),
        }
    }

    /// Invites need your address as the organiser; send to Settings if missing.
    fn require_email(&mut self, ui: &Ui) -> bool {
        if ics::valid_email(&self.db.settings.my_email) {
            return true;
        }
        self.view = View::Settings;
        self.notify(ui, "Add your email in Settings › Invitations first");
        false
    }

    fn export_ics(&mut self, ui: &Ui) {
        let items = self.db.cal_items(|_| true, true);
        let done = format!("Exported {} items", items.len());
        self.save_ics(ui, &items, false, "supervisor.ics", done);
    }

    fn export_selected(&mut self, ui: &Ui, uids: &std::collections::HashSet<String>) {
        let invite = self.db.settings.export_with_guests;
        if invite && !self.require_email(ui) {
            return;
        }
        let items = self
            .db
            .cal_items(|e| uids.contains(&e.uid), self.calendar.as_series);
        let name = if invite {
            "supervisor-invites.ics"
        } else {
            "supervisor-selection.ics"
        };
        {
            let mut msg = format!("Exported {} items", items.len());
            if invite {
                // Deadlines and reminders never have guests; anything else without one lacks an email.
                let missing = items
                    .iter()
                    .filter(|i| {
                        !matches!(i.event.kind, crate::model::EventKind::Deadline | crate::model::EventKind::Reminder)
                            && i.attendees.is_empty()
                    })
                    .count();
                if missing > 0 {
                    msg.push_str(&format!(" – {missing} without a student email, add it to invite them"));
                } else {
                    msg.push_str(" – upload it in Outlook, then open each event and Send");
                }
            }
            self.save_ics(ui, &items, invite, name, msg);
        }
    }

    fn save_invite(&mut self, ui: &Ui, student_id: Uuid, uid: &str) {
        if !self.require_email(ui) {
            return;
        }
        let Some(invite) = self.db.invite(student_id, uid) else {
            return;
        };
        if invite.attendees.is_empty() {
            self.notify(ui, "This student has no email address yet");
            return;
        }
        let name: String = format!(
            "{} {}",
            invite.event.date.format("%Y-%m-%d"),
            invite.event.title
        )
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " -–_().".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
        let series = if invite.series.is_some() {
            "repeating "
        } else {
            ""
        };
        let no_link = invite.event.online && ics::meeting_link(&invite.event, &self.db.settings).is_none();
        let hint = if no_link && invite.event.platform == crate::model::Platform::GoogleMeet {
            "upload it in Outlook, open the event, add the Google Meet link and Send"
        } else if no_link && invite.event.platform == crate::model::Platform::Zoom {
            "upload it in Outlook, open the event, add the Zoom link and Send"
        } else if no_link {
            "upload it in Outlook, open the event, turn on Teams meeting and Send"
        } else {
            "upload it in Outlook, open the event and Send"
        };
        self.save_ics(ui, &[invite], true, &format!("{name}.ics"), format!("{series}Invite saved – {hint}"));
    }

    /// Open Google Calendar's "new event" page filled in with the meeting,
    /// visit or viva `uid` (a whole series if it repeats).
    fn open_in_google(&mut self, ui: &Ui, student_id: Uuid, uid: &str) {
        let Some(item) = self.db.invite(student_id, uid) else {
            return;
        };
        let url = ics::google_calendar_url(&item, &self.db.settings, &jiff::tz::TimeZone::system());
        ui.ctx().open_url(egui::OpenUrl::new_tab(url));
        let mut msg = "Opened in Google Calendar – check it and Save to send the invitation".to_string();
        if item.series.as_ref().is_some_and(|r| !r.exceptions.is_empty()) {
            msg.push_str("; cancelled or moved weeks need changing there");
        }
        self.notify(ui, msg);
    }

    /// Ask for an .ics file and show its events for importing.
    fn pick_import(&mut self, ui: &Ui) {
        let dialog = Dialog::new().add_filter("iCalendar", &["ics"]).pick_file();
        self.open_file_dialog(ui, dialog, OnPick::ImportIcs);
    }

    /// Show the events of the .ics file at `path` for importing.
    fn import_ics(&mut self, ui: &Ui, path: &std::path::Path) {
        let text = match std::fs::read(path) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(e) => {
                self.notify(ui, format!("Reading {} failed: {e}", path.display()));
                return;
            }
        };
        let events = crate::ics_import::parse(&text, &jiff::tz::TimeZone::system());
        if events.is_empty() {
            self.notify(ui, "No events found in that file");
            return;
        }
        let file = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        self.calendar.import = Some(calendar::ImportDialog::new(file, events, &self.db));
    }

    /// Ask for a spreadsheet and show its students for importing into `cat`.
    fn pick_sheet(&mut self, ui: &Ui, kind: crate::sheet_import::SheetKind, cat: &Category) {
        let dialog = Dialog::new()
            .set_title("Import students from a spreadsheet")
            .add_filter("Spreadsheet", &["xlsx", "xlsm", "xls", "xlsb", "ods"])
            .pick_file();
        self.open_file_dialog(ui, dialog, OnPick::ImportSheet(kind, cat.clone()));
    }

    /// Show the students in the spreadsheet at `path` for importing into `cat`.
    fn import_sheet(&mut self, ui: &Ui, path: &std::path::Path, kind: crate::sheet_import::SheetKind, cat: &Category) {
        let sheet = crate::sheet_import::read_workbook(path).and_then(|sheets| crate::sheet_import::import(&sheets));
        let sheet = match sheet {
            Ok(sheet) if sheet.students.is_empty() => {
                self.notify(ui, "No students found in the first sheet of that file");
                return;
            }
            Ok(sheet) => sheet,
            Err(e) => {
                self.notify(ui, e);
                return;
            }
        };
        let file = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        let list = match cat {
            Category::Projects(list) => Some(list.as_str()),
            _ => None,
        };
        self.student_import = Some((StudentImport::new(kind, file, sheet, &self.db, list), kind));
    }

    /// The students of a spreadsheet, while they are being chosen.
    fn student_import_dialog(&mut self, ui: &mut Ui) {
        let Some((dialog, kind)) = &mut self.student_import else { return };
        let kind = *kind;
        match dialog.show(ui) {
            student_import::Outcome::Open => {}
            student_import::Outcome::Closed => self.student_import = None,
            student_import::Outcome::Add(choices) => {
                self.student_import = None;
                let added = crate::sheet_import::add_students(&mut self.db, kind, choices);
                let students = |n: usize| if n == 1 { "1 student".to_string() } else { format!("{n} students") };
                let mut msg = match (added.new, added.existing) {
                    (0, 0) => String::new(),
                    (n, 0) => format!("Added {}", students(n)),
                    (0, e) => format!("Added {} already in Supervisor to this list", students(e)),
                    (n, e) => format!("Added {}, and {e} already in Supervisor", students(n)),
                };
                if added.updated > 0 {
                    let filled = format!("filled in details of {}", students(added.updated));
                    msg = if msg.is_empty() { format!("Filled in details of {}", students(added.updated)) } else { format!("{msg}; {filled}") };
                }
                if msg.is_empty() {
                    msg = "Nothing added".into();
                }
                self.notify(ui, msg);
            }
        }
    }

    fn change_password(&mut self, ui: &Ui) {
        self.wait_for_save(ui);
        let pw = std::mem::take(&mut self.password_form);
        let result = if let Some(problem) = storage::password_problem(&pw.new) {
            Err(problem)
        } else if *pw.new != *pw.confirm {
            Err("The new passwords do not match.".to_string())
        } else {
            let result = self.vault.change_password(&pw.current, &pw.new, &self.db);
            self.resealed(result, "Could not change password")
        };
        if result.is_ok() {
            self.weak_password = false;
        }
        self.password_form.message = Some(match result {
            Ok(()) => (true, "Password changed.".into()),
            Err(e) => (false, e),
        });
    }

    /// Re-keying saves the data too. Failures are put in plain words.
    fn resealed(&mut self, result: anyhow::Result<()>, failed: &str) -> Result<(), String> {
        let Err(e) = result else {
            self.saved(Ok(()));
            return Ok(());
        };
        Err(match storage::vault_error(&e) {
            Some(VaultError::WrongPassword) => "The current password is incorrect.".into(),
            Some(VaultError::ChangedOnDisk) => {
                self.status = SaveStatus::Conflict;
                format!("{failed}: {e}")
            }
            _ => format!("{failed}: {e:#}"),
        })
    }

    fn set_backup_folder(&mut self, ui: &Ui, folder: &std::path::Path) {
        self.wait_for_save(ui);
        let before = self.vault.backup_copy().map(|c| c.file.clone());
        let result = self.vault.set_backup_folder(Some(folder), &self.db);
        let moved = before.filter(|file| Some(file) != self.vault.backup_copy().map(|c| &c.file));
        self.backup_message = Some(match (result, moved) {
            (Ok(()), Some(old)) => (true, format!("Saved there. The old copy stays at {}; delete it if you don't need it.", old.display())),
            (Ok(()), None) => (true, "Saved there. Every save now goes there too.".into()),
            (Err(e), _) => (false, format!("{e:#}")),
        });
    }

    fn backup_now(&mut self, ui: &Ui) {
        self.wait_for_save(ui);
        // A failure shows with the folder in Settings, and goes once it works.
        self.backup_message = self.vault.backup_now(&self.db).ok().map(|()| (true, "Copy saved.".to_string()));
    }

    fn stop_backup(&mut self, ui: &Ui) {
        self.wait_for_save(ui);
        let file = self.vault.backup_copy().map(|c| c.file.clone());
        self.backup_message = Some(match (self.vault.set_backup_folder(None, &self.db), file) {
            (Ok(()), Some(file)) => (true, format!("Turned off. The copy stays at {}; delete it if you don't need it.", file.display())),
            (Ok(()), None) => (true, "Turned off.".into()),
            (Err(e), _) => (false, format!("{e:#}")),
        });
    }

    /// Adding a key file, step 1: check the password and put the key file in
    /// place on this computer. The data file is re-keyed only after a backup
    /// copy has been saved (see `dialogs`).
    fn start_key_file(&mut self) {
        let form = std::mem::take(&mut self.key_form);
        let result = (|| {
            let path = storage::key_file_path().ok_or("This computer has no folder for settings.")?;
            if storage::same_folder(self.vault.path(), &path) {
                return Err(format!(
                    "Your data file is in the folder where the key file would go ({}). Move the data file first.",
                    path.parent().unwrap_or(&path).display()
                ));
            }
            match self.vault.check_password(&form.password) {
                Ok(true) => {}
                Ok(false) => return Err("The current password is incorrect.".into()),
                Err(e) => return Err(format!("{e:#}")),
            }
            // One key file per computer: reuse it if there already is one.
            let key = match storage::local_key_file() {
                Ok(Some(key)) => key,
                Ok(None) => {
                    let key = KeyFile::generate();
                    storage::install_key_file(&key).map_err(|e| format!("Could not save the key file: {e:#}"))?;
                    key
                }
                Err(e) => return Err(format!("{e:#}")),
            };
            Ok(KeyFileSetup {
                key,
                path,
                password: form.password,
                backup: None,
                choosing: None,
                error: None,
            })
        })();
        match result {
            Ok(setup) => self.key_setup = Some(setup),
            Err(e) => self.key_form.message = Some((false, e)),
        }
    }

    fn finish_key_file(&mut self, ui: &Ui) {
        self.wait_for_save(ui);
        let Some(setup) = self.key_setup.take() else { return };
        let result = self.vault.set_key_file(&setup.password, Some(setup.key), &self.db);
        self.key_form.message = Some(match self.resealed(result, "Could not turn on the key file") {
            Ok(()) => (
                true,
                "Key file turned on. On another computer, Supervisor will ask for your backup copy the first time.".into(),
            ),
            Err(e) => (false, e),
        });
    }

    fn remove_key_file(&mut self, ui: &Ui) {
        self.wait_for_save(ui);
        let form = std::mem::take(&mut self.key_form);
        let result = self.vault.set_key_file(&form.password, None, &self.db);
        self.key_form.message = Some(match self.resealed(result, "Could not remove the key file") {
            Ok(()) => (
                true,
                "Key file removed – your password alone opens this data file again. \
                 The key file stays on this computer in case another data file uses it."
                    .into(),
            ),
            Err(e) => (false, e),
        });
    }

    /// The open student's timetable, if any.
    fn timetable_dialog(&mut self, ui: &mut Ui) {
        let Some(dialog) = &mut self.timetable else { return };
        let id = dialog.student;
        let hidden = if self.private { self.peek != Some(id) } else { self.hidden.contains(&id) };
        let Database { settings, students, .. } = &mut self.db;
        let open = students.iter_mut().find(|s| s.id == id).is_some_and(|student| {
            let name = if hidden { "•".repeat(10) } else { student.display_name() };
            timetable::show(ui, dialog, student, settings, &name)
        });
        if !open {
            self.timetable = None;
        }
    }

    fn dialogs(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let without_saving = if self.leaving == Some(Leave::Quit) {
            "Quit without saving"
        } else {
            "Lock without saving"
        };

        if matches!(self.status, SaveStatus::Conflict) {
            let backup = format!("{}.bak", self.vault.path().file_name().unwrap_or_default().to_string_lossy());
            let mut keep_mine = false;
            let mut discard = false;
            egui::Modal::new(Id::unique("conflict")).frame(theme::modal_frame(ui)).show(&ctx, |ui| {
                ui.set_width(460.0);
                ui.horizontal(|ui| {
                    theme::icon_badge(ui, icons::WARNING, theme::AMBER, 40.0);
                    ui.label(theme::serif("Changed somewhere else", 22.0));
                });
                ui.add_space(4.0);
                ui.label(
                    "Since you unlocked it, your data file was changed by another copy of Supervisor – \
                     probably on another computer that shares the file, or in a second window.",
                );
                ui.add_space(4.0);
                ui.label(format!(
                    "Saving your version replaces that one. It is kept as {backup} in the same folder, so nothing is lost for good."
                ));
                ui.add_space(12.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(theme::primary_button("Save my version")).clicked() {
                        keep_mine = true;
                    }
                    if ui
                        .add(egui::Button::new(without_saving).min_size(egui::vec2(0.0, 34.0)))
                        .on_hover_text("Your changes since the last save are lost. Unlock again to see the other version.")
                        .clicked()
                    {
                        discard = true;
                    }
                });
            });
            if keep_mine {
                self.wait_for_save(ui);
                let result = self.vault.overwrite(&self.db);
                self.saved(result);
            } else if discard {
                self.discard = true;
                self.leaving.get_or_insert(Leave::Lock);
            }
        }

        if let (Some(leave), SaveStatus::Error(err)) = (self.leaving, &self.status)
            && self.dirty_since.is_some()
        {
            let err = err.clone();
            let mut retry = false;
            let mut stay = false;
            let mut discard = false;
            let resp = egui::Modal::new(Id::unique("not-saved")).frame(theme::modal_frame(ui)).show(&ctx, |ui| {
                ui.set_width(460.0);
                ui.horizontal(|ui| {
                    theme::icon_badge(ui, icons::WARNING, theme::RED, 40.0);
                    ui.label(theme::serif("Not saved yet", 22.0));
                });
                ui.add_space(4.0);
                let what = if leave == Leave::Quit { "close" } else { "lock" };
                ui.label(format!("Supervisor couldn't save your latest changes, so it didn't {what}:"));
                ui.label(RichText::new(err).small().color(theme::muted(ui)));
                ui.add_space(4.0);
                ui.label("It keeps trying. If the data file is on a network or USB drive, check that it's connected.");
                ui.add_space(12.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(theme::primary_button("Try again")).clicked() {
                        retry = true;
                    }
                    if ui.add(egui::Button::new("Keep working").min_size(egui::vec2(0.0, 34.0))).clicked() {
                        stay = true;
                    }
                    if ui.add(theme::danger_button(without_saving)).clicked() {
                        discard = true;
                    }
                });
            });
            if retry {
                self.save(&ctx);
            } else if discard {
                self.discard = true;
            } else if stay || resp.should_close() {
                self.leaving = None;
            }
        }

        if let Some(setup) = &mut self.key_setup {
            let mut turn_on = false;
            let mut cancel = false;
            let data_file = self.vault.path().to_path_buf();
            let resp = egui::Modal::new(Id::unique("key-file-setup")).frame(theme::modal_frame(ui)).show(&ctx, |ui| {
                ui.set_width(500.0);
                ui.horizontal(|ui| {
                    theme::icon_badge(ui, icons::KEY, theme::ACCENT, 40.0);
                    ui.label(theme::serif("Back up your key file", 24.0));
                });
                ui.add_space(6.0);
                ui.label("Your key file is saved on this computer at:");
                ui.label(RichText::new(setup.path.display().to_string()).small().color(theme::muted(ui)));
                ui.add_space(6.0);
                ui.label(
                    "Save a backup copy before turning it on. Keep it somewhere safe and away from your data file: \
                     a USB stick, or as an attachment in a password manager such as KeePassXC or Bitwarden. \
                     You'll also need it to use Supervisor on another computer.",
                );
                ui.add_space(4.0);
                ui.colored_label(
                    theme::fg(ui, theme::AMBER),
                    "Without the key file your data can't be opened, even with the right password.",
                );
                ui.add_space(10.0);
                if ui
                    .add_enabled(setup.choosing.is_none(), egui::Button::new(format!("{} Save backup copy…", icons::EXPORT)))
                    .clicked()
                {
                    let dialog = Dialog::new()
                        .set_title("Save a backup copy of your key file")
                        .set_file_name("supervisor-key.bin")
                        .save_file();
                    setup.choosing = Some(file_dialog::Pending::open(ui.ctx(), dialog));
                }
                let chosen = setup.choosing.as_ref().and_then(|d| d.poll());
                if chosen.is_some() {
                    setup.choosing = None;
                }
                if let Some(dest) = chosen.flatten() {
                    let beside_copy = self.vault.backup_copy().is_some_and(|c| storage::same_folder(&dest, &c.file));
                    let saved = if beside_copy {
                        Err(anyhow::anyhow!("Choose another place: a key file next to the backup copy of your data would protect nothing."))
                    } else {
                        storage::save_key_backup(&setup.key, &dest, &data_file)
                    };
                    match saved {
                        Ok(()) => {
                            setup.backup = Some(dest);
                            setup.error = None;
                        }
                        Err(e) => setup.error = Some(format!("{e:#}")),
                    }
                }
                if let Some(dest) = &setup.backup {
                    ui.colored_label(
                        theme::fg(ui, theme::GREEN),
                        format!("{} Backup saved to {}", icons::CHECK_CIRCLE, dest.display()),
                    );
                }
                if let Some(e) = &setup.error {
                    ui.colored_label(theme::fg(ui, theme::RED), e);
                }
                ui.add_space(12.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(setup.backup.is_some(), theme::primary_button("Turn on key file"))
                        .on_disabled_hover_text("Save a backup copy first")
                        .clicked()
                    {
                        turn_on = true;
                    }
                    if ui.add(egui::Button::new("Cancel").min_size(egui::vec2(0.0, 34.0))).clicked() {
                        cancel = true;
                    }
                });
            });
            if turn_on {
                self.finish_key_file(ui);
            } else if cancel || resp.should_close() {
                self.key_setup = None;
            }
        }

        if let Some(n) = &mut self.new_student {
            let mut create = false;
            let mut cancel = false;
            let dup = self.db.b_number_taken(&n.student.b_number, None);
            let resp = egui::Modal::new(Id::unique("new-student")).frame(theme::modal_frame(ui)).show(&ctx, |ui| {
                ui.set_width(460.0);
                ui.horizontal(|ui| {
                    theme::icon_badge(ui, icons::STUDENT, theme::ACCENT, 40.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.label(theme::serif("Add student", 24.0));
                        ui.label(RichText::new("Details can be completed later in the side panel.").small().color(theme::muted(ui)));
                    });
                });
                ui.add_space(10.0);
                let external = n.phd && n.research.external();
                crate::ui::widgets::form_grid(ui, "new-grid", |ui| {
                    let s = &mut n.student;
                    ui.label(crate::model::id_label(external));
                    ui.vertical(|ui| {
                        let hint = if external { "Registration number at their university" } else { "B00123456" };
                        crate::ui::widgets::text_field(ui, &mut s.b_number, hint);
                        if dup {
                            ui.colored_label(theme::fg(ui, theme::AMBER), format!("{} Already exists", icons::WARNING));
                        }
                    });
                    ui.end_row();
                    ui.label("Name");
                    crate::ui::widgets::text_field(ui, &mut s.name, "Full name");
                    ui.end_row();
                    ui.label("Email");
                    crate::ui::widgets::text_field(ui, &mut s.email, "University email");
                    ui.end_row();
                    ui.label("Course");
                    crate::ui::widgets::text_field(ui, &mut s.course, "Course");
                    ui.end_row();
                });
                ui.add_space(8.0);
                theme::card_title(ui, "Roles");
                ui.checkbox(&mut n.fyp, "Project student (FYP)");
                if n.fyp {
                    ui.indent("fyp-kind", |ui| {
                        for p in &self.db.settings.programmes {
                            ui.radio_value(&mut n.programme, p.id.clone(), &p.name);
                        }
                    });
                }
                ui.checkbox(&mut n.phd, "Research student (PhD / MSc)");
                if n.phd {
                    ui.indent("research-kind", |ui| {
                        for group in ResearchGroup::ALL {
                            ui.radio_value(&mut n.research, group, group.label());
                        }
                    });
                }
                ui.checkbox(&mut n.placement, "Placement student");
                if n.placement {
                    ui.indent("placement", |ui| {
                        crate::ui::widgets::text_field(ui, &mut n.company, "Placement company");
                        crate::ui::widgets::text_field(ui, &mut n.professional_email, "Professional email");
                    });
                }
                ui.checkbox(&mut n.advisee, "Studies advisee");
                let mut marks = n.marking.is_some();
                if ui.checkbox(&mut marks, "I mark their work, but don't supervise them").changed() {
                    n.marking = marks.then_some(MarkingKind::Project);
                }
                if let Some(kind) = &mut n.marking {
                    ui.indent("marking", |ui| {
                        for k in MarkingKind::ALL {
                            ui.radio_value(kind, k, k.label());
                        }
                        if *kind == MarkingKind::Project {
                            egui::ComboBox::from_id_salt("marking-course")
                                .selected_text(self.db.settings.programme_short(&n.marking_programme))
                                .show_ui(ui, |ui| {
                                    for p in self.db.settings.programmes.iter().filter(|p| !self.db.settings.is_sust(&p.id)) {
                                        ui.selectable_value(&mut n.marking_programme, p.id.clone(), &p.name);
                                    }
                                });
                        }
                    });
                }
                ui.add_space(12.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let valid = !n.student.name.trim().is_empty() || !n.student.b_number.trim().is_empty();
                    if ui.add_enabled(valid, theme::primary_button("Create")).clicked() {
                        create = true;
                    }
                    if ui.add(egui::Button::new("Cancel").min_size(egui::vec2(0.0, 34.0))).clicked() {
                        cancel = true;
                    }
                });
            });
            if create {
                let student = self.new_student.take().expect("open").build();
                self.selected = Some(student.id);
                self.db.students.push(student);
            } else if cancel || resp.should_close() {
                self.new_student = None;
            }
        }

        if let Some(prompt) = &mut self.reveal_prompt {
            let name = self.db.student(prompt.student).map(|s| s.display_name()).unwrap_or_default();
            let (title, explain, button) = match prompt.what {
                Reveal::Number => (
                    "Reveal registration number",
                    format!("Type your password to see the registration number of {name}."),
                    "Reveal",
                ),
                Reveal::RarNotes => ("RAR Notes", format!("Type your password to see the RAR notes of {name}."), "Open"),
            };
            let mut submit = false;
            let mut cancel = false;
            let resp = egui::Modal::new(Id::unique("reveal")).frame(theme::modal_frame(ui)).show(&ctx, |ui| {
                ui.set_width(420.0);
                ui.horizontal(|ui| {
                    theme::icon_badge(ui, icons::EYE, theme::ACCENT, 40.0);
                    ui.label(theme::serif(title, 22.0));
                });
                ui.add_space(4.0);
                ui.add(
                    egui::Label::new(explain)
                        .wrap_mode(egui::TextWrapMode::Wrap),
                );
                ui.add_space(6.0);
                let field = crate::ui::widgets::password_field(
                    ui,
                    egui::TextEdit::singleline(&mut *prompt.password)
                        .hint_text("Password")
                        .desired_width(f32::INFINITY),
                );
                if !field.has_focus() && prompt.password.is_empty() && prompt.error.is_none() {
                    field.request_focus();
                }
                if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    submit = true;
                }
                if let Some(e) = &prompt.error {
                    ui.colored_label(theme::fg(ui, theme::RED), e);
                }
                ui.add_space(12.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(theme::primary_button(button)).clicked() {
                        submit = true;
                    }
                    if ui.add(egui::Button::new("Cancel").min_size(egui::vec2(0.0, 34.0))).clicked() {
                        cancel = true;
                    }
                });
            });
            if submit && !prompt.password.is_empty() {
                match self.vault.check_password(&prompt.password) {
                    Ok(true) => {
                        match prompt.what {
                            Reveal::Number => self.revealed = Some(prompt.student),
                            Reveal::RarNotes => {
                                self.rar_notes = Some(RarDialog { student: prompt.student, focused: false });
                            }
                        }
                        self.reveal_prompt = None;
                    }
                    Ok(false) => {
                        prompt.error = Some("Wrong password.".into());
                        prompt.password = Zeroizing::new(String::new());
                    }
                    Err(e) => prompt.error = Some(format!("{e:#}")),
                }
            } else if cancel || resp.should_close() {
                self.reveal_prompt = None;
            }
        }

        self.rar_notes_dialog(&ctx, ui);

        if let Some(confirm) = &self.confirm {
            let (title, body) = match confirm {
                Confirm::DeleteStudent(id) => (
                    "Delete student?".to_string(),
                    format!(
                        "Permanently delete {} and all their records?",
                        self.db.student(*id).map(|s| s.display_name()).unwrap_or_default()
                    ),
                ),
                Confirm::RemoveRole(_, role) => (
                    format!("Remove {} record?", role.label()),
                    "All dates and notes in this section will be lost.".to_string(),
                ),
            };
            let mut yes = false;
            let mut no = false;
            let resp = egui::Modal::new(Id::unique("confirm")).frame(theme::modal_frame(ui)).show(&ctx, |ui| {
                ui.set_width(380.0);
                ui.horizontal(|ui| {
                    theme::icon_badge(ui, icons::TRASH, theme::RED, 40.0);
                    ui.label(theme::serif(title, 22.0));
                });
                ui.add_space(4.0);
                ui.label(body);
                ui.add_space(12.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(theme::danger_button("Delete")).clicked() {
                        yes = true;
                    }
                    if ui.add(egui::Button::new("Cancel").min_size(egui::vec2(0.0, 34.0))).clicked() {
                        no = true;
                    }
                });
            });
            if yes {
                match self.confirm.take().expect("open") {
                    Confirm::DeleteStudent(id) => {
                        self.db.students.retain(|s| s.id != id);
                        self.selected = None;
                    }
                    Confirm::RemoveRole(id, role) => {
                        if let Some(s) = self.db.student_mut(id) {
                            match role {
                                Role::Fyp => s.fyp = None,
                                Role::Phd => s.phd = None,
                                Role::Placement => s.placement = None,
                                Role::Advisee => s.advisee = None,
                                Role::Marking => s.marking = None,
                            }
                            // Nothing shows these any more, so nothing should keep them.
                            if s.fyp.is_none() && s.advisee.is_none() {
                                s.modules = Default::default();
                            }
                        }
                    }
                }
            } else if no || resp.should_close() {
                self.confirm = None;
            }
        }
    }
}

impl Session {
    /// A student's RAR notes, in a dialog like the meeting notes.
    fn rar_notes_dialog(&mut self, ctx: &egui::Context, ui: &Ui) {
        let Some(dialog) = &mut self.rar_notes else { return };
        let Some(student) = self.db.student_mut(dialog.student) else {
            self.rar_notes = None;
            return;
        };
        let name = student.display_name();
        let edit_id = Id::unique(("rar-notes", dialog.student));
        let mut done = false;
        let resp = egui::Modal::new(Id::unique("rar-notes-dialog")).frame(theme::modal_frame(ui)).show(ctx, |ui| {
            ui.set_width(560.0);
            ui.horizontal(|ui| {
                theme::icon_badge(ui, icons::NOTE_PENCIL, theme::ACCENT, 40.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.label(theme::serif("RAR Notes", 22.0));
                    ui.label(RichText::new(name).color(theme::muted(ui)));
                });
            });
            ui.add_space(10.0);
            let edit = egui::ScrollArea::vertical()
                .max_height(340.0)
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut student.rar_notes)
                            .id(edit_id)
                            .hint_text("The student's reasonable adjustments…")
                            .desired_rows(14)
                            .desired_width(f32::INFINITY),
                    )
                })
                .inner;
            if !dialog.focused {
                dialog.focused = true;
                edit.request_focus();
            }
            ui.add_space(12.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                done = ui.add(theme::primary_button("Done")).clicked();
            });
        });
        if done || resp.should_close() {
            self.rar_notes = None;
            // egui keeps an undo history of each text field: don't leave the
            // notes there once they are behind the password again.
            ctx.data_mut(|d| d.remove::<egui::text_edit::TextEditState>(edit_id));
        }
    }
}

#[cfg(test)]
mod ui_tests {
    //! Drives the real UI headlessly. Set `SUPERVISOR_SHOTS=<dir>` to also
    //! write screenshots of each screen.
    use super::*;
    use crate::model::*;
    use chrono::{Datelike, Duration as Days, NaiveTime};
    use egui::accesskit::Role as AkRole;
    use egui_kittest::Harness;
    use egui_kittest::kittest::{By, NodeT, Queryable};

    const PASSWORD: &str = "test-password-123";

    fn session<'s>(h: &'s mut Harness<'_, SupervisorApp>) -> &'s mut Session {
        match &mut h.state_mut().screen {
            Screen::Unlocked(s) => s,
            Screen::Locked(_) => panic!("still locked"),
        }
    }

    fn shot(h: &mut Harness<'_, SupervisorApp>, name: &str) {
        if let Ok(dir) = std::env::var("SUPERVISOR_SHOTS") {
            h.run_steps(3);
            let img = h.render().expect("render");
            img.save(std::path::Path::new(&dir).join(format!("{name}.png"))).unwrap();
        }
    }

    /// Runs frames until `done`, giving work in the background (unlocking,
    /// saving) time to finish.
    fn wait_until(h: &mut Harness<'_, SupervisorApp>, what: &str, done: impl Fn(&SupervisorApp) -> bool) {
        for _ in 0..600 {
            h.run_steps(1);
            if done(h.state()) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!("timed out waiting for: {what}");
    }

    fn wait_unlocked(h: &mut Harness<'_, SupervisorApp>) {
        wait_until(h, "unlocked", |app| matches!(app.screen, Screen::Unlocked(_)));
    }

    fn locked(app: &SupervisorApp) -> bool {
        matches!(app.screen, Screen::Locked(_))
    }

    /// Unlocked with every change saved and no save running.
    fn all_saved(app: &SupervisorApp) -> bool {
        matches!(&app.screen, Screen::Unlocked(s) if s.saving.is_none() && s.dirty_since.is_none())
    }

    fn status_is(app: &SupervisorApp, want: fn(&SaveStatus) -> bool) -> bool {
        matches!(&app.screen, Screen::Unlocked(s) if s.saving.is_none() && want(&s.status))
    }

    fn demo_students(today: chrono::NaiveDate) -> Vec<Student> {
        let t = |h| NaiveTime::from_hms_opt(h, 0, 0);
        let meeting = |days: i64, held: bool, notes: &str| Meeting {
            date: Some(today + Days::days(days)),
            time: t(14),
            held,
            notes: notes.into(),
            ..Default::default()
        };
        vec![
            Student {
                b_number: "B00912345".into(),
                name: "Aisha Khan".into(),
                email: "khan-a@uni.ac.uk".into(),
                course: "BEng Mechanical Engineering".into(),
                fyp: Some(FypRecord {
                    programme: "BEng".into(),
                    project_title: "Topology-optimised drone arm".into(),
                    initial_contact: true,
                    initial_contact_on: Some(today - Days::days(30)),
                    meetings: vec![
                        meeting(-14, true, "Agreed scope."),
                        Meeting {
                            location: "MG.201".into(),
                            repeat: Some(Repeat {
                                exceptions: vec![Occurrence {
                                    planned: today + Days::days(10),
                                    cancelled: true,
                                    ..Default::default()
                                }],
                                ..Default::default()
                            }),
                            ..meeting(3, false, "")
                        },
                    ],
                    ..Default::default()
                }),
                advisee: Some(AdviseeRecord {
                    s1: SemesterContact {
                        contacted_on: Some(today - Days::days(20)),
                        reply: ReplyStatus::Replied,
                        replied_on: Some(today - Days::days(18)),
                        meetings: vec![
                            meeting(-10, true, "Settling in well."),
                            Meeting {
                                online: true,
                                ..meeting(5, false, "Follow-on requested")
                            },
                        ],
                    },
                    ..Default::default()
                }),
                ..Default::default()
            },
            Student {
                b_number: "B00987654".into(),
                name: "Tom Murphy".into(),
                course: "MEng Electrical Engineering".into(),
                fyp: Some(FypRecord {
                    programme: "MEngGermany".into(),
                    ..Default::default()
                }),
                ..Default::default()
            },
            Student {
                b_number: "SUST-2026-01".into(),
                name: "Li Wei".into(),
                course: "BEng (SUST)".into(),
                fyp: Some(FypRecord {
                    programme: "Sust".into(),
                    initial_contact: true,
                    ..Default::default()
                }),
                ..Default::default()
            },
            Student {
                b_number: "B00876543".into(),
                name: "Ciara Doyle".into(),
                course: "BEng Mechanical Engineering".into(),
                placement: Some(PlacementRecord {
                    company: "Bombardier".into(),
                    professional_email: "ciara.doyle@company.com".into(),
                    line_manager_name: "J. Smith".into(),
                    visits: [
                        Visit {
                            request_sent: Some(today - Days::days(7)),
                            response: ReplyStatus::Replied,
                            date: Some(today + Days::days(6)),
                            time: t(11),
                            ..Default::default()
                        },
                        Visit {
                            mode: VisitMode::FaceToFace,
                            ..Default::default()
                        },
                    ],
                    ..Default::default()
                }),
                ..Default::default()
            },
            Student {
                b_number: "B00765432".into(),
                name: "Sean O'Neill".into(),
                course: "BEng Electrical Engineering".into(),
                advisee: Some(AdviseeRecord {
                    s1: SemesterContact {
                        contacted_on: Some(today - Days::days(21)),
                        reply: ReplyStatus::NoReply,
                        ..Default::default()
                    },
                    ..Default::default()
                }),
                ..Default::default()
            },
            Student {
                b_number: "B00654321".into(),
                name: "Priya Raman".into(),
                email: "raman-p@uni.ac.uk".into(),
                course: "PhD Engineering".into(),
                phd: Some(PhdRecord {
                    title: "Self-healing composites for wind turbine blades".into(),
                    chair: "Prof. A. Byrne".into(),
                    fee_status: FeeStatus::Overseas,
                    visa: VisaType::Student,
                    visa_start: Some(today - Days::days(60)),
                    visa_end: Some(today + Days::days(1420)),
                    funder: Funder::DfeQr,
                    start: Some(today - Days::days(40)),
                    end: Some(today + Days::days(1055)),
                    initial: Milestone {
                        deadline: Some(today + Days::days(130)),
                        ..Default::default()
                    },
                    confirmation: Milestone {
                        deadline: Some(today + Days::days(405)),
                        ..Default::default()
                    },
                    annual_reports: vec![Milestone {
                        deadline: Some(today + Days::days(25)),
                        ..Default::default()
                    }],
                    meetings: vec![Meeting {
                        online: true,
                        repeat: Some(Repeat {
                            every_weeks: 2,
                            ..Default::default()
                        }),
                        ..meeting(2, false, "")
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            },
        ]
    }

    /// Opens the app on `path`, which doesn't exist yet, and creates the password.
    fn create_and_unlock(path: &std::path::Path, shots: bool) -> Harness<'static, SupervisorApp> {
        let p = path.to_path_buf();
        let mut h = Harness::builder()
            .with_size(egui::vec2(1360.0, 860.0))
            .build_eframe(move |cc| SupervisorApp::new(cc, Some(p)));
        h.run_steps(2);
        if shots {
            shot(&mut h, "01_create_password");
        }

        // First run: create the password. A short one is refused first.
        let (start, rest) = PASSWORD.split_at(4);
        for text in [start, rest] {
            let fields = h.get_all_by_role(AkRole::PasswordInput).collect::<Vec<_>>();
            assert_eq!(fields.len(), 2);
            for field in &fields {
                field.focus();
                field.type_text(text);
            }
            h.run_steps(1);
            h.get_by_label("Create & unlock").click();
            h.run_steps(2);
            if text == start {
                assert!(h.query_by_label_contains("at least 12 characters").is_some());
                assert!(!path.exists());
            }
        }
        wait_unlocked(&mut h);
        h.run_steps(2);
        assert!(path.exists());
        h
    }

    #[test]
    fn end_to_end() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        let mut h = create_and_unlock(&path, true);

        // Add a student through the dialog.
        h.get_by_label("Placement").click();
        h.run_steps(2);
        h.get_by_label(&format!("{}  Add student", icons::PLUS)).click();
        h.run_steps(2);
        shot(&mut h, "02_add_student_dialog");
        let inputs = h.get_all_by_role(AkRole::TextInput).collect::<Vec<_>>();
        // The list's search box, then the dialog's B number, name, email,
        // course, company and professional email.
        assert_eq!(inputs.len(), 7);
        inputs[1].focus();
        inputs[1].type_text("B00111222");
        inputs[2].focus();
        inputs[2].type_text("Niamh Kelly");
        h.run_steps(1);
        h.get_by_label("Create").click();
        h.run_steps(2);
        let created = session(&mut h)
            .db
            .students
            .iter()
            .find(|s| s.name == "Niamh Kelly")
            .cloned()
            .expect("student created");
        assert!(created.placement.is_some(), "Placement role preset from the view");

        // Autosave after the debounce: the encrypted file must contain the student.
        for _ in 0..90 {
            h.run_steps(1);
        }
        wait_until(&mut h, "autosave", all_saved);
        let saved = storage::read_database(&path, PASSWORD, None).unwrap();
        assert!(saved.students.iter().any(|s| s.name == "Niamh Kelly"));

        // Seed richer data and capture each screen.
        let today = crate::ui::widgets::today();
        {
            let s = session(&mut h);
            s.db.settings.sem1_start = Some(today - Days::days(30));
            s.db.students.extend(demo_students(today));
            s.selected = None;
            s.view = View::Dashboard;
        }
        h.run_steps(2);
        assert!(h.query_by_label_contains("no reply").is_some(), "dashboard attention list");
        assert!(h.query_by_label_contains("Annual report").is_some(), "PhD reminder on the dashboard");
        shot(&mut h, "03_dashboard");

        for (view, name) in [
            (View::Calendar, "04_calendar"),
            (View::List(Category::Projects("BEng".into())), "04_fyp_beng"),
            (View::List(Category::Placement), "05_placement"),
            (View::List(Category::Advisee), "06_advisees"),
            (View::List(Category::Research(ResearchGroup::Phd)), "11_phd"),
            (View::Settings, "07_settings"),
        ] {
            session(&mut h).view = view;
            h.run_steps(2);
            shot(&mut h, name);
        }

        // Month grid: clicking an item selects it for export.
        {
            let s = session(&mut h);
            s.view = View::Calendar;
            s.calendar.layout = crate::ui::calendar::Layout::Month;
            s.calendar.month = (today + Days::days(3)).with_day(1).unwrap();
        }
        h.run_steps(3);
        shot(&mut h, "04_calendar_month");
        h.get_all_by_label_contains("Aisha Khan").next().expect("meeting in month grid").click();
        h.run_steps(2);
        assert_eq!(session(&mut h).calendar.selected.len(), 1);
        assert!(session(&mut h).db.settings.export_with_guests, "guests on by default");

        // Detail panels.
        let id_of = |h: &mut Harness<'_, SupervisorApp>, name: &str| {
            session(h).db.students.iter().find(|s| s.name == name).unwrap().id
        };
        let aisha = id_of(&mut h, "Aisha Khan");
        {
            let s = session(&mut h);
            s.view = View::List(Category::Projects("BEng".into()));
            s.selected = Some(aisha);
        }
        h.run_steps(2);
        shot(&mut h, "08_fyp_detail");
        let ciara = id_of(&mut h, "Ciara Doyle");
        {
            let s = session(&mut h);
            s.view = View::List(Category::Placement);
            s.selected = Some(ciara);
        }
        h.run_steps(2);
        shot(&mut h, "09_placement_detail");
        let priya = id_of(&mut h, "Priya Raman");
        {
            let s = session(&mut h);
            s.view = View::List(Category::Research(ResearchGroup::Phd));
            s.selected = Some(priya);
        }
        h.run_steps(2);
        shot(&mut h, "12_phd_detail");

        // An external MSc: no standard assessments, its own deadline, a Google Meet.
        {
            let s = session(&mut h);
            let mut p = ResearchGroup::MscExternal.new_record();
            p.university = "UFERSA".into();
            p.custom_steps.push(crate::model::CustomStep {
                title: "Qualifying exam".into(),
                milestone: crate::model::Milestone {
                    deadline: Some(today + Days::days(10)),
                    ..Default::default()
                },
                ..Default::default()
            });
            p.meetings.push(crate::model::Meeting {
                date: Some(today + Days::days(2)),
                online: true,
                platform: crate::model::Platform::GoogleMeet,
                link: "https://meet.google.com/abc-defg-hij".into(),
                ..Default::default()
            });
            let ana = Student {
                name: "Ana Souza".into(),
                phd: Some(p),
                ..Default::default()
            };
            s.selected = Some(ana.id);
            s.db.students.push(ana);
            s.view = View::List(Category::Research(ResearchGroup::MscExternal));
        }
        h.set_size(egui::vec2(1360.0, 1800.0));
        h.run_steps(3);
        shot(&mut h, "13_external_detail");
        h.set_size(egui::vec2(1360.0, 860.0));
        h.run_steps(2);

        // Dark mode.
        session(&mut h).db.settings.dark_mode = true;
        session(&mut h).view = View::Dashboard;
        h.run_steps(3);
        assert!(h.ctx.global_style().visuals.dark_mode);
        shot(&mut h, "10_dashboard_dark");

        // Lock: back to the unlock screen, and the right password reopens the data.
        h.get_by_label(&format!("{}  Lock", icons::LOCK_SIMPLE)).click();
        wait_until(&mut h, "locked", locked);
        let (_, saved) = Vault::open(&path, PASSWORD, None).unwrap();
        assert!(saved.students.iter().any(|s| s.name == "Ciara Doyle"));
        assert!(saved.settings.dark_mode);
    }

    fn unlock(h: &mut Harness<'_, SupervisorApp>) {
        let field = h.get_by_role(AkRole::PasswordInput);
        field.focus();
        field.type_text(PASSWORD);
        h.run_steps(1);
        h.get_by_label("Unlock").click();
        wait_unlocked(h);
        h.run_steps(2);
    }

    #[test]
    fn lock_waits_for_save_and_locks_when_idle() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        let mut h = create_and_unlock(&path, false);

        // Saving fails (a folder is in the backup's way): Lock must not drop the edit.
        let bak = dir.path().join("data.svt.bak");
        std::fs::create_dir_all(bak.join("blocker")).unwrap();
        session(&mut h).db.settings.my_name = "Unsaved edit".into();
        h.run_steps(1);
        h.get_by_label(&format!("{}  Lock", icons::LOCK_SIMPLE)).click();
        wait_until(&mut h, "save failed", |app| status_is(app, |s| matches!(s, SaveStatus::Error(_))));
        h.run_steps(2);
        assert!(matches!(h.state().screen, Screen::Unlocked(_)), "stays unlocked when saving fails");
        assert!(h.query_by_label("Not saved yet").is_some());

        // Once saving works again, Try again saves and locks.
        std::fs::remove_dir_all(&bak).unwrap();
        h.get_by_label("Try again").click();
        wait_until(&mut h, "locked", locked);
        assert_eq!(storage::read_database(&path, PASSWORD, None).unwrap().settings.my_name, "Unsaved edit");

        // Idle for longer than the auto-lock time: locks by itself.
        unlock(&mut h);
        assert_eq!(session(&mut h).db.settings.auto_lock_minutes, 10);
        if let Some(earlier) = Instant::now().checked_sub(Duration::from_secs(11 * 60)) {
            session(&mut h).last_active = earlier;
            h.run_steps(1);
            assert!(matches!(h.state().screen, Screen::Locked(_)), "auto-locked");
        }
    }

    #[test]
    fn changed_elsewhere_asks_before_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        let mut h = create_and_unlock(&path, false);

        // Another computer saves the file (simulated by writing other bytes).
        std::fs::write(&path, b"someone else's version").unwrap();
        session(&mut h).db.settings.my_name = "Mine".into();
        // Edits are only looked for on frames with input, as typing would give.
        h.key_press(egui::Key::F20);
        for _ in 0..90 {
            h.run_steps(1);
        }
        wait_until(&mut h, "conflict", |app| status_is(app, |s| matches!(s, SaveStatus::Conflict)));
        h.run_steps(2);
        assert_eq!(std::fs::read(&path).unwrap(), b"someone else's version", "not overwritten");
        assert!(h.query_by_label("Changed somewhere else").is_some());

        h.get_by_label("Save my version").click();
        h.run_steps(2);
        assert_eq!(storage::read_database(&path, PASSWORD, None).unwrap().settings.my_name, "Mine");
        assert_eq!(std::fs::read(dir.path().join("data.svt.bak")).unwrap(), b"someone else's version");
    }

    #[test]
    fn edit_made_while_saving_is_saved_afterwards() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        let mut h = create_and_unlock(&path, false);
        let ctx = h.ctx.clone();
        let now = ctx.input(|i| i.time);
        let s = session(&mut h);
        s.db.settings.my_name = "First".into();
        s.detect_changes(now);
        s.save(&ctx);
        // Edited while that save is still running.
        s.db.settings.my_name = "Second".into();
        s.detect_changes(now);
        s.poll_save(now, true);
        assert_eq!(storage::read_database(&path, PASSWORD, None).unwrap().settings.my_name, "First");
        assert!(matches!(s.status, SaveStatus::Pending), "the newer edit isn't saved yet");
        assert!(s.dirty_since.is_some());

        wait_until(&mut h, "second save", all_saved);
        assert_eq!(storage::read_database(&path, PASSWORD, None).unwrap().settings.my_name, "Second");
    }

    #[test]
    fn calendar_list_draws_only_weeks_in_view() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        let mut h = create_and_unlock(&path, false);
        let first = chrono::NaiveDate::from_ymd_opt(2026, 1, 5).unwrap();
        let last = first + Days::weeks(59);
        {
            let s = session(&mut h);
            s.db.students.push(Student {
                name: "Weekly Student".into(),
                fyp: Some(FypRecord {
                    programme: "BEng".into(),
                    meetings: vec![Meeting {
                        date: Some(first),
                        time: NaiveTime::from_hms_opt(10, 0, 0),
                        repeat: Some(Repeat {
                            until: Some(last),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            });
            s.view = View::Calendar;
            s.calendar.range = crate::ui::calendar::Range::All;
        }
        h.key_press(egui::Key::F20);
        h.run_steps(3);
        let week = |monday: chrono::NaiveDate| {
            format!("{} – {}", monday.format("%d %b"), (monday + Days::days(6)).format("%d %b %Y"))
        };
        assert!(h.query_by_label(&week(first)).is_some(), "first week drawn");
        assert!(h.query_by_label(&week(last)).is_none(), "weeks out of view not drawn");

        // Selecting covers every week, drawn or not.
        h.get_by_label("Select all shown").click();
        h.run_steps(2);
        assert_eq!(session(&mut h).calendar.selected.len(), 60);

        // Scrolled to the end, the last week is drawn.
        h.event(egui::Event::PointerMoved(egui::pos2(700.0, 500.0)));
        for _ in 0..80 {
            h.event(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -400.0),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Unknown,
                modifiers: egui::Modifiers::NONE,
            });
            h.run_steps(1);
        }
        h.run_steps(5);
        assert!(h.query_by_label(&week(last)).is_some(), "last week drawn once in view");
        assert!(h.query_by_label(&week(first)).is_none());
    }

    #[test]
    fn save_button_saves_now_and_keeps_that_version_as_the_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        let bak = dir.path().join("data.svt.bak");
        let mut h = create_and_unlock(&path, false);
        let name_in = |p: &std::path::Path| storage::read_database(p, PASSWORD, None).unwrap().settings.my_name;
        let edit = |h: &mut Harness<'_, SupervisorApp>, name: &str| {
            session(h).db.settings.my_name = name.into();
            h.key_press(egui::Key::F20);
        };

        // Ctrl+S saves without waiting for a pause in typing: with no frames
        // run after the key's own, an autosave can't have happened.
        session(&mut h).db.settings.my_name = "Checkpoint".into();
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        h.step();
        for _ in 0..100 {
            if name_in(&path) == "Checkpoint" {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(name_in(&path), "Checkpoint");
        wait_until(&mut h, "saved", all_saved);
        assert_eq!(name_in(&path), "Checkpoint");
        assert_eq!(name_in(&bak), "", "the first save keeps the file as unlocked");

        // The next save keeps the version saved by hand as the backup.
        edit(&mut h, "Later");
        for _ in 0..90 {
            h.run_steps(1);
        }
        wait_until(&mut h, "autosaved", all_saved);
        assert_eq!(name_in(&path), "Later");
        assert_eq!(name_in(&bak), "Checkpoint");

        // With nothing left to save, clicking Saved does the same.
        h.get_by_label(&format!("{} Saved", icons::FLOPPY_DISK)).click();
        h.run_steps(2);
        assert!(h.query_by_label("Saved").is_some(), "confirmed");
        edit(&mut h, "Latest");
        for _ in 0..90 {
            h.run_steps(1);
        }
        wait_until(&mut h, "autosaved", all_saved);
        assert_eq!(name_in(&bak), "Later");
    }

    #[test]
    fn calendar_marks_past_meetings_held_and_postpones_them() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        let mut h = create_and_unlock(&path, false);
        let today = crate::ui::widgets::today();
        let student = |name: &str, days: i64, hour: u32| Student {
            name: name.into(),
            fyp: Some(FypRecord {
                programme: "BEng".into(),
                meetings: vec![Meeting {
                    date: Some(today + Days::days(days)),
                    time: NaiveTime::from_hms_opt(hour, 0, 0),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        {
            let s = session(&mut h);
            s.db.students.extend([student("Past Student", -2, 10), student("Late Student", -1, 9), student("Future Student", 3, 10)]);
            s.view = View::Calendar;
            s.calendar.range = crate::ui::calendar::Range::All;
        }
        h.key_press(egui::Key::F20);
        h.run_steps(3);
        let meeting = |h: &mut Harness<'_, SupervisorApp>, name: &str| {
            let s = session(h);
            s.db.students.iter().find(|s| s.name == name).unwrap().fyp.as_ref().unwrap().meetings[0].clone()
        };

        // Only meetings whose time has come offer Held and Postpone.
        let held = format!("{} Held", icons::CHECK);
        assert_eq!(h.get_all_by_label(&held).count(), 2);
        shot(&mut h, "13_calendar_past_meetings");
        h.get_all_by_label(&held).next().unwrap().click();
        h.run_steps(2);
        assert!(meeting(&mut h, "Past Student").held);
        assert!(h.query_by_label("Marked as held").is_some());
        assert_eq!(h.get_all_by_label(&held).count(), 1);

        // Postpone, picking the day in the date picker inside the dialog.
        h.get_by_label("Postpone…").click();
        h.run_steps(2);
        assert!(h.query_by_label("Postpone meeting").is_some());
        let suggested = today - Days::days(1) + Days::days(7);
        h.get_by_label_contains(&suggested.format("%a %d %b %Y").to_string()).click();
        h.run_steps(2);
        shot(&mut h, "14_postpone_dialog");
        h.get_by_label(">").click();
        h.run_steps(1);
        h.get_by_label("Save").click();
        h.run_steps(1);
        h.get_by_label("Postpone").click();
        h.run_steps(2);
        let late = meeting(&mut h, "Late Student");
        assert_eq!(late.date, Some(suggested + Days::days(1)));
        assert_eq!(late.time, NaiveTime::from_hms_opt(9, 0, 0));
        assert!(h.query_by_label("Postpone meeting").is_none());

        // Month grid: right-click a past meeting to undo "held".
        {
            let s = session(&mut h);
            s.calendar.layout = crate::ui::calendar::Layout::Month;
            s.calendar.month = (today - Days::days(2)).with_day(1).unwrap();
        }
        h.run_steps(2);
        h.get_by_label_contains("Past Student").click_secondary();
        h.run_steps(2);
        shot(&mut h, "15_month_meeting_menu");
        h.get_by_label(&format!("{} Mark as not held", icons::X)).click();
        h.run_steps(2);
        assert!(!meeting(&mut h, "Past Student").held);
    }

    #[test]
    fn calendar_edits_drags_deletes_and_imports() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        let mut h = create_and_unlock(&path, false);
        // Days in the middle of next month, so the month grid shows them.
        let month = (crate::ui::widgets::today() + Days::days(31)).with_day(1).unwrap();
        let day = |n: u32| month.with_day(n).unwrap();
        let at = |hour| NaiveTime::from_hms_opt(hour, 0, 0);
        let ana = Student {
            name: "Ana Silva".into(),
            email: "ana.silva@ulster.ac.uk".into(),
            phd: Some(PhdRecord {
                meetings: vec![Meeting {
                    date: Some(day(10)),
                    time: at(14),
                    online: true,
                    platform: Platform::GoogleMeet,
                    link: "meet.google.com/abc-defg-hij".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let bob = Student {
            name: "Bob Jones".into(),
            fyp: Some(FypRecord {
                programme: "BEng".into(),
                meetings: vec![Meeting {
                    date: Some(day(4)),
                    time: at(9),
                    repeat: Some(Repeat {
                        until: Some(day(25)),
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let (ana_id, bob_id) = (ana.id, bob.id);
        {
            let s = session(&mut h);
            s.db.students.extend([ana, bob]);
            s.db.settings.week_starts_sunday = true;
            s.view = View::Calendar;
            s.calendar.layout = crate::ui::calendar::Layout::Month;
            s.calendar.month = month;
        }
        h.key_press(egui::Key::F20);
        h.run_steps(3);
        let ana_meeting = |h: &mut Harness<'_, SupervisorApp>| session(h).db.student(ana_id).unwrap().phd.as_ref().unwrap().meetings[0].clone();
        let bob_meeting = |h: &mut Harness<'_, SupervisorApp>| session(h).db.student(bob_id).unwrap().fyp.as_ref().unwrap().meetings[0].clone();

        // Weeks start on Sunday.
        let sun = h.get_by_label("SUN").rect();
        assert!(sun.left() < h.get_by_label("MON").rect().left());
        shot(&mut h, "20_month_sunday_first");

        // Drag Ana's meeting from the 10th to the 12th.
        let from = h.get_by_label_contains("14:00").rect().center();
        let to = h.get_by_label("12").rect().center() + egui::vec2(0.0, 30.0);
        h.drag_at(from);
        h.run_steps(1);
        h.hover_at(from + egui::vec2(10.0, 5.0));
        h.run_steps(1);
        h.hover_at(to);
        h.run_steps(1);
        h.drop_at(to);
        h.run_steps(2);
        let m = ana_meeting(&mut h);
        assert_eq!((m.date, m.time), (Some(day(12)), at(14)));

        // Right-click: edit, and Google Calendar with the student as guest.
        h.get_by_label_contains("Ana Silva").click_secondary();
        h.run_steps(2);
        shot(&mut h, "21_item_menu");
        h.get_by_label(&format!("{} Open in Google Calendar", icons::ARROW_SQUARE_OUT)).click();
        h.run_steps(1);
        let opened: Vec<String> = h
            .output()
            .platform_output
            .commands
            .iter()
            .filter_map(|c| match c {
                egui::OutputCommand::OpenUrl(u) => Some(u.url.clone()),
                _ => None,
            })
            .collect();
        assert!(opened.iter().any(|u| u.contains("calendar.google.com") && u.contains("add=ana.silva%40ulster.ac.uk")), "{opened:?}");
        h.run_steps(1);
        h.get_by_label_contains("Ana Silva").click_secondary();
        h.run_steps(2);
        h.get_by_label(&format!("{} Edit…", icons::PENCIL_SIMPLE)).click();
        h.run_steps(2);
        assert!(h.query_by_label("Link").is_some());
        shot(&mut h, "22_edit_dialog");
        h.get_by_label("Save").click();
        h.run_steps(2);
        assert!(h.query_by_label("Saved").is_some());

        // Select a week of Bob's meeting and press Delete: just that week is cancelled.
        let week = day(11);
        let select_bob = |h: &mut Harness<'_, SupervisorApp>| {
            h.get_all_by_label_contains("Bob Jones")
                .find(|n| n.rect().center().x > h.get_by_label("11").rect().left() - 5.0 && n.rect().center().y > h.get_by_label("11").rect().top())
                .unwrap()
                .click();
            h.run_steps(2);
        };
        // Escape clears the selection.
        select_bob(&mut h);
        assert_eq!(session(&mut h).calendar.selected.len(), 1);
        h.key_press(egui::Key::Escape);
        h.run_steps(2);
        assert!(session(&mut h).calendar.selected.is_empty());
        select_bob(&mut h);
        h.key_press(egui::Key::Delete);
        h.run_steps(2);
        assert!(h.query_by_label("Delete this item?").is_some());
        shot(&mut h, "23_delete_dialog");
        h.get_by_label("Cancel this week").click();
        h.run_steps(2);
        assert!(bob_meeting(&mut h).occurrence(week).cancelled);
        assert_eq!(bob_meeting(&mut h).occurrences(&session(&mut h).db.settings).len(), 4);

        // Import: the event with Ana as a guest is ticked and becomes her meeting.
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:g1@google.com\r\nSUMMARY:Thesis chat\r\n\
            DTSTART;TZID=Europe/London:20271103T100000\r\nATTENDEE;CN=Ana:mailto:ana.silva@ulster.ac.uk\r\n\
            END:VEVENT\r\nBEGIN:VEVENT\r\nUID:g2@google.com\r\nSUMMARY:Dentist\r\nDTSTART:20271104T090000Z\r\n\
            END:VEVENT\r\nEND:VCALENDAR\r\n";
        {
            let s = session(&mut h);
            let events = crate::ics_import::parse(ics, &jiff::tz::TimeZone::get("Europe/London").unwrap());
            s.calendar.import = Some(calendar::ImportDialog::new("work.ics".into(), events, &s.db));
        }
        h.run_steps(3);
        assert!(h.query_by_label("1 to import, 1 as student meetings").is_some());
        shot(&mut h, "24_import_dialog");
        h.get_by_label("Import 1").click();
        h.run_steps(2);
        assert!(h.query_by_label("Imported 1 event").is_some());
        let s = session(&mut h);
        let imported = &s.db.student(ana_id).unwrap().phd.as_ref().unwrap().meetings[1];
        assert_eq!((imported.source.as_str(), imported.time), ("g1@google.com", at(10)));
        assert_eq!(s.db.ignored_imports, ["g2@google.com"]);

        // Importing the file again: "Tick all shown" leaves out the event
        // imported before, and ticking it by hand warns it would show twice.
        let events = crate::ics_import::parse(ics, &jiff::tz::TimeZone::get("Europe/London").unwrap());
        s.calendar.import = Some(calendar::ImportDialog::new("work.ics".into(), events, &s.db));
        h.run_steps(3);
        assert!(h.query_by_label("Nothing ticked").is_some());
        h.get_by_label("Tick all shown").click();
        h.run_steps(2);
        assert!(h.query_by_label("1 to import, none linked to a student").is_some(), "only the skipped one");
        assert!(h.query_by_label_contains("already in Supervisor").is_none());
        // The one left unticked.
        h.get_all_by_role(AkRole::CheckBox)
            .find(|c| c.accesskit_node().toggled() == Some(egui::accesskit::Toggled::False))
            .unwrap()
            .click();
        h.run_steps(2);
        assert!(h.query_by_label(&format!("{} will show twice", icons::WARNING)).is_some());
        assert!(h.query_by_label(&format!("{} 1 is already in Supervisor", icons::WARNING)).is_some());
        shot(&mut h, "25_import_again");
    }

    #[test]
    fn privacy_mode_hides_names_and_marks_until_revealed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        let mut h = create_and_unlock(&path, false);
        let today = widgets::today();
        let (bob, li) = {
            let s = session(&mut h);
            let sust = s.db.settings.modules.iter_mut().find(|m| m.applies_to == ["Sust"]).unwrap();
            sust.courseworks.push(CourseworkTemplate { title: "Report".into(), ..Default::default() });
            // MEC526 is marked by the supervisor alone; here, by both.
            s.db.settings.modules.iter_mut().find(|m| m.applies_to == ["BEng"]).unwrap().courseworks[0].markers =
                crate::model::Markers::Both;
            let first_cw = |programme: &str| s.db.settings.modules_for(programme).next().unwrap().courseworks[0].id;
            let marked = |cw| {
                std::collections::HashMap::from([(cw, CwStatus { mark_1: "71.5".into(), mark_2: "64.5".into(), ..Default::default() })])
            };
            let bob = Student {
                name: "Bob Jones".into(),
                b_number: "B00999111".into(),
                email: "bob.jones@ulster.ac.uk".into(),
                fyp: Some(FypRecord {
                    coursework: marked(first_cw("BEng")),
                    meetings: vec![Meeting {
                        date: Some(today + Days::days(2)),
                        time: NaiveTime::from_hms_opt(10, 0, 0),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            };
            let li = Student {
                name: "Li Wei".into(),
                fyp: Some(FypRecord {
                    programme: "Sust".into(),
                    sust_email: "li.wei@sust.edu.cn".into(),
                    coursework: marked(first_cw("Sust")),
                    ..Default::default()
                }),
                ..Default::default()
            };
            let ids = (bob.id, li.id);
            s.db.students.extend([bob, li]);
            ids
        };
        h.key_press(egui::Key::F20);
        h.run_steps(3);
        assert!(all_text(&h).contains("Bob Jones (B00999111) – no initial contact yet"));
        assert!(all_text(&h).contains("BEng project meeting – Bob Jones (B00999111)"), "upcoming meeting");

        // Privacy mode: nobody is named on the dashboard.
        h.get_by_label(&format!("{}  Hide names", icons::EYE_SLASH)).click();
        h.run_steps(2);
        let text = all_text(&h);
        assert!(!text.contains("Bob") && !text.contains("B00999111") && !text.contains("Li Wei"), "{text}");
        assert!(text.contains("no initial contact yet") && text.contains("BEng project meeting"));
        shot(&mut h, "26_dashboard_private");

        // ...nor in the lists or the calendar.
        session(&mut h).view = View::List(Category::All);
        h.run_steps(3);
        let text = all_text(&h);
        assert!(!text.contains("Bob") && !text.contains("B00999111") && !text.contains("bob.jones"), "{text}");
        assert!(!text.contains("Li Wei"));
        shot(&mut h, "26b_list_private");
        session(&mut h).view = View::Calendar;
        h.run_steps(3);
        let text = all_text(&h);
        assert!(!text.contains("Bob") && !text.contains("B00999111") && text.contains("BEng project meeting"), "{text}");
        shot(&mut h, "26c_calendar_private");
        session(&mut h).view = View::Dashboard;

        // ...nor in their details, until revealed for one student.
        session(&mut h).selected = Some(bob);
        h.run_steps(3);
        assert!(!all_text(&h).contains("Bob") && !all_text(&h).contains("bob.jones"));
        shot(&mut h, "27_details_private");
        h.get_by_label("Reveal the student's details").click();
        h.run_steps(2);
        assert!(all_text(&h).contains("bob.jones@ulster.ac.uk"));
        session(&mut h).selected = Some(li);
        h.run_steps(2);
        assert!(!all_text(&h).contains("li.wei@sust.edu.cn"), "the SUST email too");
        session(&mut h).selected = Some(bob);
        h.run_steps(2);
        assert!(!all_text(&h).contains("bob.jones"), "hidden again after switching students");

        // Out of privacy mode, details show unless hidden for that student.
        h.get_by_label(&format!("{}  Show names", icons::EYE)).click();
        h.run_steps(2);
        assert!(all_text(&h).contains("bob.jones@ulster.ac.uk"));
        h.get_by_label("Hide the student's details").click();
        h.run_steps(2);
        session(&mut h).selected = Some(li);
        h.run_steps(2);
        assert!(all_text(&h).contains("li.wei@sust.edu.cn"));
        session(&mut h).selected = Some(bob);
        h.run_steps(2);
        assert!(!all_text(&h).contains("bob.jones"), "still hidden for Bob");
        h.get_by_label("Reveal the student's details").click();
        h.run_steps(2);

        // Marks are dots until revealed, for this student only. (A taller
        // window, to reach them.)
        h.set_size(egui::vec2(1360.0, 1300.0));
        h.run_steps(2);
        assert!(!all_text(&h).contains("71.5"));
        assert!(h.query_all_by_label("Marker 2").next().is_some());
        h.get_all_by_label(&format!("{} Reveal marks", icons::EYE)).next().unwrap().click();
        h.run_steps(2);
        assert!(all_text(&h).contains("71.5") && all_text(&h).contains("64.5"));
        shot(&mut h, "28_marks_revealed");

        // Marker 2 within 10% of marker 1 gives their average, to confirm.
        let retype = |h: &mut Harness<'_, SupervisorApp>, from: &str, to: &str| {
            let field = h.get(By::new().value(from).role(AkRole::TextInput));
            field.focus();
            h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
            h.get(By::new().value(from).role(AkRole::TextInput)).type_text(to);
            h.run_steps(2);
        };
        let agreed = |h: &mut Harness<'_, SupervisorApp>| {
            let s = session(h);
            let cw = s.db.settings.modules_for("BEng").next().unwrap().courseworks[0].id;
            let status = &s.db.student(bob).unwrap().fyp.as_ref().unwrap().coursework[&cw];
            (status.agreed_mark.clone(), status.agreed_unconfirmed)
        };
        retype(&mut h, "64.5", "66");
        assert_eq!(agreed(&mut h), ("68.8".into(), true));
        assert!(h.query_by_label(&format!("{} Confirm", icons::CHECK)).is_some());
        // Further apart: no average, and a warning.
        retype(&mut h, "66", "58");
        assert_eq!(agreed(&mut h), (String::new(), false));
        assert!(h.query_by_label(&format!("{} 18.9% from Marker 1", icons::WARNING)).is_some());
        shot(&mut h, "28b_marks_apart");
        retype(&mut h, "58", "65");
        assert_eq!(agreed(&mut h), ("68.3".into(), true));
        shot(&mut h, "28c_marks_to_confirm");
        h.get_by_label(&format!("{} Confirm", icons::CHECK)).click();
        h.run_steps(2);
        assert_eq!(agreed(&mut h), ("68.3".into(), false));
        assert!(h.query_by_label(&format!("{} Confirm", icons::CHECK)).is_none());
        session(&mut h).selected = Some(li);
        h.run_steps(2);
        assert!(!all_text(&h).contains("64.5"));
        // SUST: the SUST supervisor's marks, and the contact as for advisees.
        for label in ["Sent by SUST", "SUST mark", "My mark", "I contacted them", "Their reply"] {
            assert!(h.query_all_by_label(label).next().is_some(), "{label}");
        }
        // Clicking a hidden mark reveals them too.
        h.get_all_by_label(&"•".repeat(8)).next().unwrap().click();
        h.run_steps(2);
        assert!(all_text(&h).contains("64.5") && all_text(&h).contains("71.5"), "SUST mark and my own");
        shot(&mut h, "29_sust_contact_and_marks");
    }

    #[test]
    fn role_chips_jump_to_their_folded_sections() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        let mut h = create_and_unlock(&path, false);
        let ana = Student {
            name: "Ana Souza".into(),
            fyp: Some(FypRecord::default()),
            phd: Some(PhdRecord::default()),
            advisee: Some(AdviseeRecord::default()),
            ..Default::default()
        };
        let id = ana.id;
        {
            let s = session(&mut h);
            s.db.students.push(ana);
            s.view = View::List(Category::All);
            s.selected = Some(id);
        }
        h.run_steps(3);
        // Fold the details and the research section away.
        h.get_by_label(&format!("{} DETAILS", icons::CARET_DOWN)).click();
        h.run_steps(2);
        assert!(h.query_by_label("Full name").is_none() && h.query_by_label(&format!("{} DETAILS", icons::CARET_RIGHT)).is_some());
        h.set_size(egui::vec2(1360.0, 2400.0));
        h.run_steps(2);
        h.get_all_by_label_contains("Research supervision").next().unwrap().click();
        // Past the folding animation.
        h.run_steps(20);
        assert!(h.query_by_label(&format!("{} SUPERVISION MEETINGS", icons::CARET_DOWN)).is_none());
        // Its chip opens it again and brings it into view.
        h.set_size(egui::vec2(1360.0, 860.0));
        h.run_steps(2);
        h.get_by_label_contains(" PhD").click();
        h.run_steps(20);
        shot(&mut h, "25_role_chip_jump");
        assert!(h.query_by_label(&format!("{} SUPERVISION MEETINGS", icons::CARET_DOWN)).is_some(), "open again");
        let header = h.get_all_by_label_contains("Research supervision").next().unwrap().rect();
        assert!(header.top() < 300.0, "scrolled to the top: {header:?}");
    }

    #[test]
    fn external_student_on_a_brazilian_programme() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        let mut h = create_and_unlock(&path, false);
        let start = chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        let ana = {
            let s = session(&mut h);
            let mut p = ResearchGroup::PhdExternal.new_record();
            p.university = "UFRN".into();
            p.start = Some(start);
            let ana = Student {
                name: "Ana Souza".into(),
                phd: Some(p),
                ..Default::default()
            };
            let id = ana.id;
            s.db.students.push(ana);
            s.view = View::List(Category::Research(ResearchGroup::PhdExternal));
            s.selected = Some(id);
            id
        };
        h.set_size(egui::vec2(1360.0, 2400.0));
        h.run_steps(3);
        let phd = |h: &mut Harness<'_, SupervisorApp>| session(h).db.student(ana).unwrap().phd.clone().unwrap();
        // Section headings are shown in capitals, after the arrow that folds them.
        let heading = |title: &str| format!("{} {title}", icons::CARET_DOWN);
        assert!(h.query_by_label("Standard assessments of this university").is_some());
        assert!(h.query_by_label(&heading("GRANT")).is_none());
        // Their own university's number, in the list header and the details.
        // (list headers are shown in capitals).
        assert!(h.query_by_label("REGISTRATION #").is_some());
        assert!(h.query_by_label("Registration #").is_some());
        assert!(h.query_by_label("B NUMBER").is_none() && h.query_by_label("B Number").is_none());

        h.get_by_label("On a Brazilian programme").click();
        h.run_steps(3);
        let p = phd(&mut h);
        assert!(p.brazilian_programme() && !p.skip_standard);
        assert_eq!(p.brazil.level, BrLevel::AcademicDoctorate);
        for label in [heading("GRANT"), "Standard stages of the Brazilian programme".into(), heading("DEFENCE & AWARD"), "Deadline extension".into()] {
            assert!(h.query_by_label(&label).is_some(), "{label}");
        }
        assert!(h.query_by_label("Standard assessments of this university").is_none());
        assert!(h.query_by_label("Fee status").is_none());

        h.get_by_label(&format!("{} Fill in suggested deadlines", icons::CALENDAR_CHECK)).click();
        h.run_steps(3);
        let p = phd(&mut h);
        assert_eq!(p.brazil.defence_deadline.deadline, start.checked_add_months(chrono::Months::new(48)));
        assert_eq!(p.annual_reports.len(), 3);
        assert!(h.query_by_label("Thesis defence").is_some());
        shot(&mut h, "16_brazilian_external_detail");
        // A longer item name (another level) widens the column rather than
        // wrapping into the next row: the grid only grows to what its cells
        // measure, and wrapped text never measures wider.
        session(&mut h).db.student_mut(ana).unwrap().phd.as_mut().unwrap().brazil.level = BrLevel::ProfessionalMasters;
        h.run_steps(5);
        let reminder = h.get_by_label(&format!("{} reminder 01 Jan 2030", icons::BELL)).rect();
        let next = h.get_by_label("Final project defence").rect();
        assert!(reminder.bottom() <= next.top(), "{reminder:?} runs into {next:?}");
        shot(&mut h, "16b_brazilian_narrow");
        session(&mut h).db.student_mut(ana).unwrap().phd.as_mut().unwrap().brazil.level = BrLevel::AcademicDoctorate;

        // A CAPES grant gets the usual 48 months; a Zoom meeting.
        {
            let p = session(&mut h).db.student_mut(ana).unwrap().phd.as_mut().unwrap();
            p.brazil.grant.agency = "CAPES".into();
            p.suggest_br(&BrRule::default_for(BrLevel::AcademicDoctorate));
            p.meetings.push(Meeting {
                date: Some(start),
                online: true,
                platform: Platform::Zoom,
                ..Default::default()
            });
        }
        h.run_steps(3);
        assert!(h.query_by_label("Coordination for the Improvement of Higher Education Personnel").is_some());
        assert!(h.query_by_label("Sun 28 Feb 2030").is_some() || h.query_by_label_contains("28 Feb 2030").is_some());
        assert!(h.query_by_label("Zoom").is_some());
        shot(&mut h, "17_brazilian_grant_and_zoom");

        // Settings: the deadlines of each Brazilian level.
        session(&mut h).view = View::Settings;
        h.run_steps(3);
        assert!(h.query_by_label("BRAZILIAN PROGRAMMES").is_some());
        assert!(h.query_by_label("Professional doctorate").is_some());

        // Not external any more: back to the B number.
        {
            let s = session(&mut h);
            s.db.student_mut(ana).unwrap().phd.as_mut().unwrap().external = false;
            s.view = View::List(Category::Research(ResearchGroup::Phd));
        }
        h.run_steps(3);
        assert!(h.query_by_label("B NUMBER").is_some() && h.query_by_label("B Number").is_some());
        assert!(h.query_by_label("REGISTRATION #").is_none() && h.query_by_label("Registration #").is_none());
    }

    fn all_text(h: &Harness<'_, SupervisorApp>) -> String {
        h.root()
            .children_recursive()
            .flat_map(|n| [n.accesskit_node().label(), n.accesskit_node().value()])
            .flatten()
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn external_registration_number_is_hidden_until_revealed() {
        const CPF: &str = "529.982.247-25";
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        let mut h = create_and_unlock(&path, false);
        let external = |name: &str, number: &str| Student {
            name: name.into(),
            b_number: number.into(),
            phd: Some(ResearchGroup::PhdExternal.new_record()),
            ..Default::default()
        };
        let (ana, joao) = {
            let s = session(&mut h);
            let (ana, joao) = (external("Ana Souza", CPF), external("João Lima", "QQ123456C"));
            let ids = (ana.id, joao.id);
            s.db.students.extend([ana, joao]);
            s.view = View::List(Category::Research(ResearchGroup::PhdExternal));
            s.selected = Some(ids.0);
            ids
        };
        h.run_steps(3);
        assert!(!all_text(&h).contains(CPF), "the CPF is nowhere on screen");
        assert!(!all_text(&h).contains("QQ123456C"));
        // In the list, under the name and in the details.
        assert_eq!(h.query_all_by_label("•••.•••.•••-••").count(), 3);
        shot(&mut h, "18_registration_hidden");

        // Without the password option, one click reveals it.
        h.get_by_label(&format!("{} Reveal", icons::EYE)).click();
        h.run_steps(2);
        assert!(all_text(&h).contains(CPF));
        assert!(h.query_by_label_contains("This is a CPF").is_some());
        assert!(!all_text(&h).contains("QQ123456C"), "only this student's number");
        // Opening another student hides it again.
        session(&mut h).selected = Some(joao);
        h.run_steps(2);
        session(&mut h).selected = Some(ana);
        h.run_steps(2);
        assert!(!all_text(&h).contains(CPF), "hidden again after switching students");

        // With the option on, revealing asks for the password.
        session(&mut h).db.settings.reveal_needs_password = true;
        h.run_steps(1);
        h.get_by_label(&format!("{} Reveal", icons::EYE)).click();
        h.run_steps(2);
        assert!(h.query_by_label("Reveal registration number").is_some());
        shot(&mut h, "19_reveal_with_password");
        let field = h.get_by_role(AkRole::PasswordInput);
        field.focus();
        field.type_text("wrong-password-000");
        h.run_steps(1);
        h.get_by_label("Reveal").click();
        h.run_steps(2);
        assert!(h.query_by_label("Wrong password.").is_some());
        assert!(!all_text(&h).contains(CPF));
        let field = h.get_by_role(AkRole::PasswordInput);
        field.focus();
        field.type_text(PASSWORD);
        h.run_steps(1);
        h.get_by_label("Reveal").click();
        h.run_steps(2);
        assert!(h.query_by_label("Reveal registration number").is_none(), "dialog closed");
        assert!(all_text(&h).contains(CPF));
        h.get_by_label(&format!("{} Hide", icons::EYE_SLASH)).click();
        h.run_steps(2);
        assert!(!all_text(&h).contains(CPF));

        // Turning the option off shows the numbers everywhere.
        session(&mut h).db.settings.hide_external_ids = false;
        h.run_steps(2);
        assert!(all_text(&h).contains(CPF) && all_text(&h).contains("QQ123456C"));
        assert!(h.query_by_label_contains("National Insurance").is_none(), "João isn't open");
    }

    #[test]
    fn week_notes_open_in_a_dialog() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        let mut h = create_and_unlock(&path, false);
        let first = chrono::NaiveDate::from_ymd_opt(2026, 10, 29).unwrap();
        let id = {
            let s = session(&mut h);
            let student = Student {
                name: "Weekly Student".into(),
                fyp: Some(FypRecord {
                    programme: "BEng".into(),
                    meetings: vec![Meeting {
                        date: Some(first),
                        repeat: Some(Repeat {
                            until: Some(first + Days::weeks(2)),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            };
            let id = student.id;
            s.db.students.push(student);
            s.view = View::List(Category::Projects("BEng".into()));
            s.selected = Some(id);
            id
        };
        h.set_size(egui::vec2(1360.0, 2000.0));
        h.run_steps(3);
        h.get_by_label("Weeks (3)").click();
        h.run_steps(3);
        let add = format!("{} Add notes", icons::NOTE_PENCIL);
        assert_eq!(h.query_all_by_label(&add).count(), 3);
        h.get_all_by_label(&add).next().unwrap().click();
        h.run_steps(3);
        assert!(h.query_by_label("Meeting notes").is_some());
        // Ready to type straight away.
        h.event(egui::Event::Text("Talked about the literature review".into()));
        h.run_steps(2);
        shot(&mut h, "26_week_notes");
        h.get_by_label(&format!("Next week {}", icons::CARET_RIGHT)).click();
        h.run_steps(3);
        h.event(egui::Event::Text("Second week".into()));
        h.run_steps(2);
        h.get_by_label("Done").click();
        h.run_steps(3);
        assert!(h.query_by_label("Meeting notes").is_none(), "closed");
        let notes: Vec<String> = {
            let s = session(&mut h);
            let m = &s.db.student(id).unwrap().fyp.as_ref().unwrap().meetings[0];
            m.occurrences(&s.db.settings).into_iter().map(|o| o.notes).collect()
        };
        assert_eq!(notes, ["Talked about the literature review", "Second week", ""]);
        // The start of the notes shows in the list.
        let note = h.get_by_label(&format!("{} Talked about the literature review", icons::NOTE)).rect();
        let empty = h.get_by_label(&add).rect();
        assert_eq!(note.width(), empty.width(), "as wide as Add notes, however long the notes");
        shot(&mut h, "26b_week_notes_list");
    }

    #[test]
    fn shortcuts_button_lists_the_shortcuts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        let mut h = create_and_unlock(&path, false);
        h.get_by_label(icons::KEYBOARD).click();
        h.run_steps(3);
        assert!(h.query_by_label("Keyboard shortcuts").is_some());
        assert!(h.query_by_label("Save now").is_some());
        shot(&mut h, "27_shortcuts");
        h.get_by_label("Close").click();
        h.run_steps(3);
        assert!(h.query_by_label("Save now").is_none());
        h.key_press(egui::Key::F1);
        h.run_steps(3);
        assert!(h.query_by_label("Save now").is_some(), "F1 opens it too");
    }

    /// A project student with a meeting next Monday at 10:00, and semester 1
    /// teaching around it. The dates follow today, so the meeting is always to come.
    fn student_with_meeting(h: &mut Harness<'_, SupervisorApp>, time: Option<NaiveTime>) -> Uuid {
        let s = session(h);
        let today = widgets::today();
        s.db.settings.sem1_start = Some(s.db.settings.week_start(today));
        s.db.settings.sem2_start = Some(today + Days::weeks(26));
        let student = Student {
            name: "Timetabled Student".into(),
            fyp: Some(FypRecord {
                programme: "BEng".into(),
                meetings: vec![Meeting {
                    date: Some(next_monday()),
                    time,
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let id = student.id;
        s.db.students.push(student);
        s.view = View::List(Category::Projects("BEng".into()));
        s.selected = Some(id);
        id
    }

    fn next_monday() -> chrono::NaiveDate {
        let today = widgets::today();
        today + Days::days(7 - i64::from(today.weekday().num_days_from_monday()))
    }

    fn at(hour: u32, minute: u32) -> Option<NaiveTime> {
        NaiveTime::from_hms_opt(hour, minute, 0)
    }

    fn slots(h: &mut Harness<'_, SupervisorApp>, id: Uuid) -> Vec<BusySlot> {
        session(h).db.student(id).unwrap().timetable.s1.clone()
    }

    /// How many things on screen say `text`.
    fn shows(h: &Harness<'_, SupervisorApp>, text: &str) -> usize {
        h.query_all_by_label_contains(text).count()
    }

    fn slot_rect(h: &Harness<'_, SupervisorApp>, label: &str) -> egui::Rect {
        h.get_by_label(label).rect()
    }

    #[test]
    fn timetable_is_ticked_and_dragged_in_its_dialog() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        let mut h = create_and_unlock(&path, false);
        let id = student_with_meeting(&mut h, at(10, 0));
        h.run_steps(3);
        assert!(!all_text(&h).contains("Clashes"), "nothing to clash with yet");

        let name_shown = shows(&h, "Timetabled Student");
        h.get_by_label("Timetable").click();
        h.run_steps(3);
        assert!(h.query_by_label("Done").is_some());
        assert_eq!(shows(&h, "Timetabled Student"), name_shown + 1, "the dialog says whose it is");
        shot(&mut h, "30_timetable_empty");

        // A click ticks an hour, and another click clears it.
        h.get_by_label("Tue 11:15–12:00").click();
        h.run_steps(3);
        assert_eq!(slots(&mut h, id), [BusySlot { day: 1, hour: 11 }]);
        h.get_by_label("Tue 11:15–12:00").click();
        h.run_steps(3);
        assert!(slots(&mut h, id).is_empty());
        // Held down for a moment, it still only changes once.
        let press = slot_rect(&h, "Tue 11:15–12:00").center();
        h.hover_at(press);
        h.run_steps(1);
        h.drag_at(press);
        h.run_steps(3);
        h.drop_at(press);
        h.run_steps(3);
        assert_eq!(slots(&mut h, id), [BusySlot { day: 1, hour: 11 }]);
        // And Space does the same from the keyboard.
        h.get_by_label("Tue 11:15–12:00").focus();
        h.run_steps(1);
        h.key_press(egui::Key::Space);
        h.run_steps(3);
        assert!(slots(&mut h, id).is_empty());

        // A drag marks every hour it passes over, even when the pointer jumps.
        let from = slot_rect(&h, "Mon 09:15–10:00").center();
        let mid = slot_rect(&h, "Mon 10:15–11:00").center();
        let to = slot_rect(&h, "Mon 12:15–13:00").center();
        h.hover_at(from);
        h.run_steps(1);
        h.drag_at(from);
        h.run_steps(2);
        h.hover_at(mid);
        h.run_steps(2);
        h.hover_at(to);
        h.run_steps(2);
        h.drop_at(to);
        h.run_steps(3);
        let hours = |h: &mut Harness<'_, SupervisorApp>| slots(h, id).iter().map(|s| (s.day, s.hour)).collect::<Vec<_>>();
        assert_eq!(hours(&mut h), [(0, 9), (0, 10), (0, 11), (0, 12)]);
        assert!(h.query_by_label_contains("meeting still to come clashes").is_some(), "names the meeting it now clashes with");
        shot(&mut h, "31_timetable_filled");

        // Dragging from a busy hour clears instead.
        let from = slot_rect(&h, "Mon 12:15–13:00").center();
        let to = slot_rect(&h, "Mon 11:15–12:00").center();
        h.hover_at(from);
        h.run_steps(1);
        h.drag_at(from);
        h.run_steps(2);
        h.hover_at(to);
        h.run_steps(2);
        h.drop_at(to);
        h.run_steps(3);
        assert_eq!(hours(&mut h), [(0, 9), (0, 10)]);

        // Semester 2 has its own timetable, and can start from semester 1's.
        h.get_by_label("Semester 2").click();
        h.run_steps(3);
        h.get_by_label("Copy Semester 1").click();
        h.run_steps(3);
        assert_eq!(session(&mut h).db.student(id).unwrap().timetable.s2.len(), 2);
        h.get_by_label(&format!("{} Clear", icons::TRASH)).click();
        h.run_steps(3);
        assert!(session(&mut h).db.student(id).unwrap().timetable.s2.is_empty());
        assert_eq!(slots(&mut h, id).len(), 2, "semester 1 is untouched");

        h.get_by_label("Done").click();
        h.run_steps(3);
        assert!(h.query_by_label("Done").is_none(), "closed");
        // Back on the student: the meeting at 10:00 is in the way.
        assert!(h.query_by_label_contains("Clashes with the student's timetable: busy 09:15–11:00").is_some());
        h.set_size(egui::vec2(1360.0, 1900.0));
        shot(&mut h, "32_meeting_clash");

        // It is kept in the encrypted file.
        wait_until(&mut h, "saved", all_saved);
        let saved = storage::read_database(&path, PASSWORD, None).unwrap();
        assert_eq!(saved.student(id).unwrap().timetable.s1.len(), 2);
    }

    #[test]
    fn choosing_a_time_warns_of_a_clash_as_it_changes() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = create_and_unlock(&dir.path().join("data.svt"), false);
        let id = student_with_meeting(&mut h, None);
        let monday_nine = BusySlot { day: 0, hour: 9 };
        session(&mut h).db.student_mut(id).unwrap().timetable.s1.push(monday_nine);
        h.run_steps(3);
        // With only a date, it says when the student is busy that day.
        assert!(h.query_by_label_contains("The student is busy that day: 09:15–10:00").is_some());
        assert!(h.query_by_label_contains("Clashes").is_none());
        let set_time = |h: &mut Harness<'_, SupervisorApp>, time| {
            let s = session(h);
            s.db.student_mut(id).unwrap().fyp.as_mut().unwrap().meetings[0].time = time;
            h.run_steps(3);
        };
        set_time(&mut h, at(9, 30));
        assert!(h.query_by_label_contains("Clashes with the student's timetable: busy 09:15–10:00").is_some());
        set_time(&mut h, at(10, 0));
        assert!(h.query_by_label_contains("Clashes").is_none(), "free after the lecture");
        assert!(h.query_by_label_contains("busy that day").is_none(), "only until a time is chosen");

        // A repeating meeting is checked week by week.
        {
            let s = session(&mut h);
            let m = &mut s.db.student_mut(id).unwrap().fyp.as_mut().unwrap().meetings[0];
            m.time = at(9, 30);
            m.repeat = Some(Repeat { until: m.date.map(|d| d + Days::weeks(2)), ..Default::default() });
        }
        h.run_steps(3);
        assert!(h.query_by_label_contains("3 weeks clash with the student's timetable").is_some());
    }

    #[test]
    fn calendar_dialogs_warn_of_a_clash_and_dragging_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = create_and_unlock(&dir.path().join("data.svt"), false);
        let id = student_with_meeting(&mut h, at(9, 30));
        let monday = next_monday();
        {
            let s = session(&mut h);
            let student = s.db.student_mut(id).unwrap();
            student.timetable.s1.push(BusySlot { day: 0, hour: 9 });
            // The Tuesday, which is free; the Monday is in the same row of the grid.
            student.fyp.as_mut().unwrap().meetings[0].date = Some(monday + Days::days(1));
            s.view = View::Calendar;
            s.selected = None;
            s.calendar.layout = calendar::Layout::Month;
            s.calendar.month = monday.with_day(1).unwrap();
        }
        h.set_size(egui::vec2(1360.0, 1400.0));
        h.key_press(egui::Key::F20);
        h.run_steps(3);
        let alert = "clashes with the student's timetable: busy 09:15–10:00";
        let edit_dialog_warns = |h: &mut Harness<'_, SupervisorApp>| {
            h.get_by_label_contains("09:30").click_secondary();
            h.run_steps(3);
            h.get_by_label(&format!("{} Edit…", icons::PENCIL_SIMPLE)).click();
            h.run_steps(3);
            let warns = h.query_by_label_contains("Clashes with the student's timetable: busy 09:15–10:00").is_some();
            h.get_by_label("Cancel").click();
            h.run_steps(3);
            warns
        };
        assert!(!edit_dialog_warns(&mut h), "free on the Tuesday");

        // Drag it onto the Monday: the message says it now clashes.
        let from = h.get_by_label_contains("09:30").rect().center();
        let day = monday.day().to_string();
        let to = h
            .get_all_by_label(&day)
            .map(|n| n.rect().center())
            .find(|c| (c.y - from.y).abs() < 60.0)
            .expect("the Monday is in the same row")
            + egui::vec2(20.0, 30.0);
        h.drag_at(from);
        h.run_steps(1);
        h.hover_at(from + egui::vec2(10.0, 5.0));
        h.run_steps(1);
        h.hover_at(to);
        h.run_steps(1);
        h.drop_at(to);
        h.run_steps(3);
        let moved = session(&mut h).db.student(id).unwrap().fyp.as_ref().unwrap().meetings[0].date;
        assert_eq!(moved, Some(monday));
        assert!(h.query_by_label_contains(alert).is_some(), "the message warns");
        assert!(edit_dialog_warns(&mut h), "and so does the edit dialog");
    }

    #[test]
    fn calendar_shows_timetables_only_when_switched_on() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = create_and_unlock(&dir.path().join("data.svt"), false);
        let id = student_with_meeting(&mut h, at(12, 0));
        {
            let s = session(&mut h);
            s.selected = None;
            s.view = View::Calendar;
            s.calendar.layout = calendar::Layout::Month;
            s.calendar.month = next_monday().with_day(1).unwrap();
        }
        h.set_size(egui::vec2(1360.0, 1400.0));
        h.key_press(egui::Key::F20);
        h.run_steps(3);
        let pill = format!("{}  Timetables", icons::CLOCK);
        assert!(h.query_by_label(&pill).is_none(), "no timetables, no toggle");

        session(&mut h).db.student_mut(id).unwrap().timetable.s1.push(BusySlot { day: 0, hour: 9 });
        h.key_press(egui::Key::F20);
        h.run_steps(3);
        let busy = format!("{} 09:15–10:00 Timetabled Student", icons::CLOCK);
        assert!(!session(&mut h).calendar.timetables, "off to begin with");
        assert!(h.query_by_label(&pill).is_some());
        assert!(h.query_by_label_contains("09:15–10:00").is_none(), "nothing shows until it is on");

        h.get_by_label(&pill).click();
        h.run_steps(3);
        assert!(shows(&h, &busy) > 0, "shows in the month");
        shot(&mut h, "33_calendar_timetables_month");

        // They can't be selected, and aren't events: no export can include them.
        h.get_by_label("Select all shown").click();
        h.run_steps(3);
        let s = session(&mut h);
        assert_eq!(s.calendar.selected.len(), 1, "only the meeting");
        assert!(s.derived().events.iter().all(|e| !e.title.contains("09:15")));
        s.calendar.selected.clear();

        // In the list they go under each week.
        s.calendar.layout = calendar::Layout::List;
        s.calendar.range = calendar::Range::Upcoming;
        h.run_steps(3);
        assert!(shows(&h, "09:15–10:00 Timetabled Student") > 0, "shows in the list");
        shot(&mut h, "34_calendar_timetables_list");

        // Privacy mode: the times stay, the name goes.
        session(&mut h).private = true;
        h.run_steps(3);
        assert_eq!(shows(&h, "Timetabled Student"), 0);
        assert!(shows(&h, "09:15–10:00") > 0);
        session(&mut h).private = false;

        // Off again when the app next opens.
        assert!(!CalendarState::default().timetables);
    }

    #[test]
    fn calendar_narrows_timetables_to_one_student() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = create_and_unlock(&dir.path().join("data.svt"), false);
        let id = student_with_meeting(&mut h, None);
        let other = Student {
            name: "Second Student".into(),
            timetable: Timetable { s1: vec![BusySlot { day: 0, hour: 11 }], s2: Vec::new() },
            ..Default::default()
        };
        let other_id = other.id;
        {
            let s = session(&mut h);
            s.db.students.push(other);
            s.db.student_mut(id).unwrap().timetable.s1.push(BusySlot { day: 0, hour: 9 });
            s.selected = None;
            s.view = View::Calendar;
            s.calendar.layout = calendar::Layout::Month;
            s.calendar.month = next_monday().with_day(1).unwrap();
            s.calendar.timetables = true;
        }
        h.set_size(egui::vec2(1360.0, 1400.0));
        h.key_press(egui::Key::F20);
        h.run_steps(3);
        assert!(shows(&h, "09:15–10:00 Timetabled Student") > 0);
        assert!(shows(&h, "11:15–12:00 Second Student") > 0);
        session(&mut h).calendar.timetable_of = Some(other_id);
        h.run_steps(3);
        assert_eq!(shows(&h, "09:15–10:00 Timetabled Student"), 0);
        assert!(shows(&h, "11:15–12:00 Second Student") > 0);
        // Forgets a student who no longer has a timetable.
        session(&mut h).db.student_mut(other_id).unwrap().timetable = Timetable::default();
        h.key_press(egui::Key::F20);
        h.run_steps(3);
        assert_eq!(session(&mut h).calendar.timetable_of, None);
    }

    #[test]
    fn timetable_hides_the_name_in_privacy_mode() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = create_and_unlock(&dir.path().join("data.svt"), false);
        student_with_meeting(&mut h, None);
        session(&mut h).private = true;
        h.run_steps(3);
        h.get_by_label("Timetable").click();
        h.run_steps(3);
        assert!(h.query_by_label("Done").is_some());
        assert!(h.query_by_label_contains("Timetabled Student").is_none());
    }

    #[test]
    fn meeting_title_and_buttons_never_overlap() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = create_and_unlock(&dir.path().join("data.svt"), false);
        let id = student_with_meeting(&mut h, at(9, 30));
        {
            let s = session(&mut h);
            let m = &mut s.db.student_mut(id).unwrap().fyp.as_mut().unwrap().meetings[0];
            m.repeat = Some(Repeat { every_weeks: 3, until: m.date, ..Default::default() });
        }
        h.set_size(egui::vec2(1360.0, 2000.0));
        h.run_steps(3);
        let check = |h: &Harness<'_, SupervisorApp>, width: &str| {
            let chip = h.get_by_label_contains("every 3 weeks").rect();
            let google = h.get_by_label_contains("Google Calendar").rect();
            let invite = h.get_by_label_contains("Invite file").rect();
            let remove = h.get_by_label(icons::TRASH).rect();
            for (name, other) in [("Google Calendar", google), ("Invite file", invite), ("remove", remove)] {
                assert!(!chip.intersects(other), "{width}: the chip runs under {name}");
            }
            assert!(!google.intersects(invite), "{width}");
            assert!(invite.right() <= remove.right() + 1.0, "{width}: stays inside the meeting");
        };
        check(&h, "usual panel");
        shot(&mut h, "35_meeting_row");
    }

    /// Where the student's name starts in the details panel, which moves left
    /// as the panel widens. Panels grow but don't shrink back, so each record
    /// is measured in a new window.
    fn panel_growth(name_of_shot: &str, add: impl FnOnce(&mut Student)) -> f32 {
        let dir = tempfile::tempdir().unwrap();
        let mut h = create_and_unlock(&dir.path().join("data.svt"), false);
        let id = student_with_meeting(&mut h, None);
        h.set_size(egui::vec2(1360.0, 3400.0));
        let name = |h: &Harness<'_, SupervisorApp>| h.get_all_by_label("Timetabled Student").map(|n| n.rect().left()).fold(0.0, f32::max);
        session(&mut h).db.student_mut(id).unwrap().fyp = None;
        h.run_steps(4);
        let usual = name(&h);
        let student = session(&mut h).db.student_mut(id).unwrap();
        student.timetable.s1.push(BusySlot { day: 0, hour: 9 });
        add(student);
        h.run_steps(4);
        shot(&mut h, name_of_shot);
        usual - name(&h)
    }

    #[test]
    fn visit_and_viva_rows_keep_the_panel_its_usual_width() {
        // On the busy hour, so the clash warning shows as well.
        let (date, time) = (Some(next_monday()), at(9, 30));
        let visit = panel_growth("37_visit_row", |s| {
            let mut placement = PlacementRecord::default();
            placement.visits[0].date = date;
            placement.visits[0].time = time;
            s.placement = Some(placement);
        });
        let no_visit = panel_growth("37b_no_visit", |s| s.placement = Some(PlacementRecord::default()));
        assert!(visit <= no_visit + 1.0, "a visit widened the panel by {} more", visit - no_visit);
        let with_viva = panel_growth("37c_viva_row", |s| {
            let mut phd = PhdRecord::default();
            phd.viva = date;
            phd.viva_time = time;
            s.phd = Some(phd);
        });
        let without_viva = panel_growth("37d_no_viva", |s| s.phd = Some(PhdRecord::default()));
        assert!(with_viva <= without_viva + 1.0, "a viva widened the panel by {} more", with_viva - without_viva);
    }

    #[test]
    fn backup_copy_is_set_in_settings_and_problems_are_announced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        let usb = dir.path().join("usb");
        std::fs::create_dir_all(&usb).unwrap();
        let mut h = create_and_unlock(&path, false);
        session(&mut h).view = View::Settings;
        h.set_size(egui::vec2(1360.0, 4200.0));
        h.run_steps(3);
        assert!(h.query_by_label_contains("Off – the only backup").is_some());
        assert!(h.query_by_label(&format!("{} Choose folder…", icons::FLOPPY_DISK)).is_some());
        shot(&mut h, "38_backup_off");

        // The folder picker can't run here; what it hands over is the same.
        wait_until(&mut h, "saved", all_saved);
        let s = session(&mut h);
        s.vault.set_backup_folder(Some(&usb), &s.db).unwrap();
        h.run_steps(3);
        let file = session(&mut h).vault.backup_copy().unwrap().file.clone();
        assert!(h.query_by_label_contains("On – every save also goes to").is_some());
        assert!(h.query_by_label_contains(&file.display().to_string()).is_some());
        assert!(h.query_by_label_contains("Last written").is_some());
        shot(&mut h, "39_backup_on");

        // An edit reaches the copy with the autosave.
        session(&mut h).db.settings.my_name = "Edited".into();
        h.key_press(egui::Key::F20);
        h.run_steps(1);
        wait_until(&mut h, "saved", all_saved);
        assert_eq!(storage::read_database(&file, PASSWORD, None).unwrap().settings.my_name, "Edited");

        // The folder goes away: the data still saves, and it says so.
        let unplugged = dir.path().join("unplugged");
        std::fs::rename(&usb, &unplugged).unwrap();
        session(&mut h).db.settings.my_name = "Without the copy".into();
        h.key_press(egui::Key::F20);
        h.run_steps(1);
        wait_until(&mut h, "saved", all_saved);
        h.run_steps(3);
        assert!(matches!(session(&mut h).status, SaveStatus::Saved), "saving isn't held up");
        assert_eq!(storage::read_database(&path, PASSWORD, None).unwrap().settings.my_name, "Without the copy");
        assert!(h.query_by_label_contains("The backup copy couldn't be saved").is_some(), "a message says so");
        assert!(h.query_by_label_contains("The copy couldn't be saved").is_some(), "and Settings");
        let warning = format!("{} Saved", icons::WARNING);
        assert!(h.query_by_label(&warning).is_some(), "and the warning stays in the sidebar");
        shot(&mut h, "40_backup_problem");

        // Back again: Back up now puts it right.
        std::fs::rename(&unplugged, &usb).unwrap();
        h.get_by_label(&format!("{} Back up now", icons::FLOPPY_DISK)).click();
        h.run_steps(3);
        assert_eq!(storage::read_database(&file, PASSWORD, None).unwrap().settings.my_name, "Without the copy");
        assert!(h.query_by_label(&warning).is_none());
        assert!(h.query_by_label("Copy saved.").is_some());
        assert!(h.query_by_label_contains("saving again").is_some());

        h.get_by_label("Turn off").click();
        h.run_steps(3);
        assert!(h.query_by_label_contains("Off – the only backup").is_some());
        assert!(h.query_by_label_contains("The copy stays at").is_some());
        assert!(file.exists());
    }

    /// A repeating meeting's student, open on the calendar with its item selected.
    fn calendar_with_selection(path: &std::path::Path) -> Harness<'static, SupervisorApp> {
        let mut h = create_and_unlock(path, false);
        let first = chrono::NaiveDate::from_ymd_opt(2026, 10, 29).unwrap();
        {
            let s = session(&mut h);
            let student = Student {
                name: "Weekly Student".into(),
                fyp: Some(FypRecord {
                    programme: "BEng".into(),
                    meetings: vec![Meeting {
                        date: Some(first),
                        repeat: Some(Repeat {
                            until: Some(first + Days::weeks(2)),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            };
            s.selected = Some(student.id);
            s.db.students.push(student);
            s.view = View::Calendar;
        }
        h.set_size(egui::vec2(1360.0, 2000.0));
        h.key_press(egui::Key::F20);
        h.run_steps(3);
        let s = session(&mut h);
        let uid = s.derived().events.iter().find(|e| e.editable() && !e.cancelled).unwrap().uid.clone();
        s.calendar.selected.insert(uid);
        h.run_steps(3);
        assert_eq!(session(&mut h).calendar.selected.len(), 1);
        h
    }

    #[test]
    fn calendar_ignores_keys_behind_the_shortcuts() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = calendar_with_selection(&dir.path().join("data.svt"));
        h.key_press(egui::Key::F1);
        h.run_steps(3);
        h.key_press(egui::Key::Delete);
        h.run_steps(3);
        assert!(h.query_by_label("Delete this item?").is_none(), "Delete stays behind the list");
        h.key_press(egui::Key::Escape);
        h.run_steps(3);
        assert!(h.query_by_label("Save now").is_none(), "Esc closes the list");
        assert_eq!(session(&mut h).calendar.selected.len(), 1, "and leaves the selection");
        // With the list closed, the keys reach the calendar again.
        h.key_press(egui::Key::Delete);
        h.run_steps(3);
        assert!(h.query_by_label("Delete this item?").is_some());
    }

    #[test]
    fn calendar_ignores_keys_behind_the_week_notes() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = calendar_with_selection(&dir.path().join("data.svt"));
        h.get_by_label("Weeks (3)").click();
        h.run_steps(3);
        h.get_all_by_label(&format!("{} Add notes", icons::NOTE_PENCIL)).next().unwrap().click();
        h.run_steps(3);
        // Out of the text box, keys are no longer taken by it.
        h.get_by_label("Meeting notes").click();
        h.run_steps(3);
        h.key_press(egui::Key::Delete);
        h.run_steps(3);
        assert!(h.query_by_label("Delete this item?").is_none(), "Delete stays behind the notes");
        h.key_press(egui::Key::Escape);
        h.run_steps(3);
        assert!(h.query_by_label("Meeting notes").is_none(), "Esc closes the notes");
        assert_eq!(session(&mut h).calendar.selected.len(), 1, "and leaves the selection");
    }

    #[test]
    fn week_notes_take_the_cursor_when_reopened() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = calendar_with_selection(&dir.path().join("data.svt"));
        h.get_by_label("Weeks (3)").click();
        h.run_steps(3);
        let add = format!("{} Add notes", icons::NOTE_PENCIL);
        for _ in 0..2 {
            h.get_all_by_label(&add).next().unwrap().click();
            h.run_steps(3);
            assert!(h.ctx.egui_wants_keyboard_input(), "the cursor is in the notes");
            h.get_by_label("Done").click();
            h.run_steps(3);
            assert!(h.query_by_label("Meeting notes").is_none());
        }
        // Typing goes in the box on a second opening.
        h.get_all_by_label(&add).next().unwrap().click();
        h.run_steps(3);
        h.event(egui::Event::Text("Typed after reopening".into()));
        h.run_steps(2);
        h.get_by_label("Done").click();
        h.run_steps(3);
        assert!(h.query_by_label(&format!("{} Typed after reopening", icons::NOTE)).is_some());
    }

    fn modules_title() -> String {
        format!("{}  Modules", icons::BOOK_OPEN_TEXT)
    }

    fn failed_module(component: FailedComponent) -> FailedModule {
        FailedModule {
            code: "EEE526".into(),
            name: "Advanced Structural Dynamics and Vibration Analysis of Composite Assemblies for Aerospace".into(),
            component,
            cw_mark: "38.5".into(),
            ex_mark: "31.25".into(),
            discussed: false,
        }
    }

    /// Left edge of the detail panel, which moves if its contents stretch it.
    fn panel_left(h: &Harness<'_, SupervisorApp>) -> f32 {
        h.get_by_label(&format!("{} Delete student…", icons::TRASH)).rect().left()
    }

    #[test]
    fn modules_card_is_collapsed_and_never_stretches_the_panel() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        let mut h = create_and_unlock(&path, false);
        let student = Student {
            name: "Aisha Khan".into(),
            advisee: Some(AdviseeRecord::default()),
            modules: PendingModules {
                s1: SemesterModules {
                    check: ModulesCheck::Pending,
                    failed: vec![failed_module(FailedComponent::CwEx), failed_module(FailedComponent::Cw)],
                },
                s2: SemesterModules {
                    check: ModulesCheck::Pending,
                    failed: vec![failed_module(FailedComponent::Ex)],
                },
            },
            ..Default::default()
        };
        let id = student.id;
        {
            let s = session(&mut h);
            s.db.students.push(student);
            s.view = View::List(Category::Advisee);
            s.selected = Some(id);
        }
        h.set_size(egui::vec2(1360.0, 2400.0));
        h.run_steps(3);
        // Collapsed by default, with what is pending in its title.
        assert!(h.query_by_label_contains("Module 1").is_none());
        let before = panel_left(&h);
        let title = format!("{} · S1: 2 pending · S2: 1 pending", modules_title());
        h.get_by_label(&title).click();
        h.run_steps(4);
        assert_eq!(h.query_all_by_label("Module 1").count(), 2, "both semesters are open");
        assert_eq!(panel_left(&h), before, "still as wide");
        assert!(!all_text(&h).contains("38.5"), "marks are dots until revealed");
        h.get_all_by_label(&format!("{} Reveal marks", icons::EYE)).next().unwrap().click();
        h.run_steps(4);
        assert!(all_text(&h).contains("38.5") && all_text(&h).contains("31.25"));
        assert_eq!(panel_left(&h), before, "still as wide with the marks shown");
        // CW+EX has both marks, CW and EX one each.
        assert_eq!(h.query_all_by_label("CW mark").count(), 2);
        assert_eq!(h.query_all_by_label("EX mark").count(), 2);
        // Asked once, in semester 2, as the student is an advisee.
        let discussed = "Discussed with them whilst my advisee";
        assert_eq!(h.query_all_by_label(discussed).count(), 1);
        h.get_by_label(discussed).click();
        h.run_steps(2);
        let saved = session(&mut h).db.student(id).unwrap().modules.clone();
        assert!(saved.s2.failed[0].discussed && !saved.s1.failed[0].discussed);
        // Who to talk to follows the number of modules.
        assert!(h.query_by_label_contains("Needs to talk to the course director").is_some());
        assert!(h.query_by_label_contains("Needs to talk to their advisor").is_some());
        shot(&mut h, "41_modules_card");

        // At the narrowest the panel goes, the fields still fit.
        session(&mut h).db.student_mut(id).unwrap().modules.s2.failed.clear();
        h.set_size(egui::vec2(760.0, 2400.0));
        h.run_steps(4);
        for field in h.get_all_by_role(AkRole::TextInput) {
            assert!(field.rect().right() <= 760.0, "{:?} runs past the window", field.rect());
        }
        shot(&mut h, "42_modules_card_narrow");
    }

    #[test]
    fn modules_card_asks_how_many_and_only_for_local_students() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        let mut h = create_and_unlock(&path, false);
        let fyp = |programme: &str| FypRecord { programme: programme.into(), ..Default::default() };
        let (project, sust, external, research) = {
            let s = session(&mut h);
            let project = Student { name: "Project".into(), fyp: Some(fyp("BEng")), ..Default::default() };
            let sust = Student { name: "Sust".into(), fyp: Some(fyp("Sust")), advisee: Some(AdviseeRecord::default()), ..Default::default() };
            let external = Student { name: "External".into(), phd: Some(ResearchGroup::PhdExternal.new_record()), ..Default::default() };
            let research = Student { name: "Research".into(), phd: Some(ResearchGroup::Phd.new_record()), ..Default::default() };
            let ids = (project.id, sust.id, external.id, research.id);
            s.db.students.extend([project, sust, external, research]);
            s.view = View::List(Category::Projects("BEng".into()));
            s.selected = Some(ids.0);
            ids
        };
        h.set_size(egui::vec2(1360.0, 2400.0));
        h.run_steps(3);
        // A project student: the card and RAR, no year, and no advisee question.
        assert!(h.query_by_label("RAR").is_some());
        assert!(h.query_by_label("Year").is_none());
        h.get_by_label(&modules_title()).click();
        h.run_steps(4);
        assert!(h.query_by_label_contains("Discussed with them").is_none());

        // "All passed" asks for nothing more; "Pending" opens one module.
        h.get_all_by_label("All passed").next().unwrap().click();
        h.run_steps(3);
        assert!(h.query_by_label("How many").is_none());
        h.get_all_by_label("Pending").next().unwrap().click();
        h.run_steps(3);
        assert!(h.query_by_label("How many").is_some());
        assert_eq!(h.query_all_by_label("Module 1").count(), 1);
        // The count gives that many modules, and removing a blank one is fine.
        h.get_by_label(icons::PLUS).click();
        h.run_steps(3);
        h.get_by_label(icons::PLUS).click();
        h.run_steps(3);
        assert_eq!(session(&mut h).db.student(project).unwrap().modules.s1.pending(), 3);
        assert!(h.query_by_label_contains("Needs to talk to the Head of School").is_some());
        h.get_by_label("−").click();
        h.run_steps(3);
        assert_eq!(session(&mut h).db.student(project).unwrap().modules.s1.pending(), 2);
        // ...but one with details can't go by accident.
        session(&mut h).db.student_mut(project).unwrap().modules.s1.failed[1].code = "EEE526".into();
        h.run_steps(3);
        h.get_by_label("−").click();
        h.run_steps(3);
        assert_eq!(session(&mut h).db.student(project).unwrap().modules.s1.pending(), 2);

        // Removing the only role that shows them removes the modules too.
        h.set_size(egui::vec2(1360.0, 3000.0));
        h.run_steps(3);
        h.get_by_label("Remove Project supervision record…").click();
        h.run_steps(3);
        h.get_by_label("Delete").click();
        h.run_steps(3);
        assert_eq!(session(&mut h).db.student(project).unwrap().modules, PendingModules::default());

        // Not on the SUST programme or external; for a local research student, only RAR.
        session(&mut h).selected = Some(sust);
        h.run_steps(3);
        assert!(h.query_by_label("RAR").is_none() && h.query_by_label("Year").is_none());
        assert!(h.query_by_label(&modules_title()).is_none());
        session(&mut h).selected = Some(external);
        h.run_steps(3);
        assert!(h.query_by_label("RAR").is_none());
        session(&mut h).selected = Some(research);
        h.run_steps(3);
        assert!(h.query_by_label("RAR").is_some());
        assert!(h.query_by_label(&modules_title()).is_none());
    }

    #[test]
    fn advisee_year_is_chosen_in_the_details() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        let mut h = create_and_unlock(&path, false);
        let student = Student { name: "Year One".into(), advisee: Some(AdviseeRecord::default()), ..Default::default() };
        let id = student.id;
        {
            let s = session(&mut h);
            s.db.students.push(student);
            s.view = View::List(Category::Advisee);
            s.selected = Some(id);
        }
        h.run_steps(3);
        assert!(h.query_by_label("3rd").is_none());
        h.get_by_label("4th").click();
        h.run_steps(2);
        assert_eq!(session(&mut h).db.student(id).unwrap().advisee.as_ref().unwrap().year, Some(StudyYear::Fourth));
    }

    #[test]
    fn students_are_imported_from_a_spreadsheet() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        let mut h = create_and_unlock(&path, false);
        let aisha = Student {
            b_number: "B00111111".into(),
            name: "Aisha Khan".into(),
            advisee: Some(AdviseeRecord::default()),
            ..Default::default()
        };
        let ana = Student { b_number: "B00333333".into(), fyp: Some(FypRecord::default()), ..Default::default() };
        let aisha_id = aisha.id;
        {
            let s = session(&mut h);
            s.db.settings.my_email = "me@ulster.ac.uk".into();
            s.db.students.extend([aisha, ana]);
            s.view = View::List(Category::Advisee);
        }
        h.run_steps(3);
        assert!(h.query_by_label_contains("Import…").is_some(), "advisees too");
        session(&mut h).view = View::List(Category::Projects("SUST".into()));
        h.run_steps(3);
        assert!(h.query_by_label_contains("Import…").is_some(), "SUST students too");
        session(&mut h).view = View::List(Category::Projects("BEng".into()));
        h.run_steps(3);
        assert!(h.query_by_label_contains("Import…").is_some());

        let cells: Vec<Vec<String>> = [
            ["B Number", "Name", "Project Title", "Supervisor Email", "Supervisor Name"],
            ["B00111111", "Khan, Aisha", "Drone arm", "me@ulster.ac.uk", "Me"],
            ["B00222222", "Murphy, Sean", "Wind tunnel", "other@ulster.ac.uk", "Other"],
            ["B00333333", "Silva, Ana", "Heat pipes", "me@ulster.ac.uk", "Me"],
            ["B00444444", "Brown, Tom", "Gearbox", "ME@ulster.ac.uk", "Me"],
        ]
        .iter()
        .map(|r| r.iter().map(|c| c.to_string()).collect())
        .collect();
        {
            let s = session(&mut h);
            let sheet = crate::sheet_import::students(&cells).unwrap();
            let kind = crate::sheet_import::SheetKind::Projects;
            s.student_import = Some((StudentImport::new(kind, "allocations.xlsx".into(), sheet, &s.db, Some("BEng")), kind));
        }
        h.run_steps(3);
        // Only the rows with your email show, and those not yet in the list are
        // ticked, as is Ana, in the list already but without her name and title.
        assert!(h.query_by_label_contains("3 of the 4 rows have you (me@ulster.ac.uk)").is_some());
        assert!(h.query_by_label("2 students to add, 1 to fill in").is_some());
        assert!(h.query_by_label("Fills in details").is_some());
        assert!(h.query_by_label("In another list").is_some());
        assert!(h.query_by_label("Sean Murphy").is_none());
        h.get_by_value("You (3)").click();
        h.run_steps(2);
        h.get_by_label("Everyone (4)").click();
        h.run_steps(2);
        assert!(h.query_by_label("Sean Murphy").is_some());
        assert!(h.query_by_label("2 students to add, 1 to fill in").is_some(), "the ticks are kept");
        shot(&mut h, "student_import");
        h.get_by_label("Add 3 students").click();
        h.run_steps(3);
        assert!(h.query_by_label("Added 1 student, and 1 already in Supervisor; filled in details of 1 student").is_some());
        let s = session(&mut h);
        assert!(s.student_import.is_none());
        assert_eq!(s.db.students.len(), 3, "Sean was not ticked");
        let fyp = s.db.student(aisha_id).unwrap().fyp.as_ref().unwrap();
        assert_eq!((fyp.programme.as_str(), fyp.project_title.as_str()), ("BEng", "Drone arm"));
        let tom = s.db.students.iter().find(|s| s.b_number == "B00444444").unwrap();
        assert_eq!(tom.name, "Tom Brown");
        let ana = s.db.students.iter().find(|s| s.b_number == "B00333333").unwrap();
        assert_eq!((ana.name.as_str(), ana.fyp.as_ref().unwrap().project_title.as_str()), ("Ana Silva", "Heat pipes"));
    }

    #[test]
    fn import_finds_you_by_name_or_lets_you_choose_the_supervisor() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        let mut h = create_and_unlock(&path, false);
        // The SUST allocations: names only, no emails.
        let cells: Vec<Vec<String>> = [
            ["Student ID", "Student names", "Project Title", "SUST Supervisor", "UU supervisor"],
            ["B01000001", "Wei Li", "Solar still", "Dongxu Zhang", "Machado, Dr Gabriel"],
            ["B01000002", "Fang Chen", "Gearbox", "Tao Jiang", "Ralph, Dr Calvin"],
            ["B01000003", "Min Wu", "Drone", "Tao Jiang", "Ralph, Dr Calvin"],
        ]
        .iter()
        .map(|r| r.iter().map(|c| c.to_string()).collect())
        .collect();
        let open = |h: &mut Harness<'_, SupervisorApp>| {
            let s = session(h);
            let sheet = crate::sheet_import::import(std::slice::from_ref(&cells)).unwrap();
            let kind = crate::sheet_import::SheetKind::Projects;
            s.student_import = Some((StudentImport::new(kind, "sust.xlsx".into(), sheet, &s.db, Some("SUST")), kind));
            h.run_steps(3);
        };
        session(&mut h).db.settings.my_name = "Gabriel Machado".into();
        open(&mut h);
        assert!(h.query_by_label_contains("1 of the 3 rows have you (Gabriel Machado)").is_some());
        assert!(h.query_by_label("1 student to add").is_some());
        session(&mut h).student_import = None;

        // A name the sheet spells differently: choose the supervisor by hand.
        session(&mut h).db.settings.my_name = "Cal Ralph".into();
        open(&mut h);
        assert!(h.query_by_label_contains("No row has you (Cal Ralph)").is_some());
        assert!(h.query_by_label("Nothing ticked").is_some());
        h.get_by_value("Everyone (3)").click();
        h.run_steps(2);
        h.get_by_label("Ralph, Dr Calvin (2)").click();
        h.run_steps(3);
        assert!(h.query_by_label("2 students to add").is_some());
        assert!(h.query_by_label("Wei Li").is_none(), "only the chosen supervisor's rows");
        h.get_by_label("Add 2 students").click();
        h.run_steps(3);
        let names: Vec<String> = session(&mut h).db.students.iter().map(|s| s.name.clone()).collect();
        assert_eq!(names, ["Fang Chen", "Min Wu"]);
    }

    #[test]
    fn rar_notes_are_behind_the_password_every_time() {
        const SECRET: &str = "Extra time in examinations";
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        let mut h = create_and_unlock(&path, false);
        let student = Student { name: "Local Student".into(), advisee: Some(AdviseeRecord::default()), ..Default::default() };
        let id = student.id;
        {
            let s = session(&mut h);
            s.db.students.push(student);
            s.view = View::List(Category::Advisee);
            s.selected = Some(id);
        }
        h.run_steps(3);
        // The button only shows once ticked.
        assert!(h.query_by_label("RAR Notes").is_none());
        session(&mut h).db.student_mut(id).unwrap().rar = true;
        h.run_steps(2);

        let open = |h: &mut Harness<'_, SupervisorApp>, password: &str| {
            let field = h.get_by_role(AkRole::PasswordInput);
            field.focus();
            field.type_text(password);
            h.run_steps(1);
            h.get_by_label("Open").click();
            h.run_steps(3);
        };
        h.get_by_label("RAR Notes").click();
        h.run_steps(3);
        assert!(h.query_by_role(AkRole::PasswordInput).is_some(), "asks even with the privacy option off");
        open(&mut h, "wrong-password-000");
        assert!(h.query_by_label("Wrong password.").is_some());
        open(&mut h, PASSWORD);
        assert!(h.query_by_role(AkRole::PasswordInput).is_none());
        // Ready to type straight away, like the meeting notes.
        h.event(egui::Event::Text(SECRET.into()));
        h.run_steps(2);
        assert!(all_text(&h).contains(SECRET));
        shot(&mut h, "43_rar_notes");
        h.get_by_label("Done").click();
        h.run_steps(3);
        assert_eq!(session(&mut h).db.student(id).unwrap().rar_notes, SECRET);
        assert!(!all_text(&h).contains(SECRET), "not on screen once closed");

        // Asked for again the next time, with the same notes after it.
        h.get_by_label("RAR Notes").click();
        h.run_steps(3);
        assert!(!all_text(&h).contains(SECRET));
        open(&mut h, PASSWORD);
        assert!(all_text(&h).contains(SECRET));
        h.get_by_label("Done").click();
        h.run_steps(3);

        // In privacy mode, not even whether it is ticked shows.
        session(&mut h).private = true;
        h.run_steps(3);
        assert!(h.query_by_label("RAR").is_some());
        assert!(h.query_by_label("RAR Notes").is_none());
        session(&mut h).private = false;
        h.run_steps(3);

        // Saved encrypted with the rest, and in no calendar file.
        h.get_by_label(&format!("{}  Lock", icons::LOCK_SIMPLE)).click();
        wait_until(&mut h, "locked", locked);
        let bytes = std::fs::read(&path).unwrap();
        assert!(!bytes.windows(SECRET.len()).any(|w| w == SECRET.as_bytes()));
        let saved = storage::read_database(&path, PASSWORD, None).unwrap();
        assert_eq!(saved.student(id).unwrap().rar_notes, SECRET);
    }

    fn with_open_advisee(path: &std::path::Path, record: AdviseeRecord) -> (Harness<'static, SupervisorApp>, Uuid) {
        let mut h = create_and_unlock(path, false);
        let student = Student { name: "Open Student".into(), advisee: Some(record), ..Default::default() };
        let id = student.id;
        {
            let s = session(&mut h);
            s.db.students.push(student);
            s.view = View::List(Category::Advisee);
            s.selected = Some(id);
        }
        h.set_size(egui::vec2(1360.0, 1600.0));
        h.run_steps(3);
        (h, id)
    }

    #[test]
    fn esc_leaves_what_is_open_then_closes_the_student() {
        let dir = tempfile::tempdir().unwrap();
        let contact = SemesterContact { contacted_on: Some(crate::ui::widgets::today()), ..Default::default() };
        let (mut h, id) = with_open_advisee(&dir.path().join("data.svt"), AdviseeRecord { s1: contact, ..Default::default() });
        let press = |h: &mut Harness<'_, SupervisorApp>| {
            h.key_press(egui::Key::Escape);
            h.run_steps(3);
        };
        let open = |h: &mut Harness<'_, SupervisorApp>| session(h).selected.is_some();

        // In a field: the first Esc leaves it, and the student stays open.
        h.get_all_by_role(AkRole::TextInput).find(|f| f.value().as_deref() == Some("Open Student")).unwrap().focus();
        h.run_steps(3);
        assert!(h.ctx.memory(|m| m.focused().is_some()));
        press(&mut h);
        assert!(h.ctx.memory(|m| m.focused().is_none()), "out of the field");
        assert!(open(&mut h), "the student stays open");

        // A date picker: Esc closes it first.
        let date = crate::ui::widgets::today().format("%a %d %b %Y").to_string();
        h.get_all_by_label_contains(&date).next().unwrap().click();
        h.run_steps(3);
        assert!(h.query_by_label("Save").is_some(), "picker open");
        press(&mut h);
        assert!(h.query_by_label("Save").is_none(), "picker closed");
        assert!(open(&mut h), "the student stays open");

        // A dialog: Esc closes it first.
        h.get_by_label("Timetable").click();
        h.run_steps(3);
        assert!(session(&mut h).timetable.is_some());
        press(&mut h);
        assert!(session(&mut h).timetable.is_none());
        assert!(open(&mut h), "the student stays open");

        // With nothing else, it closes the student, as the X does.
        press(&mut h);
        assert!(!open(&mut h));
        assert!(session(&mut h).db.student(id).is_some(), "only the view closed");

        // In the calendar, the selection goes first.
        let dir = tempfile::tempdir().unwrap();
        let mut h = calendar_with_selection(&dir.path().join("data.svt"));
        let any = session(&mut h).db.students[0].id;
        session(&mut h).selected = Some(any);
        h.run_steps(3);
        press(&mut h);
        assert!(session(&mut h).calendar.selected.is_empty());
        assert!(open(&mut h), "the student stays open");
        press(&mut h);
        assert!(!open(&mut h));
    }

    /// Opens the date picker showing `date`, moves it a day on, and saves.
    fn pick_next_day(h: &mut Harness<'_, SupervisorApp>, date: chrono::NaiveDate) {
        h.get_all_by_label_contains(&date.format("%a %d %b %Y").to_string()).next().unwrap().click();
        h.run_steps(3);
        h.get_by_label(">").click();
        h.run_steps(3);
        h.get_by_label("Save").click();
        h.run_steps(3);
    }

    #[test]
    fn contact_dates_in_the_future_or_after_the_reply_are_refused() {
        let today = crate::ui::widgets::today();
        let dir = tempfile::tempdir().unwrap();
        let contact = SemesterContact { contacted_on: Some(today), ..Default::default() };
        let (mut h, id) = with_open_advisee(&dir.path().join("data.svt"), AdviseeRecord { s2: contact, ..Default::default() });
        let s1 = |h: &mut Harness<'_, SupervisorApp>| session(h).db.student(id).unwrap().advisee.as_ref().unwrap().s1.clone();

        // Tomorrow is refused, with the reason.
        pick_next_day(&mut h, today);
        let s2 = session(&mut h).db.student(id).unwrap().advisee.as_ref().unwrap().s2.clone();
        assert_eq!(s2.contacted_on, Some(today));
        assert!(h.query_by_label_contains("This date is in the future").is_some());

        {
            let s = session(&mut h);
            let c = &mut s.db.student_mut(id).unwrap().advisee.as_mut().unwrap().s1;
            c.contacted_on = Some(today - Days::days(5));
            c.reply = ReplyStatus::Replied;
            c.replied_on = Some(today - Days::days(6));
        }
        h.run_steps(3);
        // One saved before this check stays, with a warning on both dates.
        assert_eq!(h.query_all_by_label_contains("If they wrote first").count(), 2);
        shot(&mut h, "44_contact_dates_warning");
        assert_eq!(s1(&mut h).replied_on, Some(today - Days::days(6)));

        // A contact date can't move past a good reply.
        {
            let s = session(&mut h);
            s.db.student_mut(id).unwrap().advisee.as_mut().unwrap().s1.replied_on = Some(today - Days::days(5));
        }
        h.run_steps(3);
        assert!(h.query_by_label_contains("If they wrote first").is_none());
        let contacted = today - Days::days(5);
        pick_next_day(&mut h, contacted);
        assert_eq!(s1(&mut h).contacted_on, Some(contacted));
        assert!(h.query_by_label_contains("after their reply").is_some());

        // If they wrote first, leaving the contact empty lets the reply stand.
        {
            let s = session(&mut h);
            let c = &mut s.db.student_mut(id).unwrap().advisee.as_mut().unwrap().s1;
            c.contacted_on = None;
            c.replied_on = Some(today - Days::days(20));
        }
        h.run_steps(3);
        assert!(h.query_by_label_contains("before you contacted").is_none());
        // Set date then starts at the reply, not after it.
        h.get_all_by_label(&format!("{} Set date", icons::CALENDAR_BLANK)).next().unwrap().click();
        h.run_steps(3);
        assert_eq!(s1(&mut h).contacted_on, Some(today - Days::days(20)));
    }

    /// Saves the pictures in the README, with `SUPERVISOR_SHOTS` set; made up
    /// of invented students only. Frames named `gif_*` become its animations.
    #[test]
    fn readme_pictures() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        let mut h = create_and_unlock(&path, false);
        let today = crate::ui::widgets::today();
        let sean = {
            let s = session(&mut h);
            s.db.settings.sem1_start = Some(today - Days::days(30));
            s.db.settings.sem2_start = Some(today + Days::days(90));
            s.db.students.extend(demo_students(today));
            let sean = s.db.students.iter_mut().find(|s| s.name == "Sean O'Neill").unwrap();
            sean.email = "oneill-s@uni.ac.uk".into();
            sean.advisee.as_mut().unwrap().year = Some(StudyYear::Second);
            for (day, hour) in [(0, 9), (0, 10), (1, 14), (1, 15), (2, 11), (3, 9), (3, 10), (3, 11), (4, 13)] {
                sean.timetable.set_busy(Semester::S1, BusySlot { day, hour }, true);
            }
            let id = sean.id;
            // The dashboard and calendar work from this, normally updated on input.
            s.db_print = storage::fingerprint(&s.db);
            id
        };
        h.run_steps(3);
        shot(&mut h, "dashboard");
        {
            let s = session(&mut h);
            s.view = View::Calendar;
            s.calendar.layout = crate::ui::calendar::Layout::Month;
            s.calendar.month = today.with_day(1).unwrap();
        }
        h.run_steps(3);
        shot(&mut h, "calendar");
        {
            let s = session(&mut h);
            s.view = View::List(Category::Advisee);
            s.selected = Some(sean);
        }
        h.run_steps(3);
        shot(&mut h, "student");
        h.get_by_label("Timetable").click();
        h.run_steps(3);
        shot(&mut h, "timetable");
        session(&mut h).timetable = None;
        h.run_steps(3);

        // The modules card, being filled in, with room for all of it.
        h.set_size(egui::vec2(1360.0, 1120.0));
        h.run_steps(3);
        let type_in = |h: &mut Harness<'_, SupervisorApp>, hint_index: usize, text: &str| {
            let fields = h.get_all_by_role(AkRole::TextInput).collect::<Vec<_>>();
            fields[hint_index].focus();
            fields[hint_index].type_text(text);
            h.run_steps(2);
        };
        let mut frame = 0;
        let mut film = |h: &mut Harness<'_, SupervisorApp>, name: &str| {
            frame += 1;
            shot(h, &format!("gif_{name}_{frame:02}"));
        };
        film(&mut h, "modules");
        h.get_by_label(&modules_title()).click();
        h.run_steps(6);
        film(&mut h, "modules");
        h.get_all_by_label("Pending").next().unwrap().click();
        h.run_steps(4);
        film(&mut h, "modules");
        // The new fields come after the search box and the details' fields.
        let code = h.get_all_by_role(AkRole::TextInput).position(|f| f.value().is_some_and(|v| v.is_empty())
            && f.rect().top() > h.get_by_label("How many").rect().top()).expect("code field");
        type_in(&mut h, code, "ENG502");
        film(&mut h, "modules");
        type_in(&mut h, code + 1, "Thermofluids");
        film(&mut h, "modules");
        h.get_all_by_label("CW+EX").next().unwrap().click();
        h.run_steps(4);
        film(&mut h, "modules");
        h.get_all_by_label(&format!("{} Reveal marks", icons::EYE)).next().unwrap().click();
        h.run_steps(4);
        {
            let s = session(&mut h);
            let m = &mut s.db.student_mut(sean).unwrap().modules.s1.failed[0];
            m.cw_mark = "38".into();
            m.ex_mark = "34".into();
        }
        h.run_steps(4);
        film(&mut h, "modules");
        film(&mut h, "modules");

        // The RAR notes, behind the password.
        h.get_all_by_label(&format!("{} Hide marks", icons::EYE_SLASH)).next().unwrap().click();
        h.run_steps(3);
        h.get_by_label(&format!("{} · S1: 1 pending", modules_title())).click();
        session(&mut h).db.student_mut(sean).unwrap().rar = true;
        h.set_size(egui::vec2(1360.0, 860.0));
        h.run_steps(6);
        frame = 0;
        let mut film = |h: &mut Harness<'_, SupervisorApp>, name: &str| {
            frame += 1;
            shot(h, &format!("gif_{name}_{frame:02}"));
        };
        film(&mut h, "rar");
        h.get_by_label("RAR Notes").click();
        h.run_steps(4);
        film(&mut h, "rar");
        let field = h.get_by_role(AkRole::PasswordInput);
        field.focus();
        field.type_text(PASSWORD);
        h.run_steps(3);
        film(&mut h, "rar");
        h.get_by_label("Open").click();
        h.run_steps(4);
        film(&mut h, "rar");
        h.event(egui::Event::Text("Extra time in examinations. Slides shared in advance of lectures.".into()));
        h.run_steps(3);
        film(&mut h, "rar");
        film(&mut h, "rar");
        h.get_by_label("Done").click();
        h.run_steps(4);
        film(&mut h, "rar");
    }

    fn copied(h: &Harness<'_, SupervisorApp>) -> Vec<String> {
        h.output()
            .platform_output
            .commands
            .iter()
            .filter_map(|c| match c {
                egui::OutputCommand::CopyText(t) => Some(t.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn ticked_students_emails_are_copied() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        let mut h = create_and_unlock(&path, false);
        let advisee = |name: &str, email: &str| Student {
            name: name.into(),
            email: email.into(),
            advisee: Some(AdviseeRecord::default()),
            ..Default::default()
        };
        {
            let s = session(&mut h);
            s.db.students.extend([
                advisee("Ana Silva", "ana@ulster.ac.uk"),
                advisee("Bob Jones", " bob@ulster.ac.uk "),
                advisee("Cara Byrne", ""),
                advisee("Dan Doyle", "dan@ulster.ac.uk"),
            ]);
            s.view = View::List(Category::Advisee);
        }
        h.run_steps(3);
        let copy = format!("{}  Copy emails", icons::ENVELOPE_SIMPLE);
        assert!(h.get_by_label(&copy).accesskit_node().is_disabled(), "nothing ticked yet");

        // Ticking one doesn't open the student.
        h.get_by_label("Tick Bob Jones").click();
        h.run_steps(2);
        assert_eq!(session(&mut h).selected, None);
        h.get_by_label("Tick Ana Silva").click();
        h.run_steps(2);
        assert!(h.query_by_label("2 students ticked").is_some());
        h.get_by_label(&copy).click();
        h.run_steps(1);
        assert_eq!(copied(&h), ["ana@ulster.ac.uk; bob@ulster.ac.uk"]);
        h.run_steps(1);
        assert!(h.query_by_label_contains("Copied 2 emails").is_some());

        // Everyone shown: the student without an email is named.
        h.get_by_label("Tick everyone shown").click();
        h.run_steps(2);
        h.get_by_label(&copy).click();
        h.run_steps(1);
        assert_eq!(copied(&h), ["ana@ulster.ac.uk; bob@ulster.ac.uk; dan@ulster.ac.uk"]);
        h.run_steps(1);
        assert!(h.query_by_label_contains("No email for Cara Byrne").is_some());
        shot(&mut h, "60_copy_emails");

        // A search narrows what is copied.
        session(&mut h).search = "dan".into();
        h.run_steps(2);
        assert!(h.query_by_label("1 student ticked").is_some());
        h.get_by_label(&copy).click();
        h.run_steps(1);
        assert_eq!(copied(&h), ["dan@ulster.ac.uk"]);

        // Another list starts with nothing ticked.
        {
            let s = session(&mut h);
            s.search.clear();
            s.view = View::List(Category::All);
        }
        h.run_steps(2);
        assert!(h.get_by_label(&copy).accesskit_node().is_disabled());
        session(&mut h).view = View::List(Category::Advisee);
        h.run_steps(2);
        assert!(h.get_by_label(&copy).accesskit_node().is_disabled(), "unticked on leaving the list");
    }

    #[test]
    fn students_i_mark_are_listed_by_poster_and_marked_as_second_marker() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        let mut h = create_and_unlock(&path, false);
        let marked = |name: &str, poster: &str| Student {
            name: name.into(),
            marking: Some(MarkingRecord {
                programme: "MEng".into(),
                supervisor_name: "Bhalla, Dr Nikhil".into(),
                title: "Plasma modelling".into(),
                poster_number: poster.into(),
                ..Default::default()
            }),
            ..Default::default()
        };
        let (late, early) = (marked("Aaron Late", "190"), marked("Zoe Early", "71"));
        let report = Student {
            name: "Placement Report".into(),
            marking: Some(MarkingRecord { kind: MarkingKind::Placement, company: "Acme".into(), ..Default::default() }),
            ..Default::default()
        };
        let early_id = early.id;
        {
            let s = session(&mut h);
            s.db.students.extend([late, early, report]);
            s.view = View::List(Category::Marking(MarkingKind::Project));
        }
        h.set_size(egui::vec2(1360.0, 1300.0));
        h.run_steps(3);
        // In the order of the posters, whatever the names.
        let zoe = h.get_by_label("Zoe Early").rect().top();
        assert!(zoe < h.get_by_label("Aaron Late").rect().top());
        assert!(h.query_by_label("Placement Report").is_none(), "in its own list");
        assert!(h.query_all_by_label("0/2 marked").next().is_some());
        shot(&mut h, "50_marking_list");

        session(&mut h).selected = Some(early_id);
        h.run_steps(3);
        // As second marker of an MEng project: the poster (mine alone) and the
        // dissertation (agreed with the supervisor), not the interim report.
        for label in ["Poster", "Dissertation", "Supervisor", "My mark", "Agreed", "EEE840"] {
            assert!(h.query_all_by_label(label).next().is_some(), "{label}");
        }
        assert!(h.query_by_label("Interim report").is_none());
        h.get_all_by_label(&format!("{} Reveal marks", icons::EYE)).next().unwrap().click();
        h.run_steps(2);
        shot(&mut h, "51_marking_panel");
        let poster = session(&mut h).db.settings.second_marked("MEng")[0].1.id;
        let dissertation = session(&mut h).db.settings.second_marked("MEng")[1].1.id;
        {
            let m = session(&mut h).db.student_mut(early_id).unwrap().marking.as_mut().unwrap();
            m.coursework.insert(poster, CwStatus { mark_2: "64".into(), ..Default::default() });
            m.coursework.insert(dissertation, CwStatus { mark_1: "70".into(), mark_2: "55".into(), ..Default::default() });
        }
        h.run_steps(3);
        assert!(h.query_by_label(&format!("{} 21.4% from the supervisor's mark", icons::WARNING)).is_some());
        assert!(h.query_all_by_label("to agree").next().is_some());
        shot(&mut h, "52_marking_to_agree");

        // A placement report: my mark alone.
        session(&mut h).view = View::List(Category::Marking(MarkingKind::Placement));
        let id = session(&mut h).db.students[2].id;
        session(&mut h).selected = Some(id);
        h.run_steps(3);
        assert!(h.query_by_label("Final report").is_some());
        assert!(h.query_by_label("Agreed").is_none() && h.query_by_label("Visitor").is_none());
        shot(&mut h, "53_marking_placement");

        // Who marks each coursework, and its weight, are set in Settings.
        session(&mut h).view = View::Settings;
        h.set_size(egui::vec2(1360.0, 1800.0));
        h.run_steps(3);
        h.get_by_label("EEE840 – MEng Final Year Dissertation (Full year)").click();
        h.run_steps(3);
        let text = all_text(&h);
        for label in ["Marked by", "Weight", "Supervisor only", "2nd marker only", "Supervisor + 2nd marker"] {
            assert!(text.contains(label), "{label}");
        }
        assert!(h.query_by_label_contains("The weights add up to").is_none());
        session(&mut h).db.settings.modules[4].courseworks[2].weight = Some(70);
        h.run_steps(3);
        assert!(h.query_by_label_contains("The weights add up to 90%, not 100%").is_some());
        shot(&mut h, "54_module_markers");
    }
}
