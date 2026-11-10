//! Unlock screen (and first-run password creation).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};

use anyhow::Result;
use egui::{Align, Key, Layout, RichText};
use zeroize::Zeroizing;

use crate::model::Database;
use crate::storage::{self, KeyFile, Vault};
use crate::ui::file_dialog::{Dialog, Pending, waiting};
use crate::ui::{icons, theme, widgets};

/// What the lock screen hands over once the data is open.
pub struct Unlocked {
    pub vault: Vault,
    pub db: Database,
    /// The password is weaker than new passwords may be; suggest changing it.
    pub weak_password: bool,
}

/// Whether the data file needs a key file, and whether this computer has the right one.
enum KeyState {
    NotNeeded,
    Ready(KeyFile),
    Missing([u8; 32]),
    Wrong([u8; 32]),
}

impl KeyState {
    fn of(path: &Path) -> Self {
        let Ok(Some(id)) = storage::required_key_id(path) else {
            return Self::NotNeeded;
        };
        match storage::local_key_file() {
            Ok(Some(key)) if key.id() == id => Self::Ready(key),
            Ok(Some(_)) => Self::Wrong(id),
            _ => Self::Missing(id),
        }
    }
}

pub struct LockScreen {
    pub path: PathBuf,
    /// The file should be there: it was opened before. If it isn't, that is
    /// said, rather than offering to create a new one in its place.
    expected: bool,
    /// The expected file wasn't there when last drawn.
    was_missing: bool,
    password: Zeroizing<String>,
    confirm: Zeroizing<String>,
    error: Option<String>,
    key: KeyState,
    weak_password: bool,
    pending: Option<Receiver<Result<(Vault, Database)>>>,
    /// A file dialog while it is open, and what its answer is for.
    choosing: Option<(Pending, Choosing)>,
}

enum Choosing {
    Key([u8; 32]),
    Open,
    New,
}

impl LockScreen {
    /// `expected`: the file was opened before, so it should be there.
    pub fn new(path: PathBuf, expected: bool) -> Self {
        Self {
            key: KeyState::of(&path),
            was_missing: expected && !path.exists(),
            path,
            expected,
            password: Zeroizing::new(String::new()),
            confirm: Zeroizing::new(String::new()),
            error: None,
            weak_password: false,
            pending: None,
            choosing: None,
        }
    }

    /// Act on the file dialog's answer once it has closed.
    fn file_chosen(&mut self) {
        let Some(answer) = self.choosing.as_ref().and_then(|(d, _)| d.poll()) else { return };
        let Some((_, what)) = self.choosing.take() else { return };
        let Some(path) = answer else { return };
        match what {
            Choosing::Key(id) => match storage::import_key_file(&path, id) {
                Ok(key) => {
                    self.key = KeyState::Ready(key);
                    self.error = None;
                }
                Err(e) => self.error = Some(format!("{e:#}")),
            },
            Choosing::Open => *self = Self::new(path, true),
            Choosing::New if path.exists() => {
                self.error = Some("That file already exists – use “Open another file…”.".into());
            }
            Choosing::New => *self = Self::new(path, false),
        }
    }

    /// A new data file is to be created.
    fn first_run(&self) -> bool {
        !self.expected && !self.path.exists()
    }

    /// The data file opened before isn't there (a USB stick or network
    /// drive not connected, a synced folder not ready, or moved).
    fn missing(&self) -> bool {
        self.expected && !self.path.exists()
    }

