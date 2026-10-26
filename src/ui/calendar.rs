//! Calendar: meetings, visits and deadlines as a list grouped by week or as
//! a month grid, with selection for exporting an .ics.

use std::collections::{BTreeMap, HashSet};

use chrono::{Datelike, Duration, Months, NaiveDate};
use egui::{RichText, Ui};
use uuid::Uuid;

use crate::model::{Database, Event, EventKind, Semester};
use crate::ui::{icons, theme, widgets};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Range {
    Upcoming,
    Semester(Semester),
    All,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    List,
    Month,
}

pub struct CalendarState {
    pub layout: Layout,
    /// List range.
    pub range: Range,
    /// First day of the month shown in the month grid.
    pub month: NaiveDate,
    pub meetings: bool,
    pub visits: bool,
    pub deadlines: bool,
    pub reminders: bool,
    /// Event UIDs; kept while switching months or layouts.
    pub selected: HashSet<String>,
    /// Export a repeating meeting as one series rather than single weeks.
    pub as_series: bool,
}

impl Default for CalendarState {
    fn default() -> Self {
        Self {
            layout: Layout::List,
            range: Range::Upcoming,
            month: first_of_month(widgets::today()),
            meetings: true,
            visits: true,
            deadlines: true,
            reminders: true,
            selected: HashSet::new(),
            as_series: true,
        }
    }
}

pub enum CalAction {
    None,
    Open(Uuid),
    /// Export these event UIDs.
    Export(HashSet<String>),
}

fn first_of_month(date: NaiveDate) -> NaiveDate {
    date.with_day(1).unwrap_or(date)
}

fn monday_of(date: NaiveDate) -> NaiveDate {
    date - Duration::days(i64::from(date.weekday().num_days_from_monday()))
}

fn shown(db: &Database, st: &CalendarState, e: &Event, today: NaiveDate) -> bool {
    let kind = match e.kind {
        EventKind::FypMeeting | EventKind::AdviseeMeeting | EventKind::PhdMeeting => st.meetings,
        EventKind::Visit => st.visits,
        EventKind::Deadline => st.deadlines,
        EventKind::Reminder => st.reminders,
    };
    let in_range = match st.layout {
        Layout::Month => {
            let start = monday_of(st.month);
            let end = monday_of(st.month + Months::new(1) - Duration::days(1)) + Duration::days(6);
            (start..=end).contains(&e.date)
        }
        Layout::List => match st.range {
            Range::Upcoming => e.date >= today,
            Range::Semester(sem) => db.settings.semester_of(e.date) == sem,
            Range::All => true,
        },
    };
    kind && in_range
}

