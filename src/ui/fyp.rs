//! Final Year Project section of the student detail panel.

use egui::{RichText, Ui};

use crate::model::{FypKind, FypRecord, Settings};
use crate::ui::{theme, widgets};

pub fn show(ui: &mut Ui, fyp: &mut FypRecord, settings: &Settings) {
    widgets::form_grid(ui, "fyp-grid", |ui| {
        ui.label("Type");
        egui::ComboBox::from_id_salt("fyp-kind")
            .selected_text(fyp.kind.label())
            .width(260.0)
            .show_ui(ui, |ui| {
                for kind in FypKind::ALL {
                    ui.selectable_value(&mut fyp.kind, kind, kind.label());
                }
            });
        ui.end_row();

        ui.label("Project title");
        widgets::text_field(ui, &mut fyp.project_title, "Project title");
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

    ui.add_space(6.0);
    ui.label(RichText::new("Coursework").strong());
    let today = widgets::today();
    let mut any_module = false;
    for module in settings.modules_for(fyp.kind) {
        any_module = true;
        ui.label(
            RichText::new(format!("{} – {} · {}", module.code, module.name, module.semester.label()))
                .color(theme::ACCENT),
        );
        if module.courseworks.is_empty() {
            ui.label(
                RichText::new("No courseworks defined yet – add them in Settings › Modules.")
                    .italics()
                    .color(theme::GREY),
            );
            continue;
        }
        egui::Grid::new(("cw-grid", module.id))
            .num_columns(4)
            .striped(true)
            .spacing([10.0, 6.0])
            .show(ui, |ui| {
                ui.label(RichText::new("Item").small().color(theme::GREY));
                ui.label(RichText::new("Deadline").small().color(theme::GREY));
                ui.label(RichText::new("Extension").small().color(theme::GREY));
                ui.label(RichText::new("Submitted").small().color(theme::GREY));
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
                                theme::GREEN
                            } else if due < today {
                                theme::RED
                            } else {
                                ui.visuals().text_color()
                            };
                            ui.colored_label(color, widgets::fmt_date(Some(due)));
                        }
                        None => {
                            ui.label(RichText::new("TBC").color(theme::GREY));
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
            RichText::new("No modules are linked to this FYP type – check Settings › Modules.")
                .italics()
                .color(theme::GREY),
        );
    }

    ui.add_space(6.0);
    ui.label(RichText::new("Meetings").strong());
    widgets::meetings_editor(ui, "fyp-meetings", &mut fyp.meetings, None);
}
