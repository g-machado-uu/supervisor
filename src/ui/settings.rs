//! Settings: academic calendar, courses, module/coursework templates,
//! invitations, security, key file and about.

use std::collections::HashMap;

use egui::{RichText, Ui};
use zeroize::Zeroizing;

use crate::model::{CourseworkTemplate, Module, Programme, Semester, Settings};
use crate::storage;
use crate::ui::{icons, theme, widgets};

#[derive(Default)]
pub struct PasswordForm {
    pub current: Zeroizing<String>,
    pub new: Zeroizing<String>,
    pub confirm: Zeroizing<String>,
    pub message: Option<(bool, String)>,
}

#[derive(Default)]
pub struct KeyFileForm {
    pub password: Zeroizing<String>,
    pub message: Option<(bool, String)>,
}

/// What the Security and Key file cards show and edit.
pub struct Security<'a> {
    pub data_path: &'a str,
    pub key_file_on: bool,
    pub weak_password: bool,
    pub password: &'a mut PasswordForm,
    pub key_file: &'a mut KeyFileForm,
}

pub enum SettingsAction {
    None,
    ChangePassword,
    AddKeyFile,
    RemoveKeyFile,
    ExportIcs,
}

/// Choices for locking after a while without use (minutes; 0 = never).
const AUTO_LOCK_CHOICES: [u32; 7] = [2, 5, 10, 15, 30, 60, 0];

fn auto_lock_label(minutes: u32) -> String {
    match minutes {
        0 => "Never".into(),
        m => format!("After {m} minutes without use"),
    }
}