/// `all` is `db.events()`, built once per change. `with_guests` is the
/// stored "include students as guests" choice.
pub fn show(
    ui: &mut Ui,
    db: &Database,
    all: &[Event],
    st: &mut CalendarState,
    with_guests: &mut bool,
) -> CalAction {
    let mut action = CalAction::None;
    let today = widgets::today();
    // Forget selections of items that no longer exist (or were cancelled).
    let exists: HashSet<&str> = all
        .iter()
        .filter(|e| !e.cancelled)
        .map(|e| e.uid.as_str())
        .collect();
    st.selected.retain(|uid| exists.contains(uid.as_str()));
    let events: Vec<&Event> = all.iter().filter(|e| shown(db, st, e, today)).collect();

    let n = st.selected.len();
    let subtitle = match n {
        0 => "Meetings, visits and deadlines · tick items to export them".to_string(),
        1 => "1 item selected for export".to_string(),
        n => format!("{n} items selected for export"),
    };
    theme::page_header(ui, "Calendar", &subtitle, |ui| {
        if ui
            .add_enabled(n > 0, theme::primary_button(format!("{}  Export {n} selected", icons::EXPORT)))
            .on_hover_text("Save the selected items as an .ics file")
            .on_disabled_hover_text("Tick the items to export")
            .clicked()
        {
            action = CalAction::Export(st.selected.clone());
        }
    });

    ui.horizontal_wrapped(|ui| {
        theme::segmented(
            ui,
            &mut st.layout,
            &[
                (Layout::List, &format!("{} List", icons::LIST_BULLETS)),
                (Layout::Month, &format!("{} Month", icons::CALENDAR_DOTS)),
            ],
        );
        ui.add_space(6.0);
        match st.layout {
            Layout::List => {
                let has_sem2 = db.settings.sem2_start.is_some();
                let mut range = st.range;
                theme::segmented(
                    ui,
                    &mut range,
                    &[
                        (Range::Upcoming, "Upcoming"),
                        (Range::Semester(Semester::S1), "Semester 1"),
                        (Range::Semester(Semester::S2), "Semester 2"),
                        (Range::All, "Everything"),
                    ],
                );
                if range != Range::Semester(Semester::S2) || has_sem2 {
                    st.range = range;
                }
            }
            Layout::Month => {
                if ui.button(icons::CARET_LEFT).on_hover_text("Previous month").clicked() {
                    st.month = st.month - Months::new(1);
                }
                ui.label(theme::serif(st.month.format("%B %Y").to_string(), 20.0));
                if ui.button(icons::CARET_RIGHT).on_hover_text("Next month").clicked() {
                    st.month = st.month + Months::new(1);
                }
                if ui.button("Today").clicked() {
                    st.month = first_of_month(today);
                }
            }
        }
    });
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        theme::toggle_pill(ui, &mut st.meetings, icons::USERS, "Meetings", theme::ACCENT);
        theme::toggle_pill(ui, &mut st.visits, icons::BUILDINGS, "Visits", theme::AMBER);
        theme::toggle_pill(ui, &mut st.deadlines, icons::PUSH_PIN, "Deadlines", theme::RED);
        theme::toggle_pill(ui, &mut st.reminders, icons::BELL, "Reminders", theme::GREY);
        ui.separator();
        if ui.button("Select all shown").clicked() {
            st.selected.extend(
                events
                    .iter()
                    .filter(|e| !e.cancelled)
                    .map(|e| e.uid.clone()),
            );
        }
        if ui.button("Clear").clicked() {
            st.selected.clear();
        }
        ui.separator();
        ui.checkbox(&mut st.as_series, format!("{} Repeats as one series", icons::REPEAT))
            .on_hover_text("Selecting any week exports the whole series, ending when it ends, with cancelled and moved weeks");
        ui.checkbox(with_guests, format!("{} Students as guests", icons::PAPER_PLANE_TILT))
            .on_hover_text("You as organiser and the students (and line managers for visits) as guests, so Outlook can send the invitations");
    });
    ui.add_space(10.0);

    match st.layout {
        Layout::List => list(ui, db, st, &events, today, &mut action),
        Layout::Month => month(ui, db, st, &events, today, &mut action),
    }
    action
}

fn list(
    ui: &mut Ui,
    db: &Database,
    st: &mut CalendarState,
    events: &[&Event],
    today: NaiveDate,
    action: &mut CalAction,
) {
    if events.is_empty() {
        theme::card(ui, "", |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(30.0);
                ui.label(RichText::new(icons::CALENDAR_CHECK).size(40.0).color(theme::muted(ui)));
                ui.label(RichText::new("Nothing here.").color(theme::muted(ui)).size(16.0));
                ui.add_space(30.0);
            });
        });
        return;
    }

    // Group by the Monday of each week.
    let mut weeks: BTreeMap<NaiveDate, Vec<&Event>> = BTreeMap::new();
    for e in events {
        weeks.entry(monday_of(e.date)).or_default().push(e);
    }

    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            for (monday, items) in weeks {
                let sunday = monday + Duration::days(6);
                let dates = format!("{} – {}", monday.format("%d %b"), sunday.format("%d %b %Y"));
                let heading = match teaching_week_of(db, monday) {
                    Some((sem, week)) => {
                        let sem = if sem == Semester::S2 { 2 } else { 1 };
                        format!("Semester {sem}, week {week}")
                    }
                    None => "Outside teaching".to_string(),
                };
                let this_week = (monday..=sunday).contains(&today);
                let selectable: Vec<&str> = items
                    .iter()
                    .filter(|e| !e.cancelled)
                    .map(|e| e.uid.as_str())
                    .collect();
                theme::card_frame(ui).inner_margin(egui::Margin::symmetric(16, 12)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let mut on = !selectable.is_empty()
                        && selectable.iter().all(|uid| st.selected.contains(*uid));
                    ui.push_id(monday, |ui| {
                        if ui
                            .add_enabled(
                                !selectable.is_empty(),
                                egui::Checkbox::without_text(&mut on),
                            )
                            .changed()
                        {
                            for uid in &selectable {
                                if on {
                                    st.selected.insert(uid.to_string());
                                } else {
                                    st.selected.remove(*uid);
                                }
                            }
                        }
                    });
                    ui.label(theme::semibold(heading).size(15.0));
                    ui.label(RichText::new(dates).color(theme::muted(ui)));
                    if this_week {
                        theme::chip(ui, "this week", theme::ACCENT);
                    }
                });
                ui.add_space(2.0);
                for e in items {
                    if row(ui, e, st) {
                        *action = CalAction::Open(e.student_id);
                    }
                }
                });
                ui.add_space(10.0);
            }
        });
}

