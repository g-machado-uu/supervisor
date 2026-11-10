//! Card of a local student's modules carried from the previous semester.
//!
//! It sits in the narrow detail panel, whose width follows what is in it, so
//! everything here is stacked vertically and each field takes its width from
//! the room there is: nothing typed in can make the panel wider.

use egui::{RichText, Ui};

use crate::model::{Escalation, FailedComponent, FailedModule, ModulesCheck, SemesterModules, Settings, Student};
use crate::ui::student_form::Shown;
use crate::ui::{fyp, icons, theme, widgets};

/// Width of the labels to the left of a module's fields.
const LABEL_WIDTH: f32 = 76.0;
/// Room for a mark with decimals, such as 100.00, and for the dots hiding it.
const MARK_WIDTH: f32 = 84.0;

/// Collapsed until asked for, separately for each student.
pub fn card(ui: &mut Ui, s: &mut Student, settings: &Settings, shown: &mut Shown) {
    let mut title = format!("{}  Modules", icons::BOOK_OPEN_TEXT);
    if shown.details {
        for (label, semester) in [("S1", &s.modules.s1), ("S2", &s.modules.s2)] {
            if semester.pending() > 0 {
                title.push_str(&format!(" · {label}: {} pending", semester.pending()));
            }
        }
    }
    let advisee = s.advisee.is_some();
    theme::card_frame(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::CollapsingHeader::new(theme::semibold(title).size(15.0).color(theme::fg(ui, theme::ACCENT)))
            .id_salt(("modules-card", s.id))
            .default_open(false)
            .show(ui, |ui| {
                let text = if shown.marks {
                    format!("{} Hide marks", icons::EYE_SLASH)
                } else {
                    format!("{} Reveal marks", icons::EYE)
                };
                let mut switch_marks = ui.small_button(text).clicked();
                ui.add_space(4.0);
                switch_marks |= semester(ui, "Semester 1", &mut s.modules.s1, false, settings, shown.marks);
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);
                // Only an advisee could have talked it over as one.
                switch_marks |= semester(ui, "Semester 2", &mut s.modules.s2, advisee, settings, shown.marks);
                if switch_marks {
                    shown.marks = !shown.marks;
                }
            });
    });
}

/// Returns true when a hidden mark is clicked, to reveal the marks.
fn semester(ui: &mut Ui, title: &str, m: &mut SemesterModules, ask_discussed: bool, settings: &Settings, marks: bool) -> bool {
    let mut reveal = false;
    ui.push_id(title, |ui| {
        ui.label(theme::semibold(title));
        wrapped(ui, RichText::new("Modules pending from the previous semester").small().color(theme::muted(ui)));
        let before = m.check;
        theme::segmented(
            ui,
            &mut m.check,
            &[
                (ModulesCheck::NotChecked, "Not checked"),
                (ModulesCheck::AllPassed, "All passed"),
                (ModulesCheck::Pending, "Pending"),
            ],
        );
        if m.check != ModulesCheck::Pending {
            return;
        }
        // Answering "Pending" means there is at least one.
        if before != ModulesCheck::Pending && m.failed.is_empty() {
            m.failed.push(FailedModule::default());
        }
        how_many(ui, &mut m.failed);
        if let Some(who) = settings.escalation(m.failed.len()) {
            let color = match who {
                Escalation::Advisor => theme::ACCENT,
                Escalation::CourseDirector => theme::AMBER,
                Escalation::HeadOfSchool => theme::RED,
            };
            let text = format!("{} Needs to talk to {}", icons::USERS, who.label());
            wrapped(ui, RichText::new(text).color(theme::fg(ui, color)));
        }
        let mut remove = None;
        for (i, module) in m.failed.iter_mut().enumerate() {
            ui.push_id(i, |ui| {
                widgets::inner_frame(ui).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(theme::semibold(format!("Module {}", i + 1)));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let clicked = ui.small_button(icons::TRASH).on_hover_text("Remove this module").clicked();
                            let id = ui.make_persistent_id(("remove-module", i));
                            let ask = |_: &()| {
                                let name = [module.code.trim(), module.name.trim()].into_iter().filter(|t| !t.is_empty()).collect::<Vec<_>>().join(" ");
                                (!module.is_blank()).then(|| widgets::Ask {
                                    title: "Remove this module?".into(),
                                    body: match name.as_str() {
                                        "" => format!("Module {} is removed, with what was typed in for it.", i + 1),
                                        name => format!("{name} is removed, with its marks and notes of what was failed."),
                                    },
                                    button: "Remove".into(),
                                })
                            };
                            if widgets::confirmed(ui, id, clicked.then_some(()), ask).is_some() {
                                remove = Some(i);
                            }
                        });
                    });
                    reveal |= module_fields(ui, module, ask_discussed, marks);
                });
            });
        }
        if let Some(i) = remove {
            m.failed.remove(i);
        }
    });
    reveal
}

