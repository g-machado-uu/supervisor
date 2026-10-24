//! Placement section: company details and the two visits.

use egui::Ui;

use uuid::Uuid;

use crate::model::{PlacementRecord, ReplyStatus, VisitMode, visit_uid};
use crate::ui::{icons, theme, widgets};

/// Returns the UID of a visit to save an invite for.
pub fn show(ui: &mut Ui, p: &mut PlacementRecord, student_id: Uuid) -> Option<String> {
    let mut invite = None;
    widgets::form_grid(ui, "placement-grid", |ui| {
        ui.label("Company");
        widgets::text_field(ui, &mut p.company, "Placement company");
        ui.end_row();
        ui.label("Professional email");
        widgets::text_field(ui, &mut p.professional_email, "name@company.com");
        ui.end_row();
        ui.label("Line manager");
        widgets::text_field(ui, &mut p.line_manager_name, "Name");
        ui.end_row();
        ui.label("Line manager email");
        widgets::text_field(ui, &mut p.line_manager_email, "manager@company.com");
        ui.end_row();
    });

    for (i, v) in p.visits.iter_mut().enumerate() {
        ui.add_space(6.0);
        ui.push_id(("visit", i), |ui| {
            widgets::inner_frame(ui)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(theme::semibold(format!("Visit {}", i + 1)));
                        if v.completed {
                            theme::chip(ui, "completed", theme::GREEN);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if widgets::invite_button(ui, v.date.is_some()) {
                                invite = Some(visit_uid(student_id, i));
                            }
                        });
                    });
                    widgets::form_grid(ui, "visit-grid", |ui| {
                        ui.label("Mode");
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut v.mode, VisitMode::Online, format!("{} Online", icons::DESKTOP));
                            ui.selectable_value(&mut v.mode, VisitMode::FaceToFace, format!("{} Face-to-face", icons::BUILDINGS));
                        });
                        ui.end_row();

                        ui.label("Request sent");
                        widgets::opt_date(ui, "request", &mut v.request_sent);
                        ui.end_row();

                        ui.label("Student response");
                        widgets::reply_status(ui, &mut v.response);
                        ui.end_row();

                        if v.response == ReplyStatus::Replied {
                            ui.label("Responded on");
                            widgets::opt_date(ui, "responded", &mut v.responded_on);
                            ui.end_row();
                        }

                        ui.label("Visit date / time");
                        ui.horizontal(|ui| {
                            widgets::opt_date(ui, "date", &mut v.date);
                            widgets::opt_time(ui, "time", &mut v.time);
                        });
                        ui.end_row();

                        ui.label("Line manager attending");
                        widgets::tri_state(ui, &mut v.line_manager_attending);
                        ui.end_row();

                        ui.label("Completed");
                        ui.checkbox(&mut v.completed, "Visit done");
                        ui.end_row();
                    });
                    widgets::notes(ui, &mut v.notes, "Visit notes…");
                });
        });
    }
    invite
}
