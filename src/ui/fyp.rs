//! Project supervision section (BEng, MEng, …) of the student detail panel.

use egui::{RichText, Ui};

use crate::model::{FypRecord, Settings};
use crate::ui::{theme, widgets};

/// Returns the UID of a meeting to save an invite for.
pub fn show(ui: &mut Ui, fyp: &mut FypRecord, settings: &Settings) -> Option<String> {
    widgets::form_grid(ui, "fyp-grid", |ui| {
        ui.label("Course");
        let current = settings.programme(&fyp.programme).map_or_else(
            || format!("{} (removed)", fyp.programme),
            |p| p.name.clone(),
        );
        egui::ComboBox::from_id_salt("fyp-kind")
            .selected_text(current)
            .width(260.0)
            .show_ui(ui, |ui| {
                for p in &settings.programmes {
                    ui.selectable_value(&mut fyp.programme, p.id.clone(), &p.name);
                }
            });
        ui.end_row();

        ui.label("Project title");
        widgets::text_field(ui, &mut fyp.project_title, "Project title");
        ui.end_row();

        let sust = settings.is_sust(&fyp.programme);
        if sust {
            ui.label("SUST email");
            widgets::text_field(ui, &mut fyp.sust_email, "If known");
            ui.end_row();
        }
        let marker = if sust { "SUST supervisor" } else { "Second marker" };
        ui.label(marker);
        widgets::text_field(ui, &mut fyp.second_marker_name, "Name");
        ui.end_row();
        ui.label(format!("{marker} email"));
        widgets::text_field(ui, &mut fyp.second_marker_email, "Email");
        ui.end_row();

        ui.label("Initial contact");
        ui.horizontal(|ui| {
            ui.checkbox(&mut fyp.initial_contact, "Made contact");
            if fyp.initial_contact {
                widgets::opt_date(ui, "initial-contact", &mut fyp.initial_contact_on);
            }
        });
        ui.end_row();
    });

    ui.add_space(10.0);
    theme::card_title(ui, "Coursework");
    let today = widgets::today();
    let mut any_module = false;
    for module in settings.modules_for(&fyp.programme) {
        any_module = true;
        ui.label(
            theme::semibold(format!("{} – {} · {}", module.code, module.name, module.semester.label()))
                .color(theme::fg(ui, theme::ACCENT)),
        );
        if module.courseworks.is_empty() {
            ui.label(
                RichText::new("No courseworks defined yet – add them in Settings › Modules.")
                    .italics()
                    .color(theme::muted(ui)),
            );
            continue;
        }
        egui::Grid::new(("cw-grid", module.id))
            .num_columns(4)
            .striped(true)
            .spacing([10.0, 6.0])
            .show(ui, |ui| {
                ui.label(RichText::new("Item").small().color(theme::muted(ui)));
                ui.label(RichText::new("Deadline").small().color(theme::muted(ui)));
                ui.label(RichText::new("Extension").small().color(theme::muted(ui)));
                ui.label(RichText::new("Submitted").small().color(theme::muted(ui)));
                ui.end_row();
                for cw in &module.courseworks {
                    // Edit a copy so merely viewing doesn't create (and save) empty entries.
                    let original = fyp.coursework.get(&cw.id).cloned().unwrap_or_default();
                    let mut edited = original.clone();
                    let status = &mut edited;
                    ui.label(&cw.title);
                    match settings.effective_deadline(module, cw, Some(status)) {
                        Some(due) => {
                            let color = if status.submitted {
                                theme::fg(ui, theme::GREEN)
                            } else if due < today {
                                theme::fg(ui, theme::RED)
                            } else {
                                ui.visuals().text_color()
                            };
                            ui.colored_label(color, widgets::fmt_date(Some(due)));
                        }
                        None => {
                            ui.label(RichText::new("TBC").color(theme::muted(ui)));
                        }
                    }
                    widgets::opt_date(ui, ("ext", cw.id), &mut status.deadline_override);
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut status.submitted, "").changed()
                            && status.submitted
                            && status.submitted_on.is_none()
                        {
                            status.submitted_on = Some(today);
                        }
                        if status.submitted {
                            widgets::opt_date(ui, ("sub", cw.id), &mut status.submitted_on);
                        }
                    });
                    ui.end_row();
                    if edited != original {
                        fyp.coursework.insert(cw.id, edited);
                    }
                }
            });
    }
    if !any_module {
        ui.label(
            RichText::new("No modules are linked to this course – add them in Settings › Modules.")
                .italics()
                .color(theme::muted(ui)),
        );
    }

    ui.add_space(10.0);
    theme::card_title(ui, "Meetings");
    widgets::meetings_editor(ui, "fyp-meetings", &mut fyp.meetings, None, settings)
}
