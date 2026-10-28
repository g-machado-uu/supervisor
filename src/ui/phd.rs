//! Research section of the student detail panel: project details, the dates
//! from the university system (or added by hand) with their reminders, the
//! viva and meetings. External students on a Brazilian programme get its
//! grant, stages and defence instead (see `brazil`).

use chrono::{Months, NaiveDate};
use egui::{RichText, Ui};
use uuid::Uuid;

use crate::model::{
    CustomStep, DefenceOutcome, FeeStatus, Funder, Lead, LeadUnit, Milestone, PhdRecord, PhdStep,
    ResearchDegree, ResearchStatus, Settings, StudyMode, VisaType, VivaOutcome,
};
use crate::ui::widgets::Share;
use crate::ui::{brazil, icons, theme, widgets};

/// Usual length of a full-time project.
const FULL_TIME_MONTHS: u32 = 36;

/// Returns the meeting (or the viva) to send, if asked.
pub fn show(ui: &mut Ui, p: &mut PhdRecord, settings: &Settings, student_id: Uuid) -> Option<Share> {
    let mut invite = None;
    let today = widgets::today();
    details(ui, p);
    let br = p.brazilian_programme();

    if br {
        ui.add_space(10.0);
        theme::section(ui, "Grant", |ui| {
            brazil::grant_editor(ui, &mut p.brazil.grant, p.brazil.level, p.start);
        });
    }

    ui.add_space(10.0);
    theme::section(ui, "Project dates", |ui| {
        widgets::form_grid(ui, "phd-dates", |ui| {
            ui.label(if br { "Project start (enrolment)" } else { "Project start" });
            widgets::opt_date(ui, "start", &mut p.start);
            ui.end_row();
            ui.label("Project end");
            ui.horizontal(|ui| {
                widgets::opt_date(ui, "end", &mut p.end);
                if let (None, Some(start), StudyMode::FullTime, false) = (p.end, p.start, p.mode, br)
                    && ui
                        .small_button(format!("{FULL_TIME_MONTHS} months from start"))
                        .clicked()
                {
                    p.end = start.checked_add_months(Months::new(FULL_TIME_MONTHS));
                }
            });
            ui.end_row();
            if let Some(year) = p.year_of_study(today) {
                ui.label("Year of study");
                ui.label(format!("Year {year}"));
                ui.end_row();
            }
            if br {
                brazil::extension_row(ui, p);
            }
        });
    });

    ui.add_space(10.0);
    theme::section(ui, "Assessments & deadlines", |ui| {
        if br {
            brazil::stages_header(ui, p, &settings.br_rule(p.brazil.level));
        } else {
            let mut standard = !p.skip_standard;
            if ui
                .checkbox(&mut standard, "Standard assessments of this university")
                .on_hover_text("Initial, confirmation and final assessments, annual reports, examiners and submission")
                .changed()
            {
                p.skip_standard = !standard;
            }
            let hint = if p.skip_standard {
                "Add the deadlines from the student's university and choose when to be reminded."
            } else {
                "Type the deadlines from the university system. Reminders are worked out from them."
            };
            ui.label(RichText::new(hint).small().color(theme::muted(ui)));
        }
        let steps = p.steps();
        // The annual reports are added from the row before this step.
        let add_report_before = if br { "submission" } else { "examiners" };
        let mut remove_report = None;
        if !p.skip_standard {
            egui::Grid::new("phd-steps")
                .num_columns(3)
                .striped(true)
                .spacing([10.0, 6.0])
                .show(ui, |ui| {
                    for h in ["Item", "Deadline", "Completed"] {
                        ui.label(RichText::new(h).small().color(theme::muted(ui)));
                    }
                    ui.end_row();
                    for step in steps
                        .iter()
                        .filter(|s| s.key != "amendments" && !s.key.starts_with("custom-"))
                    {
                        if step.key == add_report_before {
                            if ui.small_button(format!("{} Annual report", icons::PLUS)).clicked() {
                                p.annual_reports.push(next_annual_report(p));
                            }
                            ui.end_row();
                        }
                        if step.key == "submission" && !br {
                            ui.label("Earliest submission");
                            widgets::opt_date(ui, "earliest", &mut p.earliest_submission);
                            ui.end_row();
                        }
                        let annual = step.key.strip_prefix("annual").and_then(|n| n.parse::<usize>().ok());
                        ui.horizontal(|ui| {
                            step_label(ui, step, today);
                            if let Some(n) = annual
                                && ui
                                    .push_id(&step.key, |ui| ui.small_button(icons::TRASH))
                                    .inner
                                    .on_hover_text("Remove this annual report")
                                    .clicked()
                            {
                                remove_report = Some(n - 1);
                            }
                        });
                        let mut edited = step.milestone.clone();
                        milestone_cells(ui, &step.key, &mut edited, today);
                        if edited != step.milestone
                            && let Some(m) = p.milestone_mut(&step.key)
                        {
                            *m = edited;
                        }
                        ui.end_row();
                    }
                });
        }
        if let Some(i) = remove_report {
            p.annual_reports.remove(i);
        }
        custom_steps(ui, &mut p.custom_steps, today);
    });

    ui.add_space(10.0);
    let steps = p.steps();
    let viva_uid = format!("phd-viva-{student_id}");
    let invite_viva = if br {
        theme::section(ui, "Defence & award", |ui| defence(ui, p, &steps, today, &viva_uid))
    } else {
        theme::section(ui, "Viva & award", |ui| viva(ui, p, &steps, today, &viva_uid))
    };
    if let Some(share) = invite_viva.flatten() {
        invite = Some(share);
    }

    ui.add_space(10.0);
    let meetings = theme::section(ui, "Supervision meetings", |ui| {
        widgets::meetings_editor(ui, "phd-meetings", &mut p.meetings, None, settings)
    });
    if let Some(share) = meetings.flatten() {
        invite = Some(share);
    }
    invite
}

