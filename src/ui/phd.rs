//! PhD section of the student detail panel: project details, the dates from
//! the university system with their reminders, the viva and meetings.

use chrono::{Months, NaiveDate};
use egui::{RichText, Ui};
use uuid::Uuid;

use crate::model::{
    FeeStatus, Funder, Milestone, PhdRecord, PhdStep, ResearchDegree, ResearchStatus, Settings,
    StudyMode, VisaType, VivaOutcome,
};
use crate::ui::{icons, theme, widgets};

/// Usual length of a full-time project.
const FULL_TIME_MONTHS: u32 = 36;

/// Returns the UID of a meeting (or the viva) to save an invite for.
pub fn show(ui: &mut Ui, p: &mut PhdRecord, settings: &Settings, student_id: Uuid) -> Option<String> {
    let mut invite = None;
    let today = widgets::today();
    details(ui, p);

    ui.add_space(10.0);
    theme::card_title(ui, "Project dates");
    widgets::form_grid(ui, "phd-dates", |ui| {
        ui.label("Project start");
        widgets::opt_date(ui, "start", &mut p.start);
        ui.end_row();
        ui.label("Project end");
        ui.horizontal(|ui| {
            widgets::opt_date(ui, "end", &mut p.end);
            if let (None, Some(start), StudyMode::FullTime) = (p.end, p.start, p.mode)
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
    });

    ui.add_space(10.0);
    theme::card_title(ui, "Assessments & deadlines");
    ui.label(
        RichText::new("Type the deadlines from the university system. Reminders are worked out from them.")
            .small()
            .color(theme::muted(ui)),
    );
    let steps = p.steps();
    let mut remove_report = None;
    egui::Grid::new("phd-steps")
        .num_columns(3)
        .striped(true)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            for h in ["Item", "Deadline", "Completed"] {
                ui.label(RichText::new(h).small().color(theme::muted(ui)));
            }
            ui.end_row();
            for step in steps.iter().filter(|s| s.key != "amendments") {
                if step.key == "examiners" {
                    if ui.small_button(format!("{} Annual report", icons::PLUS)).clicked() {
                        p.annual_reports.push(next_annual_report(p));
                    }
                    ui.end_row();
                }
                if step.key == "submission" {
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
    if let Some(i) = remove_report {
        p.annual_reports.remove(i);
    }

    ui.add_space(10.0);
    theme::card_title(ui, "Viva & award");
    widgets::form_grid(ui, "phd-viva", |ui| {
        ui.label("Viva date / time");
        ui.horizontal(|ui| {
            widgets::opt_date(ui, "viva", &mut p.viva);
            widgets::opt_time(ui, "viva-time", &mut p.viva_time);
            if widgets::invite_button(ui, p.viva.is_some()) {
                invite = Some(format!("phd-viva-{student_id}"));
            }
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

    ui.add_space(10.0);
    theme::card_title(ui, "Supervision meetings");
    if let Some(uid) = widgets::meetings_editor(ui, "phd-meetings", &mut p.meetings, None, settings) {
        invite = Some(uid);
    }
    invite
}

fn details(ui: &mut Ui, p: &mut PhdRecord) {
    widgets::form_grid(ui, "phd-grid", |ui| {
        ui.label("Title");
        widgets::text_field(ui, &mut p.title, "Research project title");
        ui.end_row();

        ui.label("Type");
        combo(ui, "degree", &mut p.degree, &ResearchDegree::ALL, |d| d.label());
        ui.end_row();

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

        ui.label("Chair of supervisors");
        widgets::text_field(ui, &mut p.chair, "Name");
        ui.end_row();

        ui.label("Supervisors");
        widgets::text_field(ui, &mut p.supervisors, "Other members of the team");
        ui.end_row();

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
    });
}

fn combo<T: Copy + PartialEq>(ui: &mut Ui, salt: &str, value: &mut T, all: &[T], label: impl Fn(T) -> &'static str) {
    egui::ComboBox::from_id_salt(salt)
        .selected_text(label(*value))
        .width(200.0)
        .show_ui(ui, |ui| {
            for &option in all {
                ui.selectable_value(value, option, label(option));
            }
        });
}

/// The step's name, coloured by state, with its reminder date underneath.
fn step_label(ui: &mut Ui, step: &PhdStep, today: NaiveDate) {
    let m = &step.milestone;
    let reminder = m.reminder(step.lead_months);
    let color = match m.deadline {
        _ if m.done => theme::fg(ui, theme::GREEN),
        Some(due) if due < today => theme::fg(ui, theme::RED),
        Some(_) if reminder.is_some_and(|r| r <= today) => theme::fg(ui, theme::AMBER),
        _ => ui.visuals().text_color(),
    };
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.label(RichText::new(&step.label).color(color));
        if let Some(r) = reminder.filter(|_| !m.done) {
            ui.label(
                RichText::new(format!("{} reminder {}", icons::BELL, r.format("%d %b %Y")))
                    .small()
                    .color(theme::muted(ui)),
            );
        }
    });
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

/// A new annual report a year after the last one (or with no date).
fn next_annual_report(p: &PhdRecord) -> Milestone {
    let last = p.annual_reports.iter().rev().find_map(|r| r.deadline);
    Milestone {
        deadline: last.and_then(|d| d.checked_add_months(Months::new(12))),
        ..Default::default()
    }
}