/// `in_use` counts the project students on each course (by id).
pub fn show(
    ui: &mut Ui,
    settings: &mut Settings,
    in_use: &HashMap<String, usize>,
    security: Security<'_>,
) -> SettingsAction {
    let mut action = SettingsAction::None;
    let Security { data_path, key_file_on, weak_password, password: pw, key_file: kf } = security;
    theme::page_header(ui, "Settings", "Academic calendar, courses, invitations, security and version", |_| {});
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        ui.set_max_width(920.0);
        ui.spacing_mut().item_spacing.y = 14.0;
        theme::card(ui, "Academic year", |ui| {
            widgets::form_grid(ui, "year-grid", |ui| {
                ui.label("Academic year");
                ui.add(egui::TextEdit::singleline(&mut settings.academic_year).desired_width(120.0));
                ui.end_row();
                for (sem, label) in [(Semester::S1, "Semester 1"), (Semester::S2, "Semester 2")] {
                    let (start, end) = match sem {
                        Semester::S2 => (&mut settings.sem2_start, &mut settings.sem2_teaching_end),
                        _ => (&mut settings.sem1_start, &mut settings.sem1_teaching_end),
                    };
                    ui.label(format!("{label} – week 1 starts"));
                    widgets::opt_date(ui, (label, "start"), start);
                    ui.end_row();
                    ui.label(format!("{label} – teaching ends"));
                    ui.horizontal(|ui| {
                        if end.is_some() {
                            widgets::opt_date(ui, (label, "end"), end);
                        } else if let Some(start) = *start {
                            let default = Settings::default_teaching_end(start);
                            ui.label(format!("{} (Friday of week 12)", widgets::fmt_date(Some(default))));
                            if ui.small_button("Change…").clicked() {
                                *end = Some(default);
                            }
                        } else {
                            ui.label(RichText::new("set week 1 first").color(theme::muted(ui)));
                        }
                    });
                    ui.end_row();
                }
                ui.label("Dashboard look-ahead");
                ui.add(egui::DragValue::new(&mut settings.upcoming_days).range(1..=120).suffix(" days"));
                ui.end_row();
                ui.label("Appearance");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut settings.dark_mode, false, format!("{} Light", icons::SUN));
                    ui.selectable_value(&mut settings.dark_mode, true, format!("{} Dark", icons::MOON));
                });
                ui.end_row();
            });
            ui.label(
                RichText::new("Coursework set by teaching week is due at the start of that week (Monday if week 1 starts on a Monday). Give an explicit date to override. Repeating meetings run until teaching ends, not until the end of exams.")
                    .small()
                    .color(theme::muted(ui)),
            );
        });

        ui.add_space(8.0);
        theme::card(ui, "Courses", |ui| {
            courses_editor(ui, &mut settings.programmes, in_use);
        });

        ui.add_space(8.0);
        theme::card(ui, "Modules & coursework", |ui| {
            ui.label(
                RichText::new("Each module applies to one or more courses. SUST modules are kept separate so their dates and coursework can differ.")
                    .small()
                    .color(theme::muted(ui)),
            );
            let starts = (settings.sem1_start, settings.sem2_start);
            let programmes = settings.programmes.clone();
            let mut remove_module = None;
            for (mi, module) in settings.modules.iter_mut().enumerate() {
                ui.push_id(module.id, |ui| {
                    let header = format!("{} – {} ({})", module.code, module.name, module.semester.label());
                    egui::CollapsingHeader::new(RichText::new(header).strong())
                        .id_salt("mod")
                        .show(ui, |ui| {
                            let start = match module.semester {
                                Semester::S1 | Semester::FullYear => starts.0,
                                Semester::S2 => starts.1,
                            };
                            if module_editor(ui, module, &programmes, start) {
                                remove_module = Some(mi);
                            }
                        });
                });
            }
            if let Some(i) = remove_module {
                settings.modules.remove(i);
            }
            if ui.button("➕ Add module").clicked() {
                settings.modules.push(Module::default());
            }
        });

        ui.add_space(8.0);
        theme::card(ui, "Invitations", |ui| {
            widgets::form_grid(ui, "invite-grid", |ui| {
                ui.label("Your name");
                widgets::text_field(ui, &mut settings.my_name, "Shown to students as the organiser");
                ui.end_row();
                ui.label("Your email");
                ui.vertical(|ui| {
                    widgets::text_field(ui, &mut settings.my_email, "name@ulster.ac.uk");
                    let email = settings.my_email.trim();
                    if !email.is_empty() && !crate::ics::valid_email(email) {
                        ui.colored_label(theme::fg(ui, theme::AMBER), format!("{} This doesn't look like an email address", icons::WARNING));
                    }
                });
                ui.end_row();
                ui.label("Teams link (optional)");
                ui.vertical(|ui| {
                    widgets::text_field(ui, &mut settings.teams_link, "Reusable Teams meeting link");
                    let link = settings.teams_link.trim();
                    if !link.is_empty() && !crate::ics::valid_link(link) {
                        ui.colored_label(theme::fg(ui, theme::AMBER), format!("{} Paste the full link, starting with https://", icons::WARNING));
                    }
                });
                ui.end_row();
            });
            ui.label(
                RichText::new("“Invite file…” on a meeting or visit saves a one-event .ics: upload it in Outlook (Add calendar › Upload from file), open the event and Send. For Teams meetings the link above is included; without one, turn on “Teams meeting” in Outlook before sending.")
                    .small()
                    .color(theme::muted(ui)),
            );
            ui.label(
                RichText::new("These details are stored in the encrypted data file. Invite files themselves are not encrypted and contain your and the student's email addresses.")
                    .small()
                    .color(theme::muted(ui)),
            );
        });

        ui.add_space(8.0);
        theme::card(ui, "Calendar", |ui| {
            if ui.button(format!("{} Export calendar (.ics)…", icons::EXPORT)).clicked() {
                action = SettingsAction::ExportIcs;
            }
            ui.label(
                RichText::new("Exports meetings, visits and coursework deadlines for Outlook / Google Calendar. The exported file is not encrypted.")
                    .small()
                    .color(theme::muted(ui)),
            );
        });

        ui.add_space(8.0);
        theme::card(ui, "Security", |ui| {
            ui.label(format!("Data file: {data_path}"));
            ui.label(
                RichText::new("Encrypted with Argon2id + XChaCha20-Poly1305. A backup of the previous save is kept alongside it (.bak).")
                    .small()
                    .color(theme::muted(ui)),
            );
            ui.add_space(6.0);
            widgets::form_grid(ui, "lock-grid", |ui| {
                ui.label("Lock automatically");
                egui::ComboBox::from_id_salt("auto-lock")
                    .selected_text(auto_lock_label(settings.auto_lock_minutes))
                    .show_ui(ui, |ui| {
                        for minutes in AUTO_LOCK_CHOICES {
                            ui.selectable_value(&mut settings.auto_lock_minutes, minutes, auto_lock_label(minutes));
                        }
                    });
                ui.end_row();
            });
            ui.label(
                RichText::new("Locking saves your changes and closes the data; your password opens it again.")
                    .small()
                    .color(theme::muted(ui)),
            );
            ui.add_space(6.0);
            ui.label(RichText::new("Change password").strong());
            if weak_password {
                ui.colored_label(
                    theme::fg(ui, theme::AMBER),
                    format!("{} Your password is easier to guess than Supervisor now allows. Please choose a new one.", icons::WARNING),
                );
            }
            widgets::form_grid(ui, "pw-grid", |ui| {
                for (label, field) in [
                    ("Current password", &mut pw.current),
                    ("New password", &mut pw.new),
                    ("Confirm new password", &mut pw.confirm),
                ] {
                    ui.label(label);
                    widgets::password_field(ui, egui::TextEdit::singleline(&mut **field).desired_width(240.0));
                    ui.end_row();
                }
            });
            ui.label(
                RichText::new(format!(
                    "At least {} characters – a short sentence you'll remember works well.",
                    storage::MIN_PASSWORD_LEN
                ))
                .small()
                .color(theme::muted(ui)),
            );
            if ui.button("Change password").clicked() {
                action = SettingsAction::ChangePassword;
            }
            if let Some((ok, msg)) = &pw.message {
                ui.colored_label(theme::fg(ui, if *ok { theme::GREEN } else { theme::RED }), msg);
            }
        });

        ui.add_space(8.0);
        theme::card(ui, "Key file (optional)", |ui| {
            if key_file_on {
                ui.colored_label(
                    theme::fg(ui, theme::GREEN),
                    format!("{} On – this data file opens only with your password and the key file on this computer.", icons::KEY),
                );
            } else {
                ui.label("Off – your password alone opens this data file.");
            }
            ui.label(
                RichText::new(
                    "A key file is a small file kept on each computer you use, never next to the data file. \
                     With it, a copy of your data file is useless to anyone who doesn't also have the key file, \
                     even if they guess your password. Use it if your data file is in a synced or shared folder.",
                )
                .small()
                .color(theme::muted(ui)),
            );
            ui.label(
                RichText::new(
                    "You copy it once to each computer you use and keep a backup, for example on a USB stick or in a \
                     password manager. If every copy is lost, your data can't be opened.",
                )
                .small()
                .color(theme::muted(ui)),
            );
            if let Some(path) = storage::key_file_path() {
                ui.label(
                    RichText::new(format!("Key file location on this computer: {}", path.display()))
                        .small()
                        .color(theme::muted(ui)),
                );
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Your password");
                widgets::password_field(ui, egui::TextEdit::singleline(&mut *kf.password).desired_width(200.0));
                if key_file_on {
                    if ui.button("Remove key file").clicked() {
                        action = SettingsAction::RemoveKeyFile;
                    }
                } else if ui.button(format!("{}  Add key file…", icons::KEY)).clicked() {
                    action = SettingsAction::AddKeyFile;
                }
            });
            if let Some((ok, msg)) = &kf.message {
                ui.colored_label(theme::fg(ui, if *ok { theme::GREEN } else { theme::RED }), msg);
            }
        });

        ui.add_space(8.0);
        theme::card(ui, "About", |ui| {
            ui.horizontal(|ui| {
                theme::icon_badge(ui, icons::GRADUATION_CAP, theme::ACCENT, 40.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.label(theme::serif("Supervisor", 21.0));
                    ui.label(RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).color(theme::muted(ui)));
                });
            });
            ui.add_space(4.0);
            ui.label("Tracks project students, placement visits and studies advisees.");
            ui.hyperlink_to(format!("{} github.com/g-machado-uu/supervisor", icons::ARROW_RIGHT), "https://github.com/g-machado-uu/supervisor");
            ui.label(
                RichText::new("Built with egui. Fonts: Inter and DM Serif Display (SIL Open Font License). Icons: Phosphor (MIT).")
                    .small()
                    .color(theme::muted(ui)),
            );
        });
    });
    action
}