/// The viva, its outcome, amendments and award. Returns the viva (`uid`)
/// if it is to be sent.
fn viva(ui: &mut Ui, p: &mut PhdRecord, steps: &[PhdStep], today: NaiveDate, uid: &str) -> Option<Share> {
    let mut invite = None;
    widgets::form_grid(ui, "phd-viva", |ui| {
        ui.label("Viva date / time");
        ui.horizontal(|ui| {
            widgets::opt_date(ui, "viva", &mut p.viva);
            widgets::opt_time(ui, "viva-time", &mut p.viva_time);
            invite = widgets::share_buttons(ui, p.viva.is_some(), || uid.to_string());
        });
        ui.end_row();
        ui.label("Viva location");
        widgets::text_field(ui, &mut p.viva_location, "Room / location");
        ui.end_row();
        ui.label("Outcome");
        combo(ui, "outcome", &mut p.outcome, &VivaOutcome::ALL, |o| o.label());
        ui.end_row();
        let amendments = steps.iter().find(|s| s.key == "amendments");
        let corrections = matches!(
            p.outcome,
            VivaOutcome::MinorCorrections | VivaOutcome::MajorCorrections | VivaOutcome::Resubmit
        );
        if let Some(step) = amendments.filter(|s| corrections || s.milestone.deadline.is_some()) {
            ui.horizontal(|ui| step_label(ui, step, today));
            ui.horizontal(|ui| {
                let mut edited = step.milestone.clone();
                milestone_cells(ui, &step.key, &mut edited, today);
                if edited != step.milestone {
                    p.amendments = edited;
                }
            });
            ui.end_row();
        }
        ui.label("Awarded on");
        widgets::opt_date(ui, "awarded", &mut p.awarded_on);
        ui.end_row();
    });
    invite
}

/// The defence of a Brazilian programme: date, committee, result, final
/// version and award. Returns the defence (`uid`) if it is to be sent.
fn defence(ui: &mut Ui, p: &mut PhdRecord, steps: &[PhdStep], today: NaiveDate, uid: &str) -> Option<Share> {
    let mut invite = None;
    widgets::form_grid(ui, "phd-defence", |ui| {
        ui.label("Defence date / time");
        ui.horizontal(|ui| {
            widgets::opt_date(ui, "viva", &mut p.viva);
            widgets::opt_time(ui, "viva-time", &mut p.viva_time);
            invite = widgets::share_buttons(ui, p.viva.is_some(), || uid.to_string());
        });
        ui.end_row();
        ui.label("Location");
        widgets::text_field(ui, &mut p.viva_location, "Room, auditorium or link");
        ui.end_row();
        ui.label("Committee");
        widgets::text_field(ui, &mut p.brazil.committee, "Members of the examining committee (banca)");
        ui.end_row();
        ui.label("Outcome");
        combo(ui, "br-outcome", &mut p.brazil.outcome, &DefenceOutcome::ALL, |o| o.label());
        ui.end_row();
        let final_version = steps.iter().find(|s| s.key == "amendments");
        if let Some(step) = final_version.filter(|s| p.brazil.outcome.corrections() || s.milestone.deadline.is_some()) {
            ui.horizontal(|ui| step_label(ui, step, today));
            ui.horizontal(|ui| {
                let mut edited = step.milestone.clone();
                milestone_cells(ui, &step.key, &mut edited, today);
                if edited != step.milestone {
                    p.amendments = edited;
                }
            });
            ui.end_row();
        }
        ui.label("Awarded on");
        widgets::opt_date(ui, "awarded", &mut p.awarded_on);
        ui.end_row();
    });
    invite
}

