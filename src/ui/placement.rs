//! Placement section: company details and the two visits.

use egui::{RichText, Ui};

use uuid::Uuid;

use crate::model::{PlacementRecord, ReplyStatus, Settings, Timetable, VisitMode, visit_uid};
use crate::ui::student_form::Shown;
use crate::ui::{fyp, icons, theme, widgets};

/// The placement module, and whether they are enrolled on it.
fn placement_module(ui: &mut Ui, p: &mut PlacementRecord, settings: &Settings) {
    let modules: Vec<_> = settings.placement_modules().collect();
    ui.label("Placement module");
    let label = |m: &crate::model::Module| format!("{} – {}", m.code, m.name);
    match modules.as_slice() {
        [] => {
            ui.label(
                RichText::new("None set up – add it in Settings › Modules & coursework, ticking Placement")
                    .italics()
                    .color(theme::muted(ui)),
            );
        }
        [only] => {
            ui.label(label(only));
        }
        _ => {
            let current = p.placement_module(settings).map_or_else(|| "Choose…".to_string(), label);
            egui::ComboBox::from_id_salt("placement-module")
                .selected_text(current)
                .width(260.0)
                .show_ui(ui, |ui| {
                    for m in &modules {
                        ui.selectable_value(&mut p.module, Some(m.id), label(m));
                    }
                });
        }
    }
    ui.end_row();
    if !modules.is_empty() {
        ui.label("Enrolled on it");
        widgets::tri_state(ui, &mut p.enrolled);
        ui.end_row();
    }
}

/// Returns the visit to send, if asked.
pub fn show(
    ui: &mut Ui,
    p: &mut PlacementRecord,
    settings: &Settings,
    timetable: &Timetable,
    student_id: Uuid,
    shown: &mut Shown,
) -> Option<widgets::Share> {
    let mut invite = None;
    let mut switch_marks = false;
    let module = p.placement_module(settings).cloned();
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
        placement_module(ui, p, settings);
        // Placements have no second marker; kept only for what older versions stored.
        if !p.second_marker_name.trim().is_empty() || !p.second_marker_email.trim().is_empty() {
            ui.label("Second marker");
            widgets::text_field(ui, &mut p.second_marker_name, "Name");
            ui.end_row();
            ui.label("Second marker email");
            widgets::text_field(ui, &mut p.second_marker_email, "Email");
            ui.end_row();
        }
    });

    for (i, v) in p.visits.iter_mut().enumerate() {
        ui.add_space(6.0);
        ui.push_id(("visit", i), |ui| {
            widgets::inner_frame(ui)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.label(theme::semibold(format!("Visit {}", i + 1)));
                        if v.completed {
                            theme::chip(ui, "completed", theme::GREEN);
                        } else if let Some(by) = module.as_ref().and_then(|m| m.visit_by[i]) {
                            let late = by < widgets::today() || v.date.is_some_and(|d| d > by);
                            let color = if late { theme::fg(ui, theme::RED) } else { theme::muted(ui) };
                            ui.label(RichText::new(format!("to be done by {}", widgets::fmt_short(by))).small().color(color))
                                .on_hover_text(format!("Set with the placement module in Settings: {}", widgets::fmt_date(Some(by))));
                        }
                        if let Some(share) = widgets::share_buttons(ui, v.date.is_some(), || visit_uid(student_id, i)) {
                            invite = Some(share);
                        }
                    });
                    widgets::form_grid(ui, "visit-grid", |ui| {
                        ui.label("Mode");
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut v.mode, VisitMode::Online, format!("{} Online", icons::DESKTOP));
                            ui.selectable_value(&mut v.mode, VisitMode::FaceToFace, format!("{} Face-to-face", icons::BUILDINGS));
                        });
                        ui.end_row();

                        ui.label("Request sent");
                        let replied = v.responded_on.filter(|_| v.response == ReplyStatus::Replied);
                        widgets::contact_date(ui, "request", &mut v.request_sent, replied);
                        ui.end_row();

                        ui.label("Student response");
                        widgets::reply_status(ui, &mut v.response);
                        ui.end_row();

                        if v.response == ReplyStatus::Replied {
                            ui.label("Responded on");
                            widgets::reply_date(ui, "responded", &mut v.responded_on, v.request_sent);
                            ui.end_row();
                        }

                        ui.label("Visit date / time");
                        let clash = (!v.completed).then_some((settings, timetable));
                        widgets::date_time_cell(ui, &mut v.date, &mut v.time, clash);
                        ui.end_row();

                        ui.label("Line manager attending");
                        widgets::tri_state(ui, &mut v.line_manager_attending);
                        ui.end_row();

                        ui.label("Completed");
                        ui.checkbox(&mut v.completed, "Visit done");
                        ui.end_row();

                        ui.label("My mark");
                        ui.horizontal(|ui| {
                            switch_marks |= fyp::mark_of_width(ui, shown.marks, &mut v.mark, 56.0);
                            let text = if shown.marks { "Hide marks" } else { "Reveal marks" };
                            if ui.small_button(text).clicked() {
                                switch_marks = true;
                            }
                            let by = module.as_ref().and_then(|m| m.visit_mark_by[i]);
                            fyp::mark_by_note(ui, by, !v.mark.trim().is_empty());
                        });
                        ui.end_row();
                    });
                    widgets::notes(ui, &mut v.notes, "Visit notes…");
                });
        });
    }
    if switch_marks {
        shown.marks = !shown.marks;
    }
    invite
}
