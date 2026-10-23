//! Small reusable input widgets.

use egui::AsIdSalt;

use chrono::{Datelike, Local, NaiveDate, NaiveTime, Timelike};
use egui::{RichText, Ui};
use egui_extras::DatePickerButton;

use crate::model::{Meeting, Repeat, ReplyStatus, Settings, TriState};
use crate::ui::theme;

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

/// Button for saving an invite file; needs a date to be set.
pub fn invite_button(ui: &mut Ui, has_date: bool) -> bool {
    ui.add_enabled(has_date, egui::Button::new("📨 Invite file…").small())
        .on_hover_text("Save an .ics with the student as attendee, to upload to Outlook and send")
        .on_disabled_hover_text("Set a date first")
        .clicked()
}

/// "Teaching week 5 · Mon 26 Oct" style label for a date.
pub fn week_label(settings: &Settings, date: NaiveDate) -> String {
    match settings.teaching_week(date) {
        Some((_, week)) => format!("W{week} · {}", date.format("%a %d %b")),
        None => date.format("%a %d %b").to_string(),
    }
}

/// Editable list of meetings: date, time, Teams or room, repetition and
/// notes. `first_label` names the first entry (e.g. "Semester meeting");
/// later entries become follow-ons. Returns the UID of a meeting whose
/// invite button was clicked.
pub fn meetings_editor(
    ui: &mut Ui,
    salt: impl AsIdSalt,
    meetings: &mut Vec<Meeting>,
    first_label: Option<&str>,
    settings: &Settings,
) -> Option<String> {
    let mut invite = None;
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
                            ui.label(RichText::new(title).strong());
                            if let Some(r) = &m.repeat {
                                theme::chip(ui, every_label(r.every_weeks), theme::ACCENT);
                            } else if m.held {
                                theme::chip(ui, "held", theme::GREEN);
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .small_button("🗑")
                                        .on_hover_text("Remove meeting")
                                        .clicked()
                                    {
                                        remove = Some(i);
                                    }
                                    if invite_button(ui, m.date.is_some()) {
                                        invite = Some(m.uid());
                                    }
                                },
                            );
                        });
                        ui.horizontal_wrapped(|ui| {
                            opt_date(ui, "date", &mut m.date);
                            opt_time(ui, "time", &mut m.time);
                            if m.repeat.is_none() {
                                ui.checkbox(&mut m.held, "Held");
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut m.teams, "💻 Teams");
                            if !m.teams {
                                ui.label("📍");
                                text_field(ui, &mut m.location, "Room / location");
                            }
                        });
                        repeat_editor(ui, m, settings);
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
            meetings.push(Meeting::default());
        }
    });
    invite
}

fn every_label(weeks: u8) -> String {
    match weeks {
        0 | 1 => "🔁 weekly".into(),
        2 => "🔁 fortnightly".into(),
        n => format!("🔁 every {n} weeks"),
    }
}

/// The "Repeats" controls and, for a repeating meeting, its list of weeks.
fn repeat_editor(ui: &mut Ui, m: &mut Meeting, settings: &Settings) {
    let teaching_end = m
        .date
        .and_then(|d| settings.teaching_end(settings.semester_of(d)));
    ui.horizontal(|ui| {
        let mut on = m.repeat.is_some();
        if ui.checkbox(&mut on, "🔁 Repeats").changed() {
            m.repeat = on.then(Repeat::default);
        }
        let Some(r) = &mut m.repeat else { return };
        egui::ComboBox::from_id_salt("every")
            .selected_text(match r.every_weeks {
                0 | 1 => "every week".to_string(),
                n => format!("every {n} weeks"),
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut r.every_weeks, 1, "every week");
                for n in 2..=4u8 {
                    ui.selectable_value(&mut r.every_weeks, n, format!("every {n} weeks"));
                }
            });
    });
    let date = m.date;
    if let Some(r) = &mut m.repeat {
        ui.horizontal(|ui| {
            ui.label("until");
            match r.until {
                None => {
                    let text = match teaching_end {
                        Some(end) => format!("end of teaching ({})", fmt_short(end)),
                        None => "end of teaching".into(),
                    };
                    ui.label(RichText::new(text).strong());
                    if ui
                        .small_button("Change…")
                        .on_hover_text("Stop on another date")
                        .clicked()
                    {
                        r.until = Some(teaching_end.or(date).unwrap_or_else(today));
                    }
                }
                Some(_) => {
                    opt_date(ui, "until", &mut r.until);
                    ui.label(
                        RichText::new("(× = end of teaching)")
                            .small()
                            .color(theme::GREY),
                    );
                }
            }
        });
    }
    if m.repeat.is_none() {
        return;
    }
    if m.date.is_none() {
        ui.label(
            RichText::new("Set the first date to see the weeks.")
                .small()
                .color(theme::GREY),
        );
        return;
    }
    if m.repeat_end(settings).is_none() {
        ui.colored_label(
            theme::AMBER,
            "⚠ Set the semester dates in Settings (or an end date) – only the first week is planned.",
        );
    }

    let weeks = m.occurrences(settings);
    let cancelled = weeks.iter().filter(|o| o.cancelled).count();
    let held = weeks.iter().filter(|o| o.held).count();
    let mut header = format!("Weeks ({}", weeks.len());
    if held > 0 {
        header.push_str(&format!(", {held} held"));
    }
    if cancelled > 0 {
        header.push_str(&format!(", {cancelled} cancelled"));
    }
    header.push(')');
    let mut changed = Vec::new();
    egui::CollapsingHeader::new(header)
        .id_salt("weeks")
        .show(ui, |ui| {
            egui::Grid::new("weeks-grid")
                .num_columns(5)
                .striped(true)
                .spacing([10.0, 4.0])
                .show(ui, |ui| {
                    for h in ["Planned", "Held", "Cancel", "Moved to", "Notes"] {
                        ui.label(RichText::new(h).small().color(theme::GREY));
                    }
                    ui.end_row();
                    for occ in weeks {
                        let mut edited = occ.clone();
                        let mut planned = RichText::new(week_label(settings, occ.planned));
                        if occ.cancelled {
                            planned = planned.strikethrough().color(theme::GREY);
                        }
                        ui.label(planned);
                        ui.push_id((occ.planned, "held"), |ui| {
                            ui.checkbox(&mut edited.held, "")
                        });
                        ui.push_id((occ.planned, "cancel"), |ui| {
                            ui.checkbox(&mut edited.cancelled, "")
                        });
                        ui.push_id((occ.planned, "move"), |ui| {
                            ui.horizontal(|ui| {
                                if edited.moved_to.is_none() && edited.time.is_none() {
                                    if ui.small_button("Move…").clicked() {
                                        edited.moved_to = Some(occ.planned);
                                        edited.time = m.time;
                                    }
                                } else {
                                    opt_date(ui, "date", &mut edited.moved_to);
                                    opt_time(ui, "time", &mut edited.time);
                                }
                            });
                        });
                        ui.push_id((occ.planned, "notes"), |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut edited.notes)
                                    .hint_text("Notes")
                                    .desired_width(120.0),
                            );
                        });
                        ui.end_row();
                        if edited != occ {
                            changed.push(edited);
                        }
                    }
                });
        });
    for occ in changed {
        m.set_occurrence(occ);
    }
}