fn details(ui: &mut Ui, p: &mut PhdRecord) {
    widgets::form_grid(ui, "phd-grid", |ui| {
        ui.label("Title");
        widgets::text_field(ui, &mut p.title, "Research project title");
        ui.end_row();

        ui.label("Type");
        ui.horizontal(|ui| {
            if p.brazilian_programme() {
                brazil::level_combo(ui, p);
            } else {
                combo(ui, "degree", &mut p.degree, &ResearchDegree::ALL, |d| d.label());
            }
            ui.checkbox(&mut p.external, "External")
                .on_hover_text("Registered at another university; you co-supervise");
        });
        ui.end_row();

        if p.external {
            ui.label("University");
            widgets::text_field(ui, &mut p.university, "Where the student is registered");
            ui.end_row();

            ui.label("Brazil");
            let mut brazilian = p.brazilian;
            if ui
                .checkbox(&mut brazilian, "On a Brazilian programme")
                .on_hover_text("A mestrado or doutorado in Brazil: its levels, grants, stages and defence")
                .changed()
            {
                p.set_brazilian(brazilian);
            }
            ui.end_row();
        }
        let br = p.brazilian_programme();

        if br {
            ui.label("Programme");
            widgets::text_field(ui, &mut p.brazil.programme, "Postgraduate programme (PPG)");
            ui.end_row();
        }

        ui.label("Status");
        combo(ui, "status", &mut p.status, &ResearchStatus::ALL, |s| s.label());
        ui.end_row();

        ui.label("Project mode");
        ui.horizontal(|ui| {
            for mode in [StudyMode::FullTime, StudyMode::PartTime] {
                ui.selectable_value(&mut p.mode, mode, mode.label());
            }
        });
        ui.end_row();

        ui.label(if br { "Main supervisor" } else { "Chair of supervisors" });
        widgets::text_field(ui, &mut p.chair, "Name");
        ui.end_row();

        ui.label(if br { "Co-supervisors" } else { "Supervisors" });
        widgets::text_field(ui, &mut p.supervisors, "Other members of the team");
        ui.end_row();

        // A Brazilian programme has its grant card instead.
        if br {
            return;
        }
        ui.label("Funder");
        ui.horizontal(|ui| {
            combo(ui, "funder", &mut p.funder, &Funder::ALL, |f| f.label());
            if p.funder == Funder::Other {
                widgets::text_field(ui, &mut p.funder_other, "Funder");
            }
        });
        ui.end_row();

        ui.label("Funding source");
        widgets::text_field(ui, &mut p.funding_source, "e.g. studentship, scholarship");
        ui.end_row();

        // Fees and visas are this university's business, not for external students.
        if p.external {
            return;
        }
        ui.label("Fee status");
        ui.horizontal(|ui| {
            for fee in [FeeStatus::National, FeeStatus::Overseas] {
                ui.selectable_value(&mut p.fee_status, fee, fee.label());
            }
        });
        ui.end_row();

        ui.label("Visa type");
        ui.horizontal(|ui| {
            combo(ui, "visa", &mut p.visa, &VisaType::ALL, |v| v.label());
            if p.visa == VisaType::Other {
                widgets::text_field(ui, &mut p.visa_other, "Visa type");
            }
        });
        ui.end_row();

        if p.visa != VisaType::NotRequired {
            ui.label("Visa start / end");
            ui.horizontal(|ui| {
                widgets::opt_date(ui, "visa-start", &mut p.visa_start);
                ui.label("–");
                widgets::opt_date(ui, "visa-end", &mut p.visa_end);
            });
            ui.end_row();

            ui.label("CAS number");
            widgets::text_field(ui, &mut p.cas_number, "Confirmation of Acceptance for Studies");
            ui.end_row();
        }

    });
}

pub fn combo<T: Copy + PartialEq>(ui: &mut Ui, salt: &str, value: &mut T, all: &[T], label: impl Fn(T) -> &'static str) {
    egui::ComboBox::from_id_salt(salt)
        .selected_text(label(*value))
        .width(200.0)
        .show_ui(ui, |ui| {
            for &option in all {
                ui.selectable_value(value, option, label(option));
            }
        });
}