/// Returns true if the module should be removed.
fn module_editor(
    ui: &mut Ui,
    module: &mut Module,
    programmes: &[Programme],
    semester_start: Option<chrono::NaiveDate>,
) -> bool {
    let mut remove = false;
    widgets::form_grid(ui, "module-grid", |ui| {
        ui.label("Code");
        ui.add(egui::TextEdit::singleline(&mut module.code).desired_width(120.0));
        ui.end_row();
        ui.label("Name");
        widgets::text_field(ui, &mut module.name, "Module name");
        ui.end_row();
        ui.label("Semester");
        ui.horizontal(|ui| {
            for sem in Semester::ALL {
                ui.selectable_value(&mut module.semester, sem, sem.label());
            }
        });
        ui.end_row();
        ui.label("Applies to");
        ui.horizontal_wrapped(|ui| {
            for p in programmes {
                let mut on = module.applies_to.contains(&p.id);
                if ui
                    .checkbox(&mut on, &p.short)
                    .on_hover_text(&p.name)
                    .changed()
                {
                    if on {
                        module.applies_to.push(p.id.clone());
                    } else {
                        module.applies_to.retain(|id| *id != p.id);
                    }
                }
            }
        });
        ui.end_row();
    });

    ui.label(RichText::new("Courseworks").strong());
    if module.courseworks.is_empty() {
        ui.label(RichText::new("None yet.").italics().color(theme::muted(ui)));
    }
    let mut remove_cw = None;
    egui::Grid::new("cw-template-grid")
        .num_columns(5)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            if !module.courseworks.is_empty() {
                for h in ["Title", "Teaching week", "Fixed deadline", "Resolves to", ""] {
                    ui.label(RichText::new(h).small().color(theme::muted(ui)));
                }
                ui.end_row();
            }
            for (i, cw) in module.courseworks.iter_mut().enumerate() {
                ui.push_id(cw.id, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut cw.title).desired_width(160.0));
                });
                ui.push_id((cw.id, "week"), |ui| {
                    ui.horizontal(|ui| {
                        let mut has_week = cw.week.is_some();
                        if ui.checkbox(&mut has_week, "").changed() {
                            cw.week = has_week.then_some(1);
                        }
                        if let Some(w) = &mut cw.week {
                            ui.add(egui::DragValue::new(w).range(1..=30).prefix("Week "));
                        }
                    });
                });
                widgets::opt_date(ui, (cw.id, "deadline"), &mut cw.deadline);
                ui.label(widgets::fmt_date(cw.resolve_deadline(semester_start)));
                if ui.small_button("🗑").on_hover_text("Remove coursework").clicked() {
                    remove_cw = Some(i);
                }
                ui.end_row();
            }
        });
    if let Some(i) = remove_cw {
        module.courseworks.remove(i);
    }
    ui.horizontal(|ui| {
        if ui.button("➕ Add coursework").clicked() {
            module.courseworks.push(CourseworkTemplate::default());
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button(RichText::new("Remove module").color(theme::fg(ui, theme::RED)))
                .on_hover_text("Double-click to remove this module")
                .double_clicked()
            {
                remove = true;
            }
        });
    });
    remove
}