    fn submit(&mut self, ctx: &egui::Context) {
        if self.missing() {
            return;
        }
        let creating = self.first_run();
        if creating {
            if let Some(problem) = storage::password_problem(&self.password) {
                self.error = Some(problem);
                return;
            }
            if *self.password != *self.confirm {
                self.error = Some("The passwords do not match.".into());
                return;
            }
        } else if self.password.is_empty() {
            return;
        }
        // The file may have changed (e.g. synced) since this screen opened.
        self.key = KeyState::of(&self.path);
        let key_file = match &self.key {
            KeyState::NotNeeded => None,
            KeyState::Ready(key) => Some(key.clone()),
            KeyState::Missing(_) | KeyState::Wrong(_) => return,
        };
        self.weak_password = !creating && storage::password_problem(&self.password).is_some();
        self.error = None;
        let (tx, rx) = channel();
        let path = self.path.clone();
        let password = std::mem::replace(&mut self.password, Zeroizing::new(String::new()));
        self.confirm = Zeroizing::new(String::new());
        let ctx = ctx.clone();
        // Key derivation is deliberately slow; keep the UI responsive.
        std::thread::spawn(move || {
            let result = if creating {
                let db = Database::default();
                Vault::create(&path, &password, &db).map(|v| (v, db))
            } else {
                Vault::open(&path, &password, key_file.as_ref())
            };
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.pending = Some(rx);
    }

    /// Draws the screen; returns the unlocked data once ready.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<Unlocked> {
        self.file_chosen();
        if let Some(rx) = &self.pending
            && let Ok(result) = rx.try_recv()
        {
            self.pending = None;
            match result {
                Ok((vault, db)) => {
                    storage::remember_path(&self.path);
                    return Some(Unlocked {
                        vault,
                        db,
                        weak_password: self.weak_password,
                    });
                }
                Err(e) => self.error = Some(format!("{e:#}")),
            }
        }

        let first_run = self.first_run();
        let missing = self.missing();
        if missing != self.was_missing {
            // Back (plugged in, synced): what it needs is known only now.
            self.was_missing = missing;
            self.key = KeyState::of(&self.path);
            self.error = None;
        }
        if missing {
            // Notice when it is plugged in or synced.
            ui.ctx().request_repaint_after(std::time::Duration::from_secs(2));
        }
        let unlocking = self.pending.is_some();
        // Unlocking, or a file dialog open: nothing to click meanwhile.
        let busy = unlocking || self.choosing.is_some();
        let needs_key = matches!(self.key, KeyState::Missing(_) | KeyState::Wrong(_));
        let width = ui.available_width();
        if width > 900.0 {
            egui::Panel::left("lock-brand")
                .exact_size((width * 0.42).min(560.0))
                .frame(egui::Frame::new().fill(theme::sidebar_fill(ui)).inner_margin(egui::Margin::same(48)))
                .show_separator_line(false)
                .resizable(false)
                .show(ui, |ui| {
                    ui.add_space((ui.available_height() * 0.22).max(10.0));
                    brand(ui, Align::Min);
                    ui.add_space(28.0);
                    for (icon, title, text) in [
                        (icons::BOOK_OPEN_TEXT, "Project students", "Meetings, coursework and deadlines by course"),
                        (icons::BUILDINGS, "Placements", "Visit requests, replies and line managers"),
                        (icons::CHAT_CIRCLE_TEXT, "Studies advisees", "Contact and meetings for each semester"),
                        (icons::LOCK_SIMPLE, "Encrypted", "Password protected, with an optional key file"),
                    ] {
                        ui.horizontal(|ui| {
                            theme::icon_badge(ui, icon, theme::ACCENT, 38.0);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 1.0;
                                ui.label(theme::semibold(title));
                                ui.label(RichText::new(text).small().color(theme::muted(ui)));
                            });
                        });
                        ui.add_space(8.0);
                    }
                });
        }
        egui::CentralPanel::default().show(ui, |ui| {
            ui.with_layout(Layout::top_down(Align::Center), |ui| {
                ui.add_space((ui.available_height() * 0.2).max(20.0));
                if width <= 900.0 {
                    brand(ui, Align::Center);
                    ui.add_space(18.0);
                }

                ui.scope(|ui| {
                    ui.set_max_width(400.0);
                    theme::card_frame(ui).inner_margin(egui::Margin::same(28)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.vertical_centered(|ui| {
                            if missing {
                                self.missing_file(ui, busy);
                                return;
                            }
                            theme::icon_badge(ui, icons::LOCK_SIMPLE, theme::ACCENT, 48.0);
                            ui.add_space(4.0);
                            if first_run {
                                ui.label(theme::serif("Create a password", 24.0));
                                ui.label(
                                    RichText::new(
                                        "Your data will be encrypted with this password.\n\
                                         There is no way to recover it if forgotten.",
                                    )
                                    .color(theme::fg(ui, theme::AMBER)),
                                );
                                ui.label(
                                    RichText::new(format!(
                                        "At least {} characters – a short sentence you'll remember works well.",
                                        storage::MIN_PASSWORD_LEN
                                    ))
                                    .small()
                                    .color(theme::muted(ui)),
                                );
                            } else {
                                ui.label(theme::serif("Welcome back", 24.0));
                                ui.label(RichText::new("Enter your password to unlock your data.").color(theme::muted(ui)));
                            }
                            if let KeyState::Missing(id) | KeyState::Wrong(id) = self.key {
                                ui.add_space(8.0);
                                let text = if matches!(self.key, KeyState::Missing(_)) {
                                    "This data file also needs its key file, which isn't on this computer yet. \
                                     Import it from your backup copy (for example on a USB stick)."
                                } else {
                                    "The key file on this computer belongs to a different data file. \
                                     Import the right one from your backup copy."
                                };
                                ui.label(RichText::new(text).color(theme::fg(ui, theme::AMBER)));
                                if ui
                                    .add_enabled(!busy, egui::Button::new(format!("{}  Import key file…", icons::KEY)))
                                    .clicked()
                                {
                                    let dialog = Dialog::new().set_title("Choose your Supervisor key file").pick_file();
                                    self.choosing = Some((Pending::open(ui.ctx(), dialog), Choosing::Key(id)));
                                }
                            }
                            ui.add_space(10.0);

                            let pw = ui
                                .add_enabled_ui(!busy, |ui| {
                                    widgets::password_field(
                                        ui,
                                        egui::TextEdit::singleline(&mut *self.password)
                                            .hint_text("Password")
                                            .desired_width(300.0)
                                            .margin(egui::vec2(10.0, 8.0)),
                                    )
                                })
                                .inner;
                            // Also after a wrong password, ready to type it again.
                            if !busy && !pw.has_focus() && self.password.is_empty() {
                                pw.request_focus();
                            }
                            let mut enter = pw.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                            if first_run {
                                let c = ui
                                    .add_enabled_ui(!busy, |ui| {
                                        widgets::password_field(
                                            ui,
                                            egui::TextEdit::singleline(&mut *self.confirm)
                                                .hint_text("Confirm password")
                                                .desired_width(300.0)
                                                .margin(egui::vec2(10.0, 8.0)),
                                        )
                                    })
                                    .inner;
                                if enter {
                                    c.request_focus();
                                    enter = false;
                                }
                                enter |= c.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                            }

                            ui.add_space(8.0);
                            let label = if first_run { "Create & unlock" } else { "Unlock" };
                            let button = theme::primary_button(label).min_size(egui::vec2(300.0, 38.0));
                            if unlocking {
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    ui.label("Unlocking…");
                                });
                            } else if ui.add_enabled(!needs_key && !busy, button).clicked() || (enter && !needs_key && !busy) {
                                self.submit(ui.ctx());
                            }
                            if let Some(err) = &self.error {
                                ui.colored_label(theme::fg(ui, theme::RED), err);
                            }
                        });
                    });
                });

                ui.add_space(12.0);
                // When it is missing, the card already says where it was.
                if !missing {
                    ui.label(
                        RichText::new(format!("Data file: {}", self.path.display()))
                            .small()
                            .color(theme::muted(ui)),
                    );
                }
                if matches!(self.key, KeyState::Ready(_)) {
                    ui.label(
                        RichText::new(format!("{} Key file found on this computer", icons::KEY))
                            .small()
                            .color(theme::muted(ui)),
                    );
                }
                ui.horizontal(|ui| {
                    ui.add_space((ui.available_width() - 330.0).max(0.0) / 2.0);
                    if ui.add_enabled(!busy, egui::Button::new("Open another file…").small()).clicked() {
                        let dialog = Dialog::new().add_filter("Supervisor data", &["svt"]).pick_file();
                        self.choosing = Some((Pending::open(ui.ctx(), dialog), Choosing::Open));
                    }
                    if ui.add_enabled(!busy, egui::Button::new("Create new file at…").small()).clicked() {
                        let dialog = Dialog::new()
                            .add_filter("Supervisor data", &["svt"])
                            .set_file_name("data.svt")
                            .save_file();
                        self.choosing = Some((Pending::open(ui.ctx(), dialog), Choosing::New));
                    }
                });
            });
        });
        if self.choosing.is_some() {
            waiting(ui);
        }
        None
    }
}

