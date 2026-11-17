//! Starting a new academic year: what becomes of last year's students, and
//! this year's dates.

use egui::{Id, RichText, Ui};

use crate::model::{Database, FixedDates, NewYear, Settings, YEAR_ON, YearGroup};
use crate::ui::{icons, theme, widgets};

pub struct NewYearDialog {
    pub plan: NewYear,
    /// Deleting was confirmed.
    pub sure: bool,
}

impl NewYearDialog {
    pub fn new(db: &Database) -> Self {
        Self { plan: db.new_year_plan(), sure: false }
    }
}

pub enum NewYearAction {
    None,
    Start,
    Cancel,
}

pub fn show(ui: &mut Ui, dialog: &mut NewYearDialog, db: &Database) -> NewYearAction {
    let mut action = NewYearAction::None;
    let ctx = ui.ctx().clone();
    let max_height = (ctx.content_rect().height() - 220.0).max(240.0);
    let resp = egui::Modal::new(Id::unique("new-year")).frame(theme::modal_frame(ui)).show(&ctx, |ui| {
        ui.set_width(600.0);
        ui.horizontal(|ui| {
            theme::icon_badge(ui, icons::GRADUATION_CAP, theme::ACCENT, 40.0);
            ui.label(theme::serif("Start a new academic year", 22.0));
        });
        ui.add_space(6.0);
        egui::ScrollArea::vertical().max_height(max_height).show(ui, |ui| {
            body(ui, dialog, db);
        });
        ui.add_space(10.0);
        let plan = &dialog.plan;
        let (leaving, _) = db.leaving_students(&plan.leaving);
        let earlier = if plan.delete_archived { db.students.iter().filter(|s| s.archived).count() } else { 0 };
        let deleting = if plan.delete { leaving.len() } else { 0 } + earlier;
        let year = plan.year.trim();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let ready = !year.is_empty() && (deleting == 0 || dialog.sure);
            let start = if deleting > 0 {
                let students = if deleting == 1 { "student" } else { "students" };
                theme::danger_button(format!("Delete {deleting} {students} and start {year}"))
            } else {
                theme::primary_button(format!("Start {year}"))
            };
            let hover = if year.is_empty() { "Name the new year first" } else { "Tick the box above to confirm deleting" };
            if ui.add_enabled(ready, start).on_disabled_hover_text(hover).clicked() {
                action = NewYearAction::Start;
            }
            if ui.add(egui::Button::new("Cancel").min_size(egui::vec2(0.0, 34.0))).clicked() {
                action = NewYearAction::Cancel;
            }
        });
    });
    if resp.should_close() && matches!(action, NewYearAction::None) {
        action = NewYearAction::Cancel;
    }
    action
}

fn note(ui: &mut Ui, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).small().color(theme::muted(ui))).wrap_mode(egui::TextWrapMode::Wrap));
}

fn heading(ui: &mut Ui, text: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(text.to_uppercase()).small().strong().color(theme::muted(ui)));
}

