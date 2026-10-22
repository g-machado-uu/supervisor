//! Placement section: company details and the two visits.

use egui::{RichText, Ui};

use crate::model::{PlacementRecord, ReplyStatus, VisitMode};
use crate::ui::{theme, widgets};

pub fn show(ui: &mut Ui, p: &mut PlacementRecord) {
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
            egui::Frame::group(ui.style())
                .corner_radius(egui::CornerRadius::same(8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("Visit {}", i + 1)).strong());
                        if v.completed {
                            theme::chip(ui, "completed", theme::GREEN);
                        }
                    });
                    widgets::form_grid(ui, "visit-grid", |ui| {
                        ui.label("Mode");
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut v.mode, VisitMode::Online, "💻 Online");
                            ui.selectable_value(&mut v.mode, VisitMode::FaceToFace, "🏢 Face-to-face");
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
}
