//! Calendar: meetings, visits and deadlines as a list grouped by week or as
//! a month grid, with selection for exporting an .ics.

use std::collections::{BTreeMap, HashSet};

use chrono::{Datelike, Duration, Months, NaiveDate};
use egui::{RichText, Ui};
use uuid::Uuid;

use crate::model::{Database, Event, EventKind, Semester};
use crate::ui::{theme, widgets};

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

fn kind_style(kind: EventKind) -> (&'static str, egui::Color32) {
    match kind {
        EventKind::FypMeeting => ("👥", theme::ACCENT),
        EventKind::Visit => ("🏢", theme::GREEN),
        EventKind::AdviseeMeeting => ("💬", theme::AMBER),
        EventKind::Deadline => ("📌", theme::RED),
    }
}

fn shown(db: &Database, st: &CalendarState, e: &Event, today: NaiveDate) -> bool {
    let kind = match e.kind {
        EventKind::FypMeeting | EventKind::AdviseeMeeting => st.meetings,
        EventKind::Visit => st.visits,
        EventKind::Deadline => st.deadlines,
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

/// `with_guests` is the stored "include students as guests" choice.
pub fn show(
    ui: &mut Ui,
    db: &Database,
    st: &mut CalendarState,
    with_guests: &mut bool,
) -> CalAction {
    let mut action = CalAction::None;
    let today = widgets::today();
    let all = db.events();
    // Forget selections of items that no longer exist (or were cancelled).
    let exists: HashSet<&str> = all
        .iter()
        .filter(|e| !e.cancelled)
        .map(|e| e.uid.as_str())
        .collect();
    st.selected.retain(|uid| exists.contains(uid.as_str()));
    let events: Vec<&Event> = all.iter().filter(|e| shown(db, st, e, today)).collect();

    ui.horizontal(|ui| {
        ui.heading("Calendar");
        ui.add_space(12.0);
        ui.selectable_value(&mut st.layout, Layout::List, "☰ List");
        ui.selectable_value(&mut st.layout, Layout::Month, "📅 Month");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let n = st.selected.len();
            let export = egui::Button::new(
                RichText::new(format!("📅 Export {n} selected (.ics)…")).strong(),
            )
            .fill(theme::ACCENT.gamma_multiply(0.85));
            if ui
                .add_enabled(n > 0, export)
                .on_disabled_hover_text("Tick the items to export")
                .clicked()
            {
                action = CalAction::Export(st.selected.clone());
            }
        });
    });

    ui.horizontal_wrapped(|ui| {
        match st.layout {
            Layout::List => {
                ui.selectable_value(&mut st.range, Range::Upcoming, "Upcoming");
                let has_sem2 = db.settings.sem2_start.is_some();
                ui.selectable_value(&mut st.range, Range::Semester(Semester::S1), "Semester 1");
                ui.add_enabled_ui(has_sem2, |ui| {
                    ui.selectable_value(&mut st.range, Range::Semester(Semester::S2), "Semester 2")
                        .on_disabled_hover_text("Set the semester 2 start date in Settings");
                });
                ui.selectable_value(&mut st.range, Range::All, "Everything");
            }
            Layout::Month => {
                if ui.button("◀").on_hover_text("Previous month").clicked() {
                    st.month = st.month - Months::new(1);
                }
                ui.label(
                    RichText::new(st.month.format("%B %Y").to_string())
                        .strong()
                        .size(16.0),
                );
                if ui.button("▶").on_hover_text("Next month").clicked() {
                    st.month = st.month + Months::new(1);
                }
                if ui.button("Today").clicked() {
                    st.month = first_of_month(today);
                }
            }
        }
        ui.separator();
        ui.toggle_value(&mut st.meetings, "👥 Meetings");
        ui.toggle_value(&mut st.visits, "🏢 Visits");
        ui.toggle_value(&mut st.deadlines, "📌 Deadlines");
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
    });
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(&mut st.as_series, "🔁 Repeating meetings as one series")
            .on_hover_text("Selecting any week exports the whole series, ending when it ends, with cancelled and moved weeks");
        ui.checkbox(with_guests, "📨 Students as guests")
            .on_hover_text("You as organiser and the students (and line managers for visits) as guests, so Outlook can send the invitations");
    });
    ui.add_space(6.0);

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
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("Nothing here.").color(theme::GREY).size(16.0));
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
                let mut heading =
                    format!("{} – {}", monday.format("%d %b"), sunday.format("%d %b %Y"));
                if let Some((sem, week)) = teaching_week_of(db, monday) {
                    let sem = if sem == Semester::S2 { 2 } else { 1 };
                    heading = format!("Semester {sem}, week {week} · {heading}");
                }
                if (monday..=sunday).contains(&today) {
                    heading.push_str(" · this week");
                }
                let selectable: Vec<&str> = items
                    .iter()
                    .filter(|e| !e.cancelled)
                    .map(|e| e.uid.as_str())
                    .collect();
                ui.add_space(4.0);
                ui.separator();
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
                    ui.label(RichText::new(heading).strong().color(theme::ACCENT));
                });
                for e in items {
                    if row(ui, e, st) {
                        *action = CalAction::Open(e.student_id);
                    }
                }
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
            let (icon, color) = kind_style(e.kind);
            let when = match e.time {
                Some(t) => format!("{} {}", e.date.format("%a %d %b"), t.format("%H:%M")),
                None => format!("{} all day", e.date.format("%a %d %b")),
            };
            let faded = e.cancelled || e.done;
            ui.label(RichText::new(when).monospace().color(if faded {
                theme::GREY
            } else {
                color
            }));
            let mut title = RichText::new(format!("{icon} {}", e.title));
            if e.cancelled {
                title = title.strikethrough().color(theme::GREY);
            } else if e.done {
                title = title.color(theme::GREY);
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
                ui.label(RichText::new("🔁").small().color(theme::GREY))
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
                ui.label(RichText::new("💻 Teams").small().color(theme::GREY));
            } else if !e.location.is_empty() {
                ui.label(
                    RichText::new(format!("📍 {}", e.location))
                        .small()
                        .color(theme::GREY),
                );
            }
        });
    });
    open
}

