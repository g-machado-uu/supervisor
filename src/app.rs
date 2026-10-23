//! Application shell: lock state, navigation, autosave and dialogs.

use std::time::Duration;

use egui::{Id, RichText, Ui};
use uuid::Uuid;

use crate::ics;
use crate::model::{AdviseeRecord, CalItem, Database, FypRecord, PlacementRecord, Student};
use crate::storage::{self, Vault};
use crate::ui::calendar::{self, CalAction, CalendarState};
use crate::ui::dashboard::{self, DashAction};
use crate::ui::lock::LockScreen;
use crate::ui::settings::{self, PasswordForm, SettingsAction};
use crate::ui::student_form::{self, FormAction, Role};
use crate::ui::student_list::{self, Category, ListState};
use crate::ui::theme;

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

impl eframe::App for SupervisorApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|i| i.viewport().close_requested())
            && let Screen::Unlocked(s) = &mut self.screen
            && s.dirty_since.is_some()
        {
            s.save();
        }
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        match &mut self.screen {
            Screen::Locked(lock) => {
                if let Some((vault, db)) = lock.ui(ui) {
                    self.screen = Screen::Unlocked(Box::new(Session::new(vault, db)));
                }
            }
            Screen::Unlocked(session) => {
                if session.ui(ui) {
                    session.save();
                    let path = session.vault.path().to_path_buf();
                    // Dropping the session wipes the key and decrypted data from memory.
                    self.screen = Screen::Locked(LockScreen::new(path));
                }
            }
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
}

enum Confirm {
    DeleteStudent(Uuid),
    RemoveRole(Uuid, Role),
}

struct NewStudent {
    student: Student,
    fyp: bool,
    programme: String,
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
    calendar: CalendarState,
    toast: Option<(String, f64)>,
    applied_dark: Option<bool>,
}

impl Session {
    fn new(vault: Vault, db: Database) -> Self {
        Self {
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
            calendar: CalendarState::default(),
            toast: None,
            applied_dark: None,
        }
    }

    fn save(&mut self) {
        match self.vault.save(&self.db) {
            Ok(()) => {
                self.status = SaveStatus::Saved;
                self.dirty_since = None;
            }
            Err(e) => {
                // Keep it dirty so the next autosave retries.
                self.status = SaveStatus::Error(format!("{e:#}"));
            }
        }
    }

    fn notify(&mut self, ui: &Ui, msg: impl Into<String>) {
        self.toast = Some((msg.into(), ui.input(|i| i.time)));
    }