/// How many modules: adds or removes them at the end. Taking one away never
/// loses what was typed in; for that there is each module's own bin.
fn how_many(ui: &mut Ui, failed: &mut Vec<FailedModule>) {
    ui.horizontal(|ui| {
        field_label(ui, "How many");
        let can_remove = failed.last().is_some_and(FailedModule::is_blank);
        let minus = ui
            .add_enabled(can_remove, egui::Button::new("−").small())
            .on_disabled_hover_text("To remove a module with details, use its bin");
        if minus.clicked() {
            failed.pop();
        }
        ui.label(theme::semibold(failed.len().to_string()));
        if ui.small_button(icons::PLUS).on_hover_text("One more module").clicked() {
            failed.push(FailedModule::default());
        }
    });
}

/// Returns true when a hidden mark is clicked.
fn module_fields(ui: &mut Ui, module: &mut FailedModule, ask_discussed: bool, marks: bool) -> bool {
    text_row(ui, "Code", &mut module.code, "Module code");
    text_row(ui, "Name", &mut module.name, "Module name");
    ui.horizontal(|ui| {
        field_label(ui, "Failed");
        let options = FailedComponent::ALL.map(|c| (c, c.label()));
        theme::segmented(ui, &mut module.component, &options);
    });
    // Marks of a component that is no longer failed aren't kept.
    if !module.component.has_cw() {
        module.cw_mark.clear();
    }
    if !module.component.has_ex() {
        module.ex_mark.clear();
    }
    let mut reveal = false;
    if module.component.has_cw() {
        reveal |= mark_row(ui, "CW mark", &mut module.cw_mark, marks);
    }
    if module.component.has_ex() {
        reveal |= mark_row(ui, "EX mark", &mut module.ex_mark, marks);
    }
    if ask_discussed {
        ui.checkbox(&mut module.discussed, "Discussed with them whilst my advisee");
    }
    reveal
}

fn text_row(ui: &mut Ui, label: &str, text: &mut String, hint: &str) {
    ui.horizontal(|ui| {
        field_label(ui, label);
        // A single line scrolls inside its field, however long the text.
        ui.add(egui::TextEdit::singleline(text).hint_text(hint).desired_width(ui.available_width()));
    });
}

fn mark_row(ui: &mut Ui, label: &str, mark: &mut String, marks: bool) -> bool {
    ui.horizontal(|ui| {
        field_label(ui, label);
        fyp::mark_of_width(ui, marks, mark, MARK_WIDTH)
    })
    .inner
}

fn field_label(ui: &mut Ui, text: &str) {
    let width = ui.label(text).rect.width();
    ui.add_space((LABEL_WIDTH - width).max(0.0));
}

fn wrapped(ui: &mut Ui, text: RichText) {
    ui.add(egui::Label::new(text).wrap_mode(egui::TextWrapMode::Wrap));
}