/// Courses whose project students are supervised; each list name becomes a
/// sidebar list. Courses with students can't be removed.
fn courses_editor(ui: &mut Ui, programmes: &mut Vec<Programme>, in_use: &HashMap<String, usize>) {
    ui.label(
        RichText::new("Courses sharing a list name are shown together in the sidebar. Add their modules and coursework below.")
            .small()
            .color(theme::muted(ui)),
    );
    let mut remove = None;
    egui::Grid::new("courses-grid")
        .num_columns(5)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            for h in ["Name", "Short name", "Sidebar list", "Students", ""] {
                ui.label(RichText::new(h).small().color(theme::muted(ui)));
            }
            ui.end_row();
            for (i, p) in programmes.iter_mut().enumerate() {
                ui.push_id(&p.id, |ui| {
                    ui.add_sized([220.0, 22.0], egui::TextEdit::singleline(&mut p.name));
                });
                ui.push_id((&p.id, "short"), |ui| {
                    ui.add_sized([90.0, 22.0], egui::TextEdit::singleline(&mut p.short));
                });
                ui.push_id((&p.id, "list"), |ui| {
                    ui.add_sized([90.0, 22.0], egui::TextEdit::singleline(&mut p.list));
                });
                let used = in_use.get(&p.id).copied().unwrap_or(0);
                ui.label(used.to_string());
                let button = ui.add_enabled(used == 0, egui::Button::new("🗑").small());
                if button
                    .on_hover_text("Remove course")
                    .on_disabled_hover_text("Students are on this course – move them first")
                    .clicked()
                {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
    if let Some(i) = remove {
        programmes.remove(i);
    }
    if ui.button("➕ Add course").clicked() {
        programmes.push(Programme::default());
    }
}
