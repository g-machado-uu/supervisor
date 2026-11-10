//! Marking section: a student whose work I mark without supervising them.
//! As second marker, a final-year project's poster and dissertation (BEng
//! or MEng) or an MSc dissertation; on my own, a final placement report.

use egui::{RichText, Ui};

use crate::model::{CwStatus, MarkingKind, MarkingRecord, Markers, Settings};
use crate::ui::fyp::{self, Cols};
use crate::ui::student_form::Shown;
use crate::ui::{icons, theme, widgets};

pub fn show(ui: &mut Ui, m: &mut MarkingRecord, settings: &Settings, shown: &mut Shown) {
    widgets::form_grid(ui, "marking-grid", |ui| {
        ui.label("Marking");
        let options = MarkingKind::ALL.map(|k| (k, k.short()));
        theme::segmented(ui, &mut m.kind, &options);
        ui.end_row();

        if m.kind == MarkingKind::Project {
            ui.label("Course");
            let current = settings
                .programme(&m.programme)
                .map_or_else(|| if m.programme.is_empty() { "Choose…".into() } else { format!("{} (removed)", m.programme) }, |p| p.name.clone());
            egui::ComboBox::from_id_salt("marking-course")
                .selected_text(current)
                .width(260.0)
                .show_ui(ui, |ui| {
                    // SUST projects have no second marker here.
                    for p in settings.programmes.iter().filter(|p| !settings.is_sust(&p.id)) {
                        ui.selectable_value(&mut m.programme, p.id.clone(), &p.name);
                    }
                });
            ui.end_row();
        }

        let staff = m.kind.staff();
        ui.label(staff);
        widgets::text_field(ui, &mut m.supervisor_name, "Name");
        ui.end_row();
        ui.label(format!("{staff} email"));
        widgets::text_field(ui, &mut m.supervisor_email, "Email");
        ui.end_row();

        if m.kind == MarkingKind::Placement {
            ui.label("Company");
            widgets::text_field(ui, &mut m.company, "Placement company");
        } else {
            ui.label("Title");
            widgets::text_field(ui, &mut m.title, "Project or dissertation title");
        }
        ui.end_row();

        if m.kind == MarkingKind::Project {
            ui.label("Poster number");
            ui.horizontal(|ui| fyp::poster_field(ui, &mut m.poster_number));
            ui.end_row();
        }
    });

    ui.add_space(10.0);
    let marks_shown = shown.marks;
    let mut switch_marks = false;
    theme::section(ui, "Marks", |ui| {
        // (title, module code, who marks it, its status)
        let items: Vec<(String, String, Markers, Option<uuid::Uuid>)> = match m.kind.report() {
            Some((title, markers)) => vec![(title.to_string(), String::new(), markers, None)],
            None => settings
                .second_marked(&m.programme)
                .into_iter()
                .map(|(module, cw)| (cw.title.clone(), module.code.clone(), cw.markers, Some(cw.id)))
                .collect(),
        };
        if items.is_empty() {
            let text = if m.programme.is_empty() {
                "Choose the course above to see what to mark."
            } else {
                "No coursework of this course is marked by the second marker – set who marks each coursework in \
                 Settings › Modules."
            };
            ui.label(RichText::new(text).italics().color(theme::muted(ui)));
            return;
        }
        let cols = Cols {
            first: items.iter().any(|(_, _, markers, _)| markers.supervisor()),
            agreed: items.iter().any(|(_, _, markers, _)| *markers == Markers::Both),
            second_first: false,
        };
        egui::Grid::new("marking-marks")
            .num_columns(2 + usize::from(cols.first) + usize::from(cols.agreed))
            .striped(true)
            .spacing([10.0, 6.0])
            .show(ui, |ui| {
                let (text, hover) = if marks_shown {
                    (format!("{} Hide marks", icons::EYE_SLASH), "Show the marks as dots again")
                } else {
                    (format!("{} Reveal marks", icons::EYE), "Show the marks to read or type them in")
                };
                if ui.small_button(text).on_hover_text(hover).clicked() {
                    switch_marks = true;
                }
                let staff = if m.kind == MarkingKind::Placement { "Visitor" } else { "Supervisor" };
                let headings = [(cols.first, staff), (true, "My mark"), (cols.agreed, "Agreed")];
                for (_, h) in headings.iter().filter(|(on, _)| *on) {
                    ui.label(RichText::new(*h).small().color(theme::muted(ui)));
                }
                ui.end_row();
                for (title, code, markers, id) in items {
                    ui.horizontal(|ui| {
                        ui.label(title);
                        if !code.is_empty() {
                            ui.label(RichText::new(code).small().color(theme::muted(ui)));
                        }
                    });
                    let edit = |status: &mut CwStatus| {
                        switch_marks |= fyp::marks_cells(ui, status, markers, marks_shown, "the supervisor's mark", cols);
                    };
                    match id {
                        Some(id) => fyp::edit_status(&mut m.coursework, id, edit),
                        None => {
                            let mut edit = edit;
                            edit(&mut m.report);
                        }
                    }
                    ui.end_row();
                }
            });
    });
    if switch_marks {
        shown.marks = !shown.marks;
    }
}
