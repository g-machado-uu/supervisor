//! Small reusable input widgets.

use egui::AsIdSalt;

use chrono::{Datelike, Local, NaiveDate, NaiveTime, Timelike};
use egui::Ui;
use egui_extras::DatePickerButton;

use crate::model::{ReplyStatus, TriState};

pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

pub fn fmt_date(date: Option<NaiveDate>) -> String {
    date.map_or_else(|| "—".into(), |d| d.format("%a %d %b %Y").to_string())
}

pub fn fmt_short(date: NaiveDate) -> String {
    date.format("%d %b").to_string()
}

fn to_jiff(d: NaiveDate) -> jiff::civil::Date {
    jiff::civil::date(d.year() as i16, d.month() as i8, d.day() as i8)
}

fn from_jiff(d: jiff::civil::Date) -> NaiveDate {
    NaiveDate::from_ymd_opt(d.year().into(), d.month() as u32, d.day() as u32).unwrap_or_else(today)
}

/// An optional date: a "Set date" button when empty, otherwise a picker plus clear button.
pub fn opt_date(ui: &mut Ui, salt: impl AsIdSalt, value: &mut Option<NaiveDate>) {
    ui.push_id(salt, |ui| {
        ui.horizontal(|ui| match value {
            Some(date) => {
                let mut j = to_jiff(*date);
                ui.add(
                    DatePickerButton::new(&mut j)
                        .id_salt("date")
                        .format("%a %d %b %Y")
                        .calendar_week(false),
                );
                *date = from_jiff(j);
                if ui.small_button("×").on_hover_text("Clear date").clicked() {
                    *value = None;
                }
            }
            None => {
                if ui.button("📅 Set date").clicked() {
                    *value = Some(today());
                }
            }
        });
    });
}

/// An optional time of day, edited as hour/minute drag values.
pub fn opt_time(ui: &mut Ui, salt: impl AsIdSalt, value: &mut Option<NaiveTime>) {
    ui.push_id(salt, |ui| {
        ui.horizontal(|ui| match value {
            Some(time) => {
                let mut h = time.hour();
                let mut m = time.minute();
                ui.add(egui::DragValue::new(&mut h).range(0..=23).custom_formatter(|v, _| format!("{v:02}")));
                ui.label(":");
                ui.add(
                    egui::DragValue::new(&mut m)
                        .range(0..=59)
                        .speed(0.3)
                        .custom_formatter(|v, _| format!("{v:02}")),
                );
                *time = NaiveTime::from_hms_opt(h, m, 0).unwrap_or(*time);
                if ui.small_button("×").on_hover_text("Clear time").clicked() {
                    *value = None;
                }
            }
            None => {
                if ui.button("🕑 Set time").clicked() {
                    *value = NaiveTime::from_hms_opt(10, 0, 0);
                }
            }
        });
    });
}

pub fn tri_state(ui: &mut Ui, value: &mut TriState) {
    ui.horizontal(|ui| {
        ui.selectable_value(value, TriState::Unknown, "Unknown");
        ui.selectable_value(value, TriState::Yes, "Yes");
        ui.selectable_value(value, TriState::No, "No");
    });
}

pub fn reply_status(ui: &mut Ui, value: &mut ReplyStatus) {
    ui.horizontal(|ui| {
        for status in [ReplyStatus::Awaiting, ReplyStatus::Replied, ReplyStatus::NoReply] {
            ui.selectable_value(value, status, status.label());
        }
    });
}

pub fn notes(ui: &mut Ui, text: &mut String, hint: &str) {
    ui.add(
        egui::TextEdit::multiline(text)
            .hint_text(hint)
            .desired_rows(3)
            .desired_width(f32::INFINITY),
    );
}

pub fn text_field(ui: &mut Ui, text: &mut String, hint: &str) {
    ui.add(
        egui::TextEdit::singleline(text)
            .hint_text(hint)
            .desired_width(f32::INFINITY),
    );
}

/// Two-column form grid with consistent spacing.
pub fn form_grid(ui: &mut Ui, id: impl AsIdSalt, add: impl FnOnce(&mut Ui)) {
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([12.0, 8.0])
        .min_col_width(130.0)
        .show(ui, add);
}

/// Editable list of meetings with date, time, held flag and notes.
/// `first_label` names the first entry (e.g. "Semester meeting"); later
/// entries become follow-ons.
pub fn meetings_editor(
    ui: &mut Ui,
    salt: impl AsIdSalt,
    meetings: &mut Vec<crate::model::Meeting>,
    first_label: Option<&str>,
) {
    ui.push_id(salt, |ui| {
        let mut remove = None;
        for (i, m) in meetings.iter_mut().enumerate() {
            let title = match (first_label, i) {
                (Some(label), 0) => label.to_string(),
                (Some(_), n) => format!("Follow-on {n}"),
                (None, n) => format!("Meeting {}", n + 1),
            };
            ui.push_id(m.id, |ui| {
                egui::Frame::group(ui.style())
                    .corner_radius(egui::CornerRadius::same(8))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(title).strong());
                            if m.held {
                                crate::ui::theme::chip(ui, "held", crate::ui::theme::GREEN);
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("🗑").on_hover_text("Remove meeting").clicked() {
                                    remove = Some(i);
                                }
                            });
                        });
                        ui.horizontal_wrapped(|ui| {
                            opt_date(ui, "date", &mut m.date);
                            opt_time(ui, "time", &mut m.time);
                            ui.checkbox(&mut m.held, "Held");
                        });
                        notes(ui, &mut m.notes, "Meeting notes…");
                    });
            });
        }
        if let Some(i) = remove {
            meetings.remove(i);
        }
        let add_label = match (first_label, meetings.is_empty()) {
            (Some(label), true) => format!("➕ Add {}", label.to_lowercase()),
            (Some(_), false) => "➕ Add follow-on meeting".into(),
            (None, _) => "➕ Add meeting".into(),
        };
        if ui.button(add_label).clicked() {
            meetings.push(crate::model::Meeting::default());
        }
    });
}