/// Teaching week of a Monday-to-Friday week, if any day of it is taught.
fn teaching_week_of(db: &Database, monday: NaiveDate) -> Option<(Semester, i64)> {
    (0..5).find_map(|d| db.settings.teaching_week(monday + Duration::days(d)))
}

/// One item; returns true when its title was clicked.
fn row(ui: &mut Ui, e: &Event, st: &mut CalendarState) -> bool {
    let mut open = false;
    ui.push_id(&e.uid, |ui| {
        ui.horizontal_wrapped(|ui| {
            let mut on = st.selected.contains(&e.uid);
            if ui
                .add_enabled(!e.cancelled, egui::Checkbox::without_text(&mut on))
                .changed()
            {
                if on {
                    st.selected.insert(e.uid.clone());
                } else {
                    st.selected.remove(&e.uid);
                }
            }
            let (icon, color) = theme::kind_style(e.kind);
            let faded = e.cancelled || e.done;
            let color = if faded { theme::muted(ui) } else { theme::fg(ui, color) };
            let when = match e.time {
                Some(t) => format!("{} {}", e.date.format("%a %d"), t.format("%H:%M")),
                None => format!("{} all day", e.date.format("%a %d")),
            };
            ui.add_sized([110.0, 20.0], egui::Label::new(RichText::new(when).color(color)));
            ui.label(RichText::new(icon).size(16.0).color(color));
            let mut title = RichText::new(&e.title);
            if e.cancelled {
                title = title.strikethrough().color(theme::muted(ui));
            } else if e.done {
                title = title.color(theme::muted(ui));
            }
            if ui
                .add(egui::Label::new(title).sense(egui::Sense::click()))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("Open student")
                .clicked()
            {
                open = true;
            }
            if e.series.is_some() {
                ui.label(RichText::new(icons::REPEAT).color(theme::muted(ui)))
                    .on_hover_text("Repeating meeting");
            }
            if e.cancelled {
                theme::chip(ui, "cancelled", theme::GREY);
            } else if e.moved() {
                theme::chip(ui, "moved", theme::AMBER);
            }
            if e.done && !e.cancelled {
                let text = if e.kind == EventKind::Deadline {
                    "submitted"
                } else {
                    "done"
                };
                theme::chip(ui, text, theme::GREEN);
            }
            if e.online {
                ui.label(RichText::new(format!("{} Teams", icons::DESKTOP)).small().color(theme::muted(ui)));
            } else if !e.location.is_empty() {
                ui.label(
                    RichText::new(format!("{} {}", icons::MAP_PIN, e.location))
                        .small()
                        .color(theme::muted(ui)),
                );
            }
        });
    });
    open
}

/// Short text for a month cell: the student, or the coursework for deadlines.
fn cell_text(db: &Database, e: &Event) -> String {
    let what = match e.kind {
        EventKind::Deadline | EventKind::Reminder => e.title.split(" – ").next().unwrap_or(&e.title).to_string(),
        _ => db
            .student(e.student_id)
            .map(|s| {
                if s.name.is_empty() {
                    s.display_name()
                } else {
                    s.name.clone()
                }
            })
            .unwrap_or_default(),
    };
    let (icon, _) = theme::kind_style(e.kind);
    match e.time {
        Some(t) => format!("{} {icon} {what}", t.format("%H:%M")),
        None => format!("{icon} {what}"),
    }
}

fn month(
    ui: &mut Ui,
    db: &Database,
    st: &mut CalendarState,
    events: &[&Event],
    today: NaiveDate,
    action: &mut CalAction,
) {
    let mut by_day: BTreeMap<NaiveDate, Vec<&Event>> = BTreeMap::new();
    for e in events {
        by_day.entry(e.date).or_default().push(e);
    }
    let first = monday_of(st.month);
    let last = st.month + Months::new(1) - Duration::days(1);
    let gap = 4.0;
    let week_col = 40.0;
    // Leave room for the scroll bar and the cell borders.
    let col = ((ui.available_width() - week_col - gap * 8.0 - 16.0) / 7.0).max(90.0);

    ui.label(
        RichText::new("Click an item to select it, double-click to open the student.")
            .small()
            .color(theme::muted(ui)),
    );
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
            ui.horizontal(|ui| {
                ui.add_sized([week_col, 18.0], egui::Label::new(""));
                for day in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"] {
                    ui.add_sized(
                        [col, 18.0],
                        egui::Label::new(
                            theme::semibold(day.to_uppercase())
                                .size(11.0)
                                .extra_letter_spacing(0.6)
                                .color(theme::muted(ui)),
                        ),
                    );
                }
            });
            let mut monday = first;
            while monday <= last {
                let busiest = (0..7)
                    .map(|d| {
                        by_day
                            .get(&(monday + Duration::days(d)))
                            .map_or(0, Vec::len)
                    })
                    .max()
                    .unwrap_or(0);
                let height = (24.0 + 22.0 * busiest as f32).max(84.0);
                ui.horizontal_top(|ui| {
                    let week = teaching_week_of(db, monday)
                        .map_or_else(String::new, |(_, w)| format!("W{w}"));
                    ui.add_sized(
                        [week_col, 22.0],
                        egui::Label::new(theme::semibold(week).small().color(theme::fg(ui, theme::ACCENT))),
                    )
                    .on_hover_text("Teaching week");
                    for d in 0..7 {
                        let date = monday + Duration::days(d);
                        let items = by_day.get(&date).map_or(&[][..], Vec::as_slice);
                        if let Some(student) =
                            day_cell(ui, db, st, date, items, today, egui::vec2(col, height))
                        {
                            *action = CalAction::Open(student);
                        }
                    }
                });
                monday += Duration::days(7);
            }
        });
}