/// Green when done, red when overdue, amber once the reminder has passed.
fn state_color(ui: &Ui, m: &Milestone, lead: Lead, today: NaiveDate) -> egui::Color32 {
    match m.deadline {
        _ if m.done => theme::fg(ui, theme::GREEN),
        Some(due) if due < today => theme::fg(ui, theme::RED),
        Some(_) if m.reminder(lead).is_some_and(|r| r <= today) => theme::fg(ui, theme::AMBER),
        _ => ui.visuals().text_color(),
    }
}

fn reminder_note(ui: &mut Ui, m: &Milestone, lead: Lead) {
    if let Some(r) = m.reminder(lead).filter(|_| !m.done) {
        ui.label(
            RichText::new(format!("{} reminder {}", icons::BELL, r.format("%d %b %Y")))
                .small()
                .color(theme::muted(ui)),
        );
    }
}

/// The step's name, coloured by state, with its reminder date underneath.
fn step_label(ui: &mut Ui, step: &PhdStep, today: NaiveDate) {
    let color = state_color(ui, &step.milestone, step.lead, today);
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.label(RichText::new(&step.label).color(color));
        reminder_note(ui, &step.milestone, step.lead);
    });
}

/// Deadlines added by hand, each with its own reminder.
fn custom_steps(ui: &mut Ui, steps: &mut Vec<CustomStep>, today: NaiveDate) {
    let mut remove = None;
    if !steps.is_empty() {
        ui.add_space(6.0);
        egui::Grid::new("phd-custom-steps")
            .num_columns(4)
            .striped(true)
            .spacing([10.0, 6.0])
            .show(ui, |ui| {
                for h in ["Item", "Deadline", "Completed", "Remind me"] {
                    ui.label(RichText::new(h).small().color(theme::muted(ui)));
                }
                ui.end_row();
                for (i, c) in steps.iter_mut().enumerate() {
                    let color = state_color(ui, &c.milestone, c.remind, today);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.horizontal(|ui| {
                            ui.add_sized(
                                [180.0, 22.0],
                                egui::TextEdit::singleline(&mut c.title)
                                    .id_salt((c.id, "title"))
                                    .hint_text("e.g. Qualifying exam")
                                    .text_color(color),
                            );
                            if ui
                                .push_id((c.id, "remove"), |ui| ui.small_button(icons::TRASH))
                                .inner
                                .on_hover_text("Remove this deadline")
                                .clicked()
                            {
                                remove = Some(i);
                            }
                        });
                        reminder_note(ui, &c.milestone, c.remind);
                    });
                    milestone_cells(ui, &format!("custom-{}", c.id), &mut c.milestone, today);
                    ui.push_id((c.id, "lead"), |ui| {
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(&mut c.remind.amount).range(0..=365));
                            egui::ComboBox::from_id_salt("unit")
                                .selected_text(c.remind.unit.label())
                                .width(80.0)
                                .show_ui(ui, |ui| {
                                    for unit in LeadUnit::ALL {
                                        ui.selectable_value(&mut c.remind.unit, unit, unit.label());
                                    }
                                });
                            ui.label("before");
                        })
                        .response
                        .on_hover_text("0 = no reminder");
                    });
                    ui.end_row();
                }
            });
    }
    if let Some(i) = remove {
        steps.remove(i);
    }
    if ui.small_button(format!("{} Add deadline", icons::PLUS)).clicked() {
        steps.push(CustomStep::default());
    }
}

/// Deadline and completed cells of a milestone row.
fn milestone_cells(ui: &mut Ui, key: &str, m: &mut Milestone, today: NaiveDate) {
    widgets::opt_date(ui, (key, "due"), &mut m.deadline);
    ui.horizontal(|ui| {
        let done = ui.push_id((key, "done"), |ui| ui.checkbox(&mut m.done, "")).inner;
        if done.changed() && m.done && m.done_on.is_none() {
            m.done_on = Some(today);
        }
        if m.done {
            widgets::opt_date(ui, (key, "done-on"), &mut m.done_on);
        }
    });
}

/// A new annual report a year after the last one (or with no date). On a
/// Brazilian programme the first is a year after enrolment.
fn next_annual_report(p: &PhdRecord) -> Milestone {
    let last = p
        .annual_reports
        .iter()
        .rev()
        .find_map(|r| r.deadline)
        .or(p.start.filter(|_| p.brazilian_programme()));
    Milestone {
        deadline: last.and_then(|d| d.checked_add_months(Months::new(12))),
        ..Default::default()
    }
}