    /// Draws the unlocked app. Returns true if the user asked to lock.
    fn ui(&mut self, ui: &mut Ui) -> bool {
        let before = self.db.clone();
        let now = ui.input(|i| i.time);
        let mut lock = false;

        if ui.input_mut(|i| i.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::L))) {
            lock = true;
        }

        self.top_bar(ui, &mut lock);
        self.sidebar(ui);
        if self.view != View::Settings && self.selected.is_some() {
            self.detail_panel(ui);
        }
        egui::CentralPanel::default().show(ui, |ui| self.central(ui));
        self.dialogs(ui);

        if self.db != before {
            self.dirty_since = Some(now);
            self.status = SaveStatus::Pending;
        }
        if self.applied_dark != Some(self.db.settings.dark_mode) {
            self.applied_dark = Some(self.db.settings.dark_mode);
            theme::apply(ui.ctx(), self.db.settings.dark_mode);
        }
        if let Some(since) = self.dirty_since {
            let wait = AUTOSAVE_DELAY - (now - since);
            if wait <= 0.0 {
                self.save();
                if self.dirty_since.is_some() {
                    // Save failed; back off before retrying.
                    self.dirty_since = Some(now + 4.0);
                }
            } else {
                ui.ctx().request_repaint_after(Duration::from_secs_f64(wait));
            }
        }
        lock
    }

    fn top_bar(&mut self, ui: &mut Ui, lock: &mut bool) {
        egui::Panel::top("top-bar").show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("🎓 Supervisor").strong().size(18.0));
                ui.label(RichText::new(&self.db.settings.academic_year).color(theme::GREY));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("🔒 Lock").on_hover_text("Lock (Ctrl+L)").clicked() {
                        *lock = true;
                    }
                    let dark = &mut self.db.settings.dark_mode;
                    if ui
                        .button(if *dark { "☀" } else { "🌙" })
                        .on_hover_text("Toggle light/dark")
                        .clicked()
                    {
                        *dark = !*dark;
                    }
                    match &self.status {
                        SaveStatus::Saved => ui.label(RichText::new("Saved ✔").small().color(theme::GREEN)),
                        SaveStatus::Pending => ui.label(RichText::new("Saving…").small().color(theme::GREY)),
                        SaveStatus::Error(e) => ui
                            .label(RichText::new("⚠ Save failed").small().color(theme::RED))
                            .on_hover_text(e),
                    };
                    let now = ui.input(|i| i.time);
                    if let Some((msg, at)) = &self.toast {
                        if now - at < 5.0 {
                            ui.label(RichText::new(msg).small().color(theme::ACCENT));
                            ui.ctx().request_repaint_after(Duration::from_millis(500));
                        } else {
                            self.toast = None;
                        }
                    }
                });
            });
            ui.add_space(2.0);
        });
    }

    fn sidebar(&mut self, ui: &mut Ui) {
        egui::Panel::left("nav").exact_size(230.0).show(ui, |ui| {
            ui.add_space(8.0);
            let lists = self.db.settings.programme_lists();
            let mut nav = |ui: &mut Ui, view: View, label: &str, count: Option<usize>| {
                let mut b = egui::Button::selectable(self.view == view, label).min_size(egui::vec2(ui.available_width(), 32.0));
                if let Some(c) = count {
                    b = b.right_text(RichText::new(c.to_string()).color(theme::GREY));
                }
                if ui.add(b).clicked() {
                    self.view = view;
                }
            };
            nav(ui, View::Dashboard, "🏠  Dashboard", None);
            nav(ui, View::Calendar, "📅  Calendar", None);
            ui.add_space(6.0);
            ui.label(RichText::new("PROJECT STUDENTS").small().color(theme::GREY));
            for list in lists {
                let cat = Category::Projects(list.clone());
                let count = cat.count(&self.db);
                nav(ui, View::List(cat), &format!("📘  {list}"), Some(count));
            }
            ui.add_space(6.0);
            ui.label(RichText::new("OTHER").small().color(theme::GREY));
            nav(ui, View::List(Category::Placement), "🏢  Placement", Some(Category::Placement.count(&self.db)));
            nav(ui, View::List(Category::Advisee), "💬  Studies Advisees", Some(Category::Advisee.count(&self.db)));
            nav(ui, View::List(Category::All), "👥  All Students", Some(Category::All.count(&self.db)));
            ui.add_space(6.0);
            ui.separator();
            nav(ui, View::Settings, "⚙  Settings", None);
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
        egui::Panel::right("detail")
            .resizable(true)
            .default_size(520.0)
            .min_size(400.0)
            .show(ui, |ui| {
                ui.add_space(6.0);
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
            View::Dashboard => match dashboard::show(ui, &self.db) {
                DashAction::None => {}
                DashAction::Open(id) => self.selected = Some(id),
                DashAction::Goto(cat) => self.view = View::List(cat),
                DashAction::ExportIcs => self.export_ics(ui),
            },
            View::Calendar => {
                let mut guests = self.db.settings.export_with_guests;
                let action = calendar::show(ui, &self.db, &mut self.calendar, &mut guests);
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
                match settings::show(
                    ui,
                    &mut self.db.settings,
                    &in_use,
                    &path,
                    &mut self.password_form,
                ) {
                    SettingsAction::None => {}
                    SettingsAction::ExportIcs => self.export_ics(ui),
                    SettingsAction::ChangePassword => self.change_password(),
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
                // Deadlines never have guests; anything else without one lacks an email.
                let missing = items
                    .iter()
                    .filter(|i| i.event.kind != crate::model::EventKind::Deadline && i.attendees.is_empty())
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
        let hint = if invite.event.online && self.db.settings.teams_link.trim().is_empty() {
            "upload it in Outlook, open the event, turn on Teams meeting and Send"
        } else {
            "upload it in Outlook, open the event and Send"
        };
        if self.write_ics(ui, &[invite], true, &format!("{name}.ics")) {
            self.notify(ui, format!("{series}Invite saved – {hint}"));
        }
    }

    fn change_password(&mut self) {
        let pw = &mut self.password_form;
        let result = (|| {
            if pw.new.chars().count() < 8 {
                return Err("The new password must have at least 8 characters.".to_string());
            }
            if *pw.new != *pw.confirm {
                return Err("The new passwords do not match.".to_string());
            }
            Vault::open(self.vault.path(), &pw.current).map_err(|_| "The current password is incorrect.".to_string())?;
            self.vault
                .change_password(&pw.new, &self.db)
                .map_err(|e| format!("Could not change password: {e:#}"))
        })();
        *pw = PasswordForm::default();
        pw.message = Some(match result {
            Ok(()) => (true, "Password changed.".into()),
            Err(e) => (false, e),
        });
    }

    fn dialogs(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();

        if let Some(n) = &mut self.new_student {
            let mut create = false;
            let mut cancel = false;
            let dup = self.db.b_number_taken(&n.student.b_number, None);
            let resp = egui::Modal::new(Id::unique("new-student")).show(&ctx, |ui| {
                ui.set_width(440.0);
                ui.heading("Add student");
                ui.add_space(6.0);
                crate::ui::widgets::form_grid(ui, "new-grid", |ui| {
                    let s = &mut n.student;
                    ui.label("B Number");
                    ui.vertical(|ui| {
                        crate::ui::widgets::text_field(ui, &mut s.b_number, "B00123456");
                        if dup {
                            ui.colored_label(theme::AMBER, "⚠ Already exists");
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
                ui.add_space(6.0);
                ui.label(RichText::new("Roles").strong());
                ui.checkbox(&mut n.fyp, "Project student (FYP / MSc)");
                if n.fyp {
                    ui.indent("fyp-kind", |ui| {
                        for p in &self.db.settings.programmes {
                            ui.radio_value(&mut n.programme, p.id.clone(), &p.name);
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
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let valid = !n.student.name.trim().is_empty() || !n.student.b_number.trim().is_empty();
                    if ui
                        .add_enabled(valid, egui::Button::new(RichText::new("Create").strong()).fill(theme::ACCENT.gamma_multiply(0.85)))
                        .clicked()
                    {
                        create = true;
                    }
                    if ui.button("Cancel").clicked() {
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
            let resp = egui::Modal::new(Id::unique("confirm")).show(&ctx, |ui| {
                ui.set_width(360.0);
                ui.heading(title);
                ui.label(body);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new(RichText::new("Delete").strong()).fill(theme::RED.gamma_multiply(0.8))).clicked() {
                        yes = true;
                    }
                    if ui.button("Cancel").clicked() {
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

    fn wait_unlocked(h: &mut Harness<'_, SupervisorApp>) {
        for _ in 0..600 {
            h.run_steps(1);
            if matches!(h.state().screen, Screen::Unlocked(_)) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!("did not unlock");
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
        ]
    }

    #[test]
    fn end_to_end() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        let p = path.clone();
        let mut h = Harness::builder()
            .with_size(egui::vec2(1360.0, 860.0))
            .build_eframe(move |cc| SupervisorApp::new(cc, Some(p)));
        h.run_steps(2);
        shot(&mut h, "01_create_password");

        // First run: create the password.
        let fields = h.get_all_by_role(AkRole::PasswordInput).collect::<Vec<_>>();
        assert_eq!(fields.len(), 2);
        fields[0].focus();
        fields[0].type_text(PASSWORD);
        fields[1].focus();
        fields[1].type_text(PASSWORD);
        h.run_steps(1);
        h.get_by_label("Create & unlock").click();
        wait_unlocked(&mut h);
        h.run_steps(2);
        assert!(path.exists());

        // Add a student through the dialog.
        h.get_by_label_contains("🏢  Placement").click();
        h.run_steps(2);
        h.get_by_label("➕ Add student").click();
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
        let (_, saved) = Vault::open(&path, PASSWORD).unwrap();
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
        shot(&mut h, "03_dashboard");

        for (view, name) in [
            (View::Calendar, "04_calendar"),
            (View::List(Category::Projects("BEng".into())), "04_fyp_beng"),
            (View::List(Category::Placement), "05_placement"),
            (View::List(Category::Advisee), "06_advisees"),
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

        // Dark mode.
        session(&mut h).db.settings.dark_mode = true;
        session(&mut h).view = View::Dashboard;
        h.run_steps(3);
        assert!(h.ctx.global_style().visuals.dark_mode);
        shot(&mut h, "10_dashboard_dark");

        // Lock: back to the unlock screen, and the right password reopens the data.
        h.get_by_label("🔒 Lock").click();
        h.run_steps(2);
        assert!(matches!(h.state().screen, Screen::Locked(_)));
        let (_, saved) = Vault::open(&path, PASSWORD).unwrap();
        assert!(saved.students.iter().any(|s| s.name == "Ciara Doyle"));
        assert!(saved.settings.dark_mode);
    }
}