/// One day of the month grid. Returns the student of a double-clicked item.
fn day_cell(
    ui: &mut Ui,
    db: &Database,
    st: &mut CalendarState,
    date: NaiveDate,
    items: &[&Event],
    today: NaiveDate,
    size: egui::Vec2,
) -> Option<Uuid> {
    let mut open = None;
    let in_month = date.month() == st.month.month();
    let fill = if in_month {
        ui.visuals().window_fill
    } else {
        ui.visuals().faint_bg_color
    };
    let stroke = if date == today {
        egui::Stroke::new(1.5, theme::fg(ui, theme::ACCENT))
    } else {
        ui.visuals().widgets.noninteractive.bg_stroke
    };
    let margin = 5.0;
    egui::Frame::NONE
        .fill(fill)
        .stroke(stroke)
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::same(margin as i8))
        .show(ui, |ui| {
            let inner = size - egui::vec2(2.0 * margin + 2.0, 2.0 * margin + 2.0);
            ui.set_min_size(inner);
            ui.set_max_width(inner.x);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                let number = theme::semibold(date.day().to_string());
                if date == today {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 11.0, theme::ACCENT);
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        date.day().to_string(),
                        egui::FontId::proportional(12.0),
                        egui::Color32::WHITE,
                    );
                } else {
                    ui.add_sized(
                        [22.0, 22.0],
                        egui::Label::new(if in_month { number } else { number.color(theme::muted(ui)) }),
                    );
                }
                for e in items {
                    ui.push_id(&e.uid, |ui| {
                        let (_, color) = theme::kind_style(e.kind);
                        let mut text = RichText::new(cell_text(db, e)).small();
                        text = if e.cancelled {
                            text.strikethrough().color(theme::muted(ui))
                        } else if e.done {
                            text.color(theme::muted(ui))
                        } else {
                            text.color(theme::fg(ui, color))
                        };
                        let selected = st.selected.contains(&e.uid);
                        let faded = e.cancelled || e.done;
                        let mut hover = e.title.clone();
                        if e.cancelled {
                            hover.push_str("\nCancelled");
                        } else if e.moved() {
                            hover.push_str("\nMoved");
                        }
                        if e.online {
                            hover.push_str(&format!("\n{} Teams", icons::DESKTOP));
                        } else if !e.location.is_empty() {
                            hover.push_str(&format!("\n{} {}", icons::MAP_PIN, e.location));
                        }
                        let pill = egui::Button::new(text)
                            .truncate()
                            .min_size(egui::vec2(ui.available_width(), 0.0))
                            .fill(if faded {
                                egui::Color32::TRANSPARENT
                            } else if selected {
                                theme::fg(ui, color).gamma_multiply(0.3)
                            } else {
                                theme::soft(ui, color)
                            })
                            .stroke(if selected {
                                egui::Stroke::new(1.5, theme::fg(ui, color))
                            } else {
                                egui::Stroke::NONE
                            })
                            .corner_radius(egui::CornerRadius::same(6));
                        let resp = ui
                            .scope(|ui| {
                                ui.spacing_mut().button_padding = egui::vec2(6.0, 2.0);
                                ui.add_enabled(!e.cancelled, pill)
                            })
                            .inner
                            .on_hover_text(hover);
                        if resp.clicked() {
                            if selected {
                                st.selected.remove(&e.uid);
                            } else {
                                st.selected.insert(e.uid.clone());
                            }
                        }
                        if resp.double_clicked() {
                            open = Some(e.student_id);
                        }
                    });
                }
            });
        });
    open
}