/// Short text for a month cell: the student, or the coursework for deadlines.
fn cell_text(db: &Database, e: &Event) -> String {
    let what = match e.kind {
        EventKind::Deadline => e.title.split(" – ").next().unwrap_or(&e.title).to_string(),
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
    let (icon, _) = kind_style(e.kind);
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
            .color(theme::GREY),
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
                        egui::Label::new(RichText::new(day).strong().color(theme::GREY)),
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
                        egui::Label::new(RichText::new(week).small().strong().color(theme::ACCENT)),
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
        egui::Stroke::new(1.5, theme::ACCENT)
    } else {
        ui.visuals().widgets.noninteractive.bg_stroke
    };
    let margin = 4.0;
    egui::Frame::NONE
        .fill(fill)
        .stroke(stroke)
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::same(margin as i8))
        .show(ui, |ui| {
            let inner = size - egui::vec2(2.0 * margin + 2.0, 2.0 * margin + 2.0);
            ui.set_min_size(inner);
            ui.set_max_width(inner.x);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                let number = RichText::new(date.day().to_string()).strong();
                ui.label(match (date == today, in_month) {
                    (true, _) => number.color(theme::ACCENT),
                    (false, true) => number,
                    (false, false) => number.color(theme::GREY),
                });
                for e in items {
                    ui.push_id(&e.uid, |ui| {
                        let (_, color) = kind_style(e.kind);
                        let mut text = RichText::new(cell_text(db, e)).small();
                        text = if e.cancelled {
                            text.strikethrough().color(theme::GREY)
                        } else if e.done {
                            text.color(theme::GREY)
                        } else {
                            text.color(color)
                        };
                        let selected = st.selected.contains(&e.uid);
                        let mut hover = e.title.clone();
                        if e.cancelled {
                            hover.push_str("\nCancelled");
                        } else if e.moved() {
                            hover.push_str("\nMoved");
                        }
                        if e.online {
                            hover.push_str("\n💻 Teams");
                        } else if !e.location.is_empty() {
                            hover.push_str(&format!("\n📍 {}", e.location));
                        }
                        let resp = ui
                            .add_enabled(
                                !e.cancelled,
                                egui::Button::selectable(selected, text).truncate(),
                            )
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
