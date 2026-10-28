//! Project supervision section (BEng, MEng, …) of the student detail panel.

use std::collections::HashMap;

use egui::{RichText, Ui};
use uuid::Uuid;

use crate::model::{CwStatus, FypRecord, MARK_TOLERANCE, ReplyStatus, Settings, fmt_mark};
use crate::ui::student_form::{Shown, hidden_value};
use crate::ui::{icons, theme, widgets};

/// Returns the meeting to send, if asked.
pub fn show(ui: &mut Ui, fyp: &mut FypRecord, settings: &Settings, shown: &mut Shown) -> Option<widgets::Share> {
    let sust = settings.is_sust(&fyp.programme);
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

        if sust {
            ui.label("SUST email");
            if shown.details {
                widgets::text_field(ui, &mut fyp.sust_email, "If known");
            } else {
                hidden_value(ui, &fyp.sust_email);
            }
            ui.end_row();
        }
        let marker = if sust { "SUST supervisor" } else { "Second marker" };
        ui.label(marker);
        widgets::text_field(ui, &mut fyp.second_marker_name, "Name");
        ui.end_row();
        ui.label(format!("{marker} email"));
        widgets::text_field(ui, &mut fyp.second_marker_email, "Email");
        ui.end_row();

        if sust {
            sust_contact(ui, fyp);
        } else {
            ui.label("Initial contact");
            ui.horizontal(|ui| {
                ui.checkbox(&mut fyp.initial_contact, "Made contact");
                if fyp.initial_contact {
                    widgets::opt_date(ui, "initial-contact", &mut fyp.initial_contact_on);
                }
            });
            ui.end_row();
        }
    });

    ui.add_space(10.0);
    let marks_shown = shown.marks;
    // Clicking Reveal/Hide, or a hidden mark, switches all marks of the student.
    let mut switch_marks = false;
    theme::section(ui, "Coursework", |ui| {
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
                        ui.label(&cw.title);
                        edit_status(&mut fyp.coursework, cw.id, |status| {
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
                        });
                        ui.end_row();
                    }
                });
            ui.add_space(6.0);
            egui::Grid::new(("marks-grid", module.id))
                .num_columns(if sust { 5 } else { 4 })
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
                    let headings: &[&str] = if sust {
                        &["Sent by SUST", "SUST mark", "My mark", "Agreed"]
                    } else {
                        &["Marker 1", "Marker 2", "Agreed"]
                    };
                    for h in headings {
                        ui.label(RichText::new(*h).small().color(theme::muted(ui)));
                    }
                    ui.end_row();
                    for cw in &module.courseworks {
                        ui.label(&cw.title);
                        edit_status(&mut fyp.coursework, cw.id, |status| {
                            let before = status.clone();
                            let mut mark = |ui: &mut Ui, value: &mut String| {
                                switch_marks |= mark(ui, marks_shown, value);
                            };
                            if sust {
                                ui.checkbox(&mut status.marks_received, "")
                                    .on_hover_text("The SUST supervisor has sent their marks");
                                mark(ui, &mut status.mark_2);
                                mark(ui, &mut status.mark_1);
                            } else {
                                mark(ui, &mut status.mark_1);
                                mark(ui, &mut status.mark_2);
                            }
                            ui.horizontal(|ui| {
                                mark(ui, &mut status.agreed_mark);
                                agreed_note(ui, status, marks_shown, if sust { "my mark" } else { "Marker 1" });
                            });
                            if (&status.mark_1, &status.mark_2) != (&before.mark_1, &before.mark_2) {
                                status.suggest_agreed();
                            } else if status.agreed_mark != before.agreed_mark {
                                status.agreed_unconfirmed = false;
                            }
                        });
                        ui.end_row();
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
    });

    if switch_marks {
        shown.marks = !shown.marks;
    }

    ui.add_space(10.0);
    theme::section(ui, "Meetings", |ui| {
        widgets::meetings_editor(ui, "fyp-meetings", &mut fyp.meetings, None, settings)
    })
    .flatten()
}

/// For SUST students, the first contact as for advisees: when I wrote,
/// their reply and when it came.
fn sust_contact(ui: &mut Ui, fyp: &mut FypRecord) {
    let before = (fyp.initial_contact_on, fyp.reply);
    ui.label("I contacted them");
    widgets::opt_date(ui, "initial-contact", &mut fyp.initial_contact_on);
    ui.end_row();

    ui.label("Their reply");
    widgets::reply_status(ui, &mut fyp.reply);
    ui.end_row();

    if fyp.reply == ReplyStatus::Replied {
        ui.label("Replied on");
        widgets::opt_date(ui, "replied", &mut fyp.replied_on);
        ui.end_row();
    }
    // Only on a change, so contact ticked in older versions without a date stays.
    if (fyp.initial_contact_on, fyp.reply) != before {
        fyp.initial_contact = fyp.initial_contact_on.is_some() || fyp.reply != ReplyStatus::Awaiting;
    }
}

/// Edits a coursework's status; viewing it doesn't create (and save) an empty one.
fn edit_status(coursework: &mut HashMap<Uuid, CwStatus>, id: Uuid, edit: impl FnOnce(&mut CwStatus)) {
    let original = coursework.get(&id).cloned().unwrap_or_default();
    let mut edited = original.clone();
    edit(&mut edited);
    if edited != original {
        coursework.insert(id, edited);
    }
}

/// Beside the agreed mark: a warning when marker 2's mark is far from
/// marker 1's (`reference`), or Confirm for an average filled in. Shown as
/// icons while marks are hidden.
fn agreed_note(ui: &mut Ui, status: &mut CwStatus, shown: bool, reference: &str) {
    if let Some(gap) = status.marks_apart() {
        let text = if shown {
            format!("{} {}% from {reference}", icons::WARNING, fmt_mark(gap))
        } else {
            icons::WARNING.to_string()
        };
        ui.colored_label(theme::fg(ui, theme::AMBER), text).on_hover_text(format!(
            "The other mark is more than {}% away from {reference}: agree the mark together",
            fmt_mark(MARK_TOLERANCE * 100.0)
        ));
    } else if status.agreed_unconfirmed {
        let hover = "The average of the two marks, filled in for you. Confirm it, or type another mark.";
        if !shown {
            ui.colored_label(theme::fg(ui, theme::AMBER), icons::HOURGLASS).on_hover_text("Agreed mark to confirm");
        } else if ui
            .small_button(RichText::new(format!("{} Confirm", icons::CHECK)).color(theme::fg(ui, theme::AMBER)))
            .on_hover_text(hover)
            .clicked()
        {
            status.agreed_unconfirmed = false;
        }
    }
}

/// A mark, as dots until the marks are revealed. Returns true when a
/// hidden one is clicked, to reveal them.
fn mark(ui: &mut Ui, shown: bool, value: &mut String) -> bool {
    if shown {
        ui.add(egui::TextEdit::singleline(value).hint_text("–").desired_width(56.0));
        return false;
    }
    let text = if value.trim().is_empty() { "–".to_string() } else { "•".repeat(8) };
    ui.add(egui::Label::new(RichText::new(text).monospace().color(theme::muted(ui))).sense(egui::Sense::click()))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("Click to reveal the marks")
        .clicked()
}
