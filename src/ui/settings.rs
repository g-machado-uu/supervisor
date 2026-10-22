//! Settings: academic calendar, module/coursework templates and security.

use egui::{RichText, Ui};
use zeroize::Zeroizing;

use crate::model::{CourseworkTemplate, FypKind, Module, Semester, Settings};
use crate::ui::{theme, widgets};

#[derive(Default)]
pub struct PasswordForm {
    pub current: Zeroizing<String>,
    pub new: Zeroizing<String>,
    pub confirm: Zeroizing<String>,
    pub message: Option<(bool, String)>,
}

pub enum SettingsAction {
    None,
    ChangePassword,
    ExportIcs,
}

pub fn show(ui: &mut Ui, settings: &mut Settings, data_path: &str, pw: &mut PasswordForm) -> SettingsAction {
    let mut action = SettingsAction::None;
    ui.heading("Settings");
    ui.add_space(6.0);
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        theme::card(ui, "Academic year", |ui| {
            widgets::form_grid(ui, "year-grid", |ui| {
                ui.label("Academic year");
                ui.add(egui::TextEdit::singleline(&mut settings.academic_year).desired_width(120.0));
                ui.end_row();
                ui.label("Semester 1 – week 1 starts");
                widgets::opt_date(ui, "sem1", &mut settings.sem1_start);
                ui.end_row();
                ui.label("Semester 2 – week 1 starts");
                widgets::opt_date(ui, "sem2", &mut settings.sem2_start);
                ui.end_row();
                ui.label("Dashboard look-ahead");
                ui.add(egui::DragValue::new(&mut settings.upcoming_days).range(1..=120).suffix(" days"));
                ui.end_row();
                ui.label("Appearance");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut settings.dark_mode, false, "☀ Light");
                    ui.selectable_value(&mut settings.dark_mode, true, "🌙 Dark");
                });
                ui.end_row();
            });
            ui.label(
                RichText::new("Coursework set by teaching week is due at the start of that week (Monday if week 1 starts on a Monday). Give an explicit date to override.")
                    .small()
                    .color(theme::GREY),
            );
        });

        ui.add_space(8.0);
        theme::card(ui, "Modules & coursework", |ui| {
            ui.label(
                RichText::new("Each module applies to one or more FYP types. SUST modules are kept separate so their dates and coursework can differ.")
                    .small()
                    .color(theme::GREY),
            );
            let starts = (settings.sem1_start, settings.sem2_start);
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
                            if module_editor(ui, module, start) {
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
        theme::card(ui, "Calendar", |ui| {
            if ui.button("📅 Export calendar (.ics)…").clicked() {
                action = SettingsAction::ExportIcs;
            }
            ui.label(
                RichText::new("Exports meetings, visits and coursework deadlines for Outlook / Google Calendar. The exported file is not encrypted.")
                    .small()
                    .color(theme::GREY),
            );
        });

        ui.add_space(8.0);
        theme::card(ui, "Security", |ui| {
            ui.label(format!("Data file: {data_path}"));
            ui.label(
                RichText::new("Encrypted with Argon2id + XChaCha20-Poly1305. A backup of the previous save is kept alongside it (.bak).")
                    .small()
                    .color(theme::GREY),
            );
            ui.add_space(6.0);
            ui.label(RichText::new("Change password").strong());
            widgets::form_grid(ui, "pw-grid", |ui| {
                for (label, field) in [
                    ("Current password", &mut pw.current),
                    ("New password", &mut pw.new),
                    ("Confirm new password", &mut pw.confirm),
                ] {
                    ui.label(label);
                    ui.add(egui::TextEdit::singleline(&mut **field).password(true).desired_width(240.0));
                    ui.end_row();
                }
            });
            if ui.button("Change password").clicked() {
                action = SettingsAction::ChangePassword;
            }
            if let Some((ok, msg)) = &pw.message {
                ui.colored_label(if *ok { theme::GREEN } else { theme::RED }, msg);
            }
        });
    });
    action
}

/// Returns true if the module should be removed.
fn module_editor(ui: &mut Ui, module: &mut Module, semester_start: Option<chrono::NaiveDate>) -> bool {
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
            for kind in FypKind::ALL {
                let mut on = module.applies_to.contains(&kind);
                if ui.checkbox(&mut on, kind.short()).changed() {
                    if on {
                        module.applies_to.push(kind);
                    } else {
                        module.applies_to.retain(|k| *k != kind);
                    }
                }
            }
        });
        ui.end_row();
    });

    ui.label(RichText::new("Courseworks").strong());
    if module.courseworks.is_empty() {
        ui.label(RichText::new("None yet.").italics().color(theme::GREY));
    }
    let mut remove_cw = None;
    egui::Grid::new("cw-template-grid")
        .num_columns(5)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            if !module.courseworks.is_empty() {
                for h in ["Title", "Teaching week", "Fixed deadline", "Resolves to", ""] {
                    ui.label(RichText::new(h).small().color(theme::GREY));
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
                .small_button(RichText::new("Remove module").color(theme::RED))
                .on_hover_text("Double-click to remove this module")
                .double_clicked()
            {
                remove = true;
            }
        });
    });
    remove
}