fn body(ui: &mut Ui, dialog: &mut NewYearDialog, db: &Database) {
    let current = &db.settings.academic_year;
    let plan = &mut dialog.plan;
    widgets::form_grid(ui, "new-year-name", |ui| {
        ui.label("New academic year");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut plan.year).desired_width(120.0));
            ui.label(RichText::new(format!("now {current}")).color(theme::muted(ui)));
        });
        ui.end_row();
    });

    heading(ui, "Last year's students");
    note(
        ui,
        "Students change each year. Tick the ones whose year is over. A student with a role you keep (for example a \
         research student who is also an advisee) stays as they are.",
    );
    ui.add_space(4.0);
    for group in YearGroup::ALL {
        let count = db.count_in(group);
        let mut on = plan.leaving.contains(&group);
        let text = format!("{} ({count})", group.label());
        let resp = ui.checkbox(&mut on, text);
        let resp = if group.ends_with_the_year() { resp } else { resp.on_hover_text("Research students usually carry on into the next year") };
        if resp.changed() {
            if on {
                plan.leaving.insert(group);
            } else {
                plan.leaving.remove(&group);
            }
        }
    }
    let (leaving, staying) = db.leaving_students(&plan.leaving);
    if staying > 0 {
        let text = if staying == 1 {
            "1 student also has a role you keep, and stays.".to_string()
        } else {
            format!("{staying} students also have a role you keep, and stay.")
        };
        note(ui, &text);
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.selectable_value(&mut plan.delete, false, format!("{} Archive them", icons::ARCHIVE));
        ui.selectable_value(&mut plan.delete, true, RichText::new("Delete them").color(theme::fg(ui, theme::RED)));
    });
    note(
        ui,
        if plan.delete {
            "Deleted students and everything recorded about them are gone for good."
        } else {
            "Archived students are hidden from the lists, the dashboard and the calendar. Show archived, above each \
             list, brings them back into view."
        },
    );
    let archived = db.students.iter().filter(|s| s.archived).count();
    if archived > 0 {
        let text = if archived == 1 {
            "Also delete the 1 student archived before".to_string()
        } else {
            format!("Also delete the {archived} students archived before")
        };
        ui.checkbox(&mut plan.delete_archived, text);
    }
    let deleting = if plan.delete { leaving.len() } else { 0 } + if plan.delete_archived { archived } else { 0 };
    if deleting > 0 {
        ui.add_space(4.0);
        ui.checkbox(&mut dialog.sure, "I understand that deleted students can't be brought back");
        note(
            ui,
            "The data as it is now is kept next to your data file as its .bak, until you next click Save or unlock \
             it again.",
        );
    }

    heading(ui, "This year's dates");
    note(
        ui,
        "Suggested 52 weeks on from this year's, so each keeps its weekday. Coursework set by teaching week follows \
         these dates.",
    );
    ui.add_space(4.0);
    widgets::form_grid(ui, "new-year-dates", |ui| {
        for (label, start, end) in [
            ("Semester 1", &mut plan.sem1_start, &mut plan.sem1_teaching_end),
            ("Semester 2", &mut plan.sem2_start, &mut plan.sem2_teaching_end),
        ] {
            ui.label(format!("{label} – week 1 starts"));
            widgets::opt_date(ui, (label, "new-start"), start);
            ui.end_row();
            ui.label(format!("{label} – teaching ends"));
            ui.horizontal(|ui| {
                if end.is_some() {
                    widgets::opt_date(ui, (label, "new-end"), end);
                } else if let Some(start) = *start {
                    let default = Settings::default_teaching_end(start);
                    ui.label(format!("{} (Friday of week 12)", widgets::fmt_date(Some(default))));
                    if ui.small_button("Change…").clicked() {
                        *end = Some(default);
                    }
                } else {
                    ui.label(RichText::new("set week 1 first").color(theme::muted(ui)));
                }
            });
            ui.end_row();
        }
    });

    let fixed = db.fixed_dates();
    if !fixed.is_empty() {
        heading(ui, "Fixed dates");
        note(ui, "These don't follow the semester dates. Check them in Settings › Modules & coursework and Marking deadlines afterwards.");
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.selectable_value(&mut plan.fixed_dates, FixedDates::MoveOn, "Move on 52 weeks");
            ui.selectable_value(&mut plan.fixed_dates, FixedDates::Clear, "Clear them");
            ui.selectable_value(&mut plan.fixed_dates, FixedDates::Keep, "Keep them");
        });
        egui::Grid::new("new-year-fixed").num_columns(2).spacing([16.0, 4.0]).striped(true).show(ui, |ui| {
            for f in &fixed {
                ui.label(&f.label);
                let after = match plan.fixed_dates {
                    FixedDates::MoveOn => format!("{} → {}", widgets::fmt_date(Some(f.date)), widgets::fmt_date(Some(f.date + YEAR_ON))),
                    FixedDates::Clear => format!("{} → not set", widgets::fmt_date(Some(f.date))),
                    FixedDates::Keep => widgets::fmt_date(Some(f.date)),
                };
                ui.label(RichText::new(after).color(theme::muted(ui)));
                ui.end_row();
            }
        });
    }
}
