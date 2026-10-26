//! Application shell: lock state, navigation, autosave and dialogs.

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use chrono::NaiveDate;
use egui::{Id, RichText, Ui};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::ics;
use crate::model::{AdviseeRecord, Attention, CalItem, Database, Event, FypRecord, PhdRecord, PlacementRecord, Student};
use crate::storage::{self, KeyFile, Vault, VaultError};
use crate::ui::calendar::{self, CalAction, CalendarState};
use crate::ui::dashboard::{self, DashAction};
use crate::ui::lock::{LockScreen, Unlocked};
use crate::ui::settings::{self, KeyFileForm, PasswordForm, SettingsAction};
use crate::ui::student_form::{self, FormAction, Role};
use crate::ui::student_list::{self, Category, ListState};
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
    error: Option<String>,
}

enum Confirm {
    DeleteStudent(Uuid),
    RemoveRole(Uuid, Role),
}

struct NewStudent {
    student: Student,
    fyp: bool,
    programme: String,
    phd: bool,
    placement: bool,
    company: String,
    professional_email: String,
    advisee: bool,
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
            placement: false,
            company: String::new(),
            professional_email: String::new(),
            advisee: false,
        };
        match cat {
            Some(Category::Projects(list)) => {
                n.fyp = true;
                if let Some(p) = settings.programmes.iter().find(|p| p.list_name() == list) {
                    n.programme = p.id.clone();
                }
            }
            Some(Category::Phd) => n.phd = true,
            Some(Category::Placement) => n.placement = true,
            Some(Category::Advisee) => n.advisee = true,
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
            s.phd = Some(PhdRecord::default());
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
        s
    }
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
    dirty_since: Option<f64>,
    status: SaveStatus,
    new_student: Option<NewStudent>,
    confirm: Option<Confirm>,
    password_form: PasswordForm,
    key_form: KeyFileForm,
    key_setup: Option<KeyFileSetup>,
    calendar: CalendarState,
    toast: Option<(String, f64)>,
    applied_dark: Option<bool>,
    /// Fingerprint of `db` when last checked, to spot edits.
    db_print: [u8; 32],
    /// Events and attention items for `db_print` and today.
    derived: Option<Derived>,
    /// A save running in the background, and the `db_print` it started from.
    saving: Option<(Receiver<anyhow::Result<[u8; 32]>>, [u8; 32])>,
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
            vault,
            db,
            view: View::Dashboard,
            selected: None,
            search: String::new(),
            show_archived: false,
            dirty_since: None,
            status: SaveStatus::Saved,
            new_student: None,
            confirm: None,
            password_form: PasswordForm::default(),
            key_form: KeyFileForm::default(),
            key_setup: None,
            calendar: CalendarState::default(),
            toast: None,
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
            Ok(hash) => {
                self.vault.saved(hash);
                // Edits made while it was saving stay pending for the next save.
                if self.db_print == started_from {
                    self.saved(Ok(()));
                }
            }
            Err(e) => {
                self.saved(Err(e));
                if matches!(self.status, SaveStatus::Error(_)) {
                    // Back off before retrying.
                    self.dirty_since = Some(now + 4.0);
                }
            }
        }
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

    /// Draws the unlocked app. Returns what to do once nothing is left unsaved.
    fn ui(&mut self, ui: &mut Ui) -> Option<Leave> {
        let now = ui.input(|i| i.time);
        let mut lock = false;
        if ui.input(|i| !i.events.is_empty()) {
            self.last_active = Instant::now();
        }
        if let Some(msg) = self.notice.take() {
            self.notify(ui, msg);
        }

        if ui.input_mut(|i| i.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::L))) {
            lock = true;
        }

        self.sidebar(ui, &mut lock);
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
        self.dialogs(ui);

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
        }
        if let Some(since) = self.dirty_since
            && !matches!(self.status, SaveStatus::Conflict)
            && self.saving.is_none()
        {
            // Locking or quitting doesn't wait for a pause in typing, only
            // for the back-off after a failed save.
            let wait = match (&self.status, self.leaving) {
                (SaveStatus::Pending, Some(_)) => 0.0,
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

    fn sidebar(&mut self, ui: &mut Ui, lock: &mut bool) {
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
                section(ui, "PROJECT STUDENTS");
                for list in lists {
                    let cat = Category::Projects(list.clone());
                    let count = cat.count(&self.db);
                    nav(ui, View::List(cat), icons::BOOK_OPEN_TEXT, &list, Some(count));
                }
                section(ui, "RESEARCH");
                nav(ui, View::List(Category::Phd), icons::GRADUATION_CAP, "PhD", Some(Category::Phd.count(&self.db)));
                section(ui, "OTHER");
                nav(ui, View::List(Category::Placement), icons::BUILDINGS, "Placement", Some(Category::Placement.count(&self.db)));
                nav(ui, View::List(Category::Advisee), icons::CHAT_CIRCLE_TEXT, "Studies Advisees", Some(Category::Advisee.count(&self.db)));
                nav(ui, View::List(Category::All), icons::USERS_THREE, "All Students", Some(Category::All.count(&self.db)));

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
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            match &self.status {
                                SaveStatus::Saved => ui
                                    .label(RichText::new(format!("{} Saved", icons::CLOUD_CHECK)).small().color(theme::fg(ui, theme::GREEN)))
                                    .on_hover_text("All changes are saved and encrypted"),
                                SaveStatus::Pending => ui.label(RichText::new("Saving…").small().color(theme::muted(ui))),
                                SaveStatus::Error(e) => ui
                                    .label(RichText::new(format!("{} Save failed", icons::WARNING)).small().color(theme::fg(ui, theme::RED)))
                                    .on_hover_text(e),
                                SaveStatus::Conflict => ui
                                    .label(RichText::new(format!("{} Not saved", icons::WARNING)).small().color(theme::fg(ui, theme::RED)))
                                    .on_hover_text("The data file was changed somewhere else"),
                            };
                        });
                    });
                    ui.add_space(4.0);
                    nav(ui, View::Settings, icons::GEAR_SIX, "Settings", None);
                });
            });
    }

    fn detail_panel(&mut self, ui: &mut Ui) {
        let Some(id) = self.selected else { return };
        let dup = self
            .db
            .student(id)
            .is_some_and(|s| self.db.b_number_taken(&s.b_number, Some(id)));
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
                action = student_form::show(ui, student, settings, dup);
            });
        match action {
            FormAction::None => {}
            FormAction::Close => self.selected = None,
            FormAction::Delete => self.confirm = Some(Confirm::DeleteStudent(id)),
            FormAction::RemoveRole(role) => self.confirm = Some(Confirm::RemoveRole(id, role)),
            FormAction::Invite(uid) => self.save_invite(ui, id, &uid),
        }
    }

    fn central(&mut self, ui: &mut Ui) {
        match self.view.clone() {
            View::Dashboard => {
                let d = self.derived();
                let (events, attention) = (Rc::clone(&d.events), Rc::clone(&d.attention));
                match dashboard::show(ui, &self.db, &events, &attention) {
                    DashAction::None => {}
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
                let action = calendar::show(ui, &self.db, &events, &mut self.calendar, &mut guests);
                self.db.settings.export_with_guests = guests;
                match action {
                    CalAction::None => {}
                    CalAction::Open(id) => self.selected = Some(id),
                    CalAction::Export(uids) => self.export_selected(ui, &uids),
                }
            }
            View::List(cat) => {
                let add = student_list::show(
                    ui,
                    &self.db,
                    &cat,
                    ListState {
                        search: &mut self.search,
                        show_archived: &mut self.show_archived,
                        selected: &mut self.selected,
                    },
                );
                if add
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
                };
                match settings::show(ui, &mut self.db.settings, &in_use, security) {
                    SettingsAction::None => {}
                    SettingsAction::ExportIcs => self.export_ics(ui),
                    SettingsAction::ChangePassword => self.change_password(ui),
                    SettingsAction::AddKeyFile => self.start_key_file(),
                    SettingsAction::RemoveKeyFile => self.remove_key_file(ui),
                }
            }
        }
    }

    /// Ask where to save, then write an .ics of `items`.
    fn write_ics(&mut self, ui: &Ui, items: &[CalItem], invite: bool, file_name: &str) -> bool {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("iCalendar", &["ics"])
            .set_file_name(file_name)
            .save_file()
        else {
            return false;
        };
        let text = ics::calendar(
            items,
            &self.db.settings,
            invite,
            &jiff::tz::TimeZone::system(),
        );
        match std::fs::write(&path, text) {
            Ok(()) => true,
            Err(e) => {
                self.notify(ui, format!("Saving {} failed: {e}", path.display()));
                false
            }
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
        if self.write_ics(ui, &items, false, "supervisor.ics") {
            self.notify(ui, format!("Exported {} items", items.len()));
        }
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
        if self.write_ics(ui, &items, invite, name) {
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
            self.notify(ui, msg);
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
        let hint = if invite.event.online && !ics::valid_link(&self.db.settings.teams_link) {
            "upload it in Outlook, open the event, turn on Teams meeting and Send"
        } else {
            "upload it in Outlook, open the event and Send"
        };
        if self.write_ics(ui, &[invite], true, &format!("{name}.ics")) {
            self.notify(ui, format!("{series}Invite saved – {hint}"));
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
                if ui.button(format!("{} Save backup copy…", icons::EXPORT)).clicked()
                    && let Some(dest) = rfd::FileDialog::new()
                        .set_title("Save a backup copy of your key file")
                        .set_file_name("supervisor-key.bin")
                        .save_file()
                {
                    match storage::save_key_backup(&setup.key, &dest, &data_file) {
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
                crate::ui::widgets::form_grid(ui, "new-grid", |ui| {
                    let s = &mut n.student;
                    ui.label("B Number");
                    ui.vertical(|ui| {
                        crate::ui::widgets::text_field(ui, &mut s.b_number, "B00123456");
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
                ui.checkbox(&mut n.fyp, "Project student (FYP / MSc)");
                if n.fyp {
                    ui.indent("fyp-kind", |ui| {
                        for p in &self.db.settings.programmes {
                            ui.radio_value(&mut n.programme, p.id.clone(), &p.name);
                        }
                    });
                }
                ui.checkbox(&mut n.phd, "PhD student");
                ui.checkbox(&mut n.placement, "Placement student");
                if n.placement {
                    ui.indent("placement", |ui| {
                        crate::ui::widgets::text_field(ui, &mut n.company, "Placement company");
                        crate::ui::widgets::text_field(ui, &mut n.professional_email, "Professional email");
                    });
                }
                ui.checkbox(&mut n.advisee, "Studies advisee");
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

#[cfg(test)]
mod ui_tests {
    //! Drives the real UI headlessly. Set `SUPERVISOR_SHOTS=<dir>` to also
    //! write screenshots of each screen.
    use super::*;
    use crate::model::*;
    use chrono::{Datelike, Duration as Days, NaiveTime};
    use egui::accesskit::Role as AkRole;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

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
                                teams: true,
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
                        teams: true,
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
            (View::List(Category::Phd), "11_phd"),
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
            s.view = View::List(Category::Phd);
            s.selected = Some(priya);
        }
        h.run_steps(2);
        shot(&mut h, "12_phd_detail");

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
}