impl LockScreen {
    /// In place of the password: the data file opened before isn't there.
    fn missing_file(&mut self, ui: &mut egui::Ui, busy: bool) {
        theme::icon_badge(ui, icons::WARNING, theme::AMBER, 48.0);
        ui.add_space(4.0);
        ui.label(theme::serif("Data file not found", 24.0));
        ui.label("Your data file isn't where it was:");
        ui.label(RichText::new(self.path.display().to_string()).small().color(theme::muted(ui)));
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "If it's on a USB stick, a network drive or in a synced folder (OneDrive, Dropbox), check that it's \
                 connected and available. If it was moved, open it where it is now, or open a backup copy.",
            )
            .color(theme::muted(ui)),
        );
        ui.add_space(10.0);
        let again = theme::primary_button("Look again").min_size(egui::vec2(300.0, 38.0));
        if ui.add_enabled(!busy, again).clicked() {
            self.error = Some("It still isn't there.".into());
        }
        ui.add_space(4.0);
        if ui
            .add_enabled(!busy, egui::Button::new("Start a new, empty data file here").small())
            .on_hover_text("Only if you have no data file yet: none of your data will be in it")
            .clicked()
        {
            self.expected = false;
            self.error = None;
        }
        if let Some(err) = &self.error {
            ui.colored_label(theme::fg(ui, theme::RED), err);
        }
    }
}

fn brand(ui: &mut egui::Ui, align: Align) {
    ui.with_layout(Layout::top_down(align), |ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(56.0, 56.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, egui::CornerRadius::same(16), theme::ACCENT);
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            icons::GRADUATION_CAP,
            egui::FontId::proportional(30.0),
            egui::Color32::WHITE,
        );
        ui.add_space(6.0);
        ui.label(theme::serif("Supervisor", 40.0));
        ui.label(RichText::new("Projects · Placements · Studies Advisees").color(theme::muted(ui)));
        ui.label(RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).color(theme::muted(ui)));

    });
}
