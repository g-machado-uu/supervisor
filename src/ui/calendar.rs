//! Calendar: meetings, visits and deadlines as a list grouped by week or as
//! a month grid, with selection for exporting an .ics. Items can be edited,
//! deleted and (in the month grid) dragged to another day, and another
//! calendar's .ics can be imported.

use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{Datelike, Duration, Local, Months, NaiveDate, NaiveDateTime, NaiveTime};
use egui::{RichText, Ui};
use uuid::Uuid;

use crate::model::{
    Database, Event, EventDetails, EventKind, ImportTarget, ImportedEvent, Known, MeetingChange, MeetingRole, Platform,
    Semester,
};
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
    /// Imported events without a student.
    pub other: bool,
    /// Event UIDs; kept while switching months or layouts.
    pub selected: HashSet<String>,
    /// Export a repeating meeting as one series rather than single weeks.
    pub as_series: bool,
    /// Height of each week card in the list (by its first day) when last
    /// drawn; cards out of view take up that space without being drawn.
    week_heights: HashMap<NaiveDate, f32>,
    /// What those heights were measured for.
    heights_for: Option<ListKey>,
    /// The "Postpone meeting" dialog, while open.
    postpone: Option<Postpone>,
    /// The edit dialog, while open.
    editing: Option<Editing>,
    /// Items waiting for "Delete" to be confirmed.
    deleting: Option<Vec<Event>>,
    /// The import dialog, while open.
    pub import: Option<ImportDialog>,
    /// Privacy mode (switched on the dashboard): no student is named.
    pub private: bool,
}

impl CalendarState {
    /// What an item is called: without the student in privacy mode.
    fn title<'e>(&self, e: &'e Event) -> &'e str {
        if self.private { e.anonymous_title() } else { &e.title }
    }

    /// Where it is; a company can name the student, so nothing in privacy mode.
    fn place<'e>(&self, e: &'e Event) -> &'e str {
        if self.private { "" } else { &e.location }
    }
}

/// A meeting (or one week of a repeating one) being postponed, and to when.
struct Postpone {
    event: Event,
    date: Option<NaiveDate>,
    time: Option<NaiveTime>,
}

impl Postpone {
    /// Suggests a week after it was planned, and not before tomorrow.
    fn new(event: &Event, today: NaiveDate) -> Self {
        Self {
            date: Some((event.date + Duration::days(7)).max(today + Duration::days(1))),
            time: event.time,
            event: event.clone(),
        }
    }
}

/// An item open in the edit dialog.
struct Editing {
    event: Event,
    details: EventDetails,
    /// The date picker wants an `Option`.
    date: Option<NaiveDate>,
}

/// The list's range, the kinds shown, the first day of the week, the width,
/// and the events (a new list after every edit, so its address and length change).
type ListKey = (Range, [bool; 6], u32, usize, usize);

/// What is carried while an item is dragged in the month grid.
struct Dragged(Event);

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
            other: true,
            selected: HashSet::new(),
            as_series: true,
            week_heights: HashMap::new(),
            heights_for: None,
            postpone: None,
            editing: None,
            deleting: None,
            import: None,
            private: false,
        }
    }
}

impl CalendarState {
    fn dialog_open(&self) -> bool {
        self.postpone.is_some() || self.editing.is_some() || self.deleting.is_some() || self.import.is_some()
    }
}

pub enum CalAction {
    None,
    Open(Uuid),
    /// Export these event UIDs.
    Export(HashSet<String>),
    /// Mark a meeting held or not, postpone it, or cancel or restore a week.
    Change(Event, MeetingChange),
    /// Move an item to another day, keeping its time.
    Move(Event, NaiveDate),
    Edit(Event, EventDetails),
    /// Delete these items; with `true`, whole repeating meetings rather
    /// than single weeks.
    Delete(Vec<Event>, bool),
    /// Open the item in Google Calendar.
    Google(Event),
    /// Choose an .ics file to import.
    PickImport,
    Import(Vec<(ImportedEvent, ImportTarget)>),
}

/// A meeting whose time has come, so it can be marked held or postponed.
fn started(e: &Event, now: NaiveDateTime) -> bool {
    !e.cancelled && e.meeting_id().is_some() && e.date.and_time(e.time.unwrap_or(NaiveTime::MIN)) <= now
}

fn first_of_month(date: NaiveDate) -> NaiveDate {
    date.with_day(1).unwrap_or(date)
}

/// Short names of the days of the week, in the order the calendar shows them.
fn day_names(db: &Database) -> [&'static str; 7] {
    if db.settings.week_starts_sunday {
        ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]
    } else {
        ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
    }
}

fn shown(db: &Database, st: &CalendarState, e: &Event, today: NaiveDate) -> bool {
    let kind = match e.kind {
        EventKind::FypMeeting | EventKind::AdviseeMeeting | EventKind::PhdMeeting => st.meetings,
        EventKind::Visit => st.visits,
        EventKind::Deadline => st.deadlines,
        EventKind::Reminder => st.reminders,
        EventKind::Other => st.other,
    };
    let in_range = match st.layout {
        Layout::Month => {
            let start = db.settings.week_start(st.month);
            let last = st.month + Months::new(1) - Duration::days(1);
            let end = db.settings.week_start(last) + Duration::days(6);
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
    let now = Local::now().naive_local();
    let today = now.date();
    // Forget selections of items that no longer exist (or were cancelled).
    let exists: HashSet<&str> = all
        .iter()
        .filter(|e| !e.cancelled)
        .map(|e| e.uid.as_str())
        .collect();
    st.selected.retain(|uid| exists.contains(uid.as_str()));
    let events: Vec<&Event> = all.iter().filter(|e| shown(db, st, e, today)).collect();

    // Delete removes the selected items, once confirmed.
    if !st.dialog_open()
        && !st.selected.is_empty()
        && !ui.ctx().egui_wants_keyboard_input()
        && ui.input(|i| i.key_pressed(egui::Key::Delete))
    {
        let chosen: Vec<Event> = all
            .iter()
            .filter(|e| st.selected.contains(&e.uid) && e.editable() && !e.cancelled)
            .cloned()
            .collect();
        if !chosen.is_empty() {
            st.deleting = Some(chosen);
        }
    }

    // Escape clears the selection.
    if !st.dialog_open()
        && !st.selected.is_empty()
        && !ui.ctx().egui_wants_keyboard_input()
        && ui.input(|i| i.key_pressed(egui::Key::Escape))
    {
        st.selected.clear();
    }

    let n = st.selected.len();
    let subtitle = match n {
        0 => "Tick items to export them · right-click an item to edit or delete it".to_string(),
        1 => "1 item selected – export it, or press Delete to remove it".to_string(),
        n => format!("{n} items selected – export them, or press Delete to remove them"),
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
        if ui
            .add(egui::Button::new(format!("{}  Import…", icons::DOWNLOAD_SIMPLE)).min_size(egui::vec2(0.0, 34.0)))
            .on_hover_text("Bring in events from another calendar (.ics from Outlook or Google Calendar)")
            .clicked()
        {
            action = CalAction::PickImport;
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
        if !db.entries.is_empty() {
            theme::toggle_pill(ui, &mut st.other, icons::CALENDAR_BLANK, "Other", theme::GREY)
                .on_hover_text("Imported events that aren't linked to a student");
        }
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
        Layout::List => {
            let kinds = [st.meetings, st.visits, st.deadlines, st.reminders, st.other, db.settings.week_starts_sunday];
            let key = (st.range, kinds, ui.available_width() as u32, all.as_ptr() as usize, all.len());
            if st.heights_for != Some(key) {
                st.heights_for = Some(key);
                st.week_heights.clear();
            }
            list(ui, db, st, &events, now, &mut action);
        }
        Layout::Month => month(ui, db, st, &events, now, &mut action),
    }
    postpone_dialog(ui, st, &mut action);
    edit_dialog(ui, st, &mut action);
    delete_dialog(ui, st, &mut action);
    import_dialog(ui, db, st, &mut action);
    action
}

/// Right-click menu of an item: mark a past meeting held or postpone it,
/// edit, delete or restore it, send it to Google Calendar, open the student.
fn item_menu(ui: &mut Ui, db: &Database, e: &Event, st: &mut CalendarState, now: NaiveDateTime, action: &mut CalAction) {
    if started(e, now) {
        if e.done {
            if ui.button(format!("{} Mark as not held", icons::X)).clicked() {
                *action = CalAction::Change(e.clone(), MeetingChange::Held(false));
                ui.close();
            }
        } else {
            if ui.button(format!("{} Mark as held", icons::CHECK)).clicked() {
                *action = CalAction::Change(e.clone(), MeetingChange::Held(true));
                ui.close();
            }
            if ui.button(format!("{} Postpone…", icons::CALENDAR_BLANK)).clicked() {
                st.postpone = Some(Postpone::new(e, now.date()));
                ui.close();
            }
        }
        ui.separator();
    }
    if e.cancelled && e.series.is_some() {
        if ui.button(format!("{} Restore this week", icons::REPEAT)).clicked() {
            *action = CalAction::Change(e.clone(), MeetingChange::Cancelled(false));
            ui.close();
        }
    } else if e.editable() {
        if ui.button(format!("{} Edit…", icons::PENCIL_SIMPLE)).clicked() {
            if let Some(details) = db.event_details(e) {
                st.editing = Some(Editing {
                    event: e.clone(),
                    date: Some(details.date),
                    details,
                });
            }
            ui.close();
        }
        if ui.button(format!("{} Delete…", icons::TRASH)).clicked() {
            st.deleting = Some(vec![e.clone()]);
            ui.close();
        }
        if ui
            .button(format!("{} Open in Google Calendar", icons::ARROW_SQUARE_OUT))
            .on_hover_text("Opens the browser with it filled in, and the student as a guest; Save there to send it")
            .clicked()
        {
            *action = CalAction::Google(e.clone());
            ui.close();
        }
    }
    if !e.student_id.is_nil() && ui.button(format!("{} Open student", icons::STUDENT)).clicked() {
        *action = CalAction::Open(e.student_id);
        ui.close();
    }
}

/// Asks for the new day and time of the meeting being postponed.
fn postpone_dialog(ui: &mut Ui, st: &mut CalendarState, action: &mut CalAction) {
    let private = st.private;
    let Some(p) = &mut st.postpone else { return };
    let mut postpone = false;
    let mut cancel = false;
    let resp = egui::Modal::new(egui::Id::unique("postpone-meeting")).frame(theme::modal_frame(ui)).show(ui.ctx(), |ui| {
        ui.set_width(440.0);
        ui.horizontal(|ui| {
            theme::icon_badge(ui, icons::CALENDAR_BLANK, theme::ACCENT, 40.0);
            ui.label(theme::serif("Postpone meeting", 22.0));
        });
        ui.add_space(4.0);
        ui.label(if private { p.event.anonymous_title() } else { &p.event.title });
        ui.label(RichText::new(format!("Was planned for {}", when_text(&p.event))).small().color(theme::muted(ui)));
        ui.add_space(8.0);
        widgets::form_grid(ui, "postpone-grid", |ui| {
            ui.label("New date");
            widgets::opt_date(ui, "date", &mut p.date);
            ui.end_row();
            ui.label("Time");
            widgets::opt_time(ui, "time", &mut p.time);
            ui.end_row();
        });
        ui.add_space(12.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add_enabled(p.date.is_some(), theme::primary_button("Postpone")).clicked() {
                postpone = true;
            }
            if ui.add(egui::Button::new("Cancel").min_size(egui::vec2(0.0, 34.0))).clicked() {
                cancel = true;
            }
        });
    });
    if postpone && let Some(date) = p.date {
        *action = CalAction::Change(p.event.clone(), MeetingChange::Postpone(date, p.time));
        st.postpone = None;
    } else if cancel || resp.should_close() {
        st.postpone = None;
    }
}

/// "Mon 09 Nov 2026 at 14:00", or just the day.
fn when_text(e: &Event) -> String {
    match e.time {
        Some(t) => format!("{} at {}", e.date.format("%a %d %b %Y"), t.format("%H:%M")),
        None => e.date.format("%a %d %b %Y").to_string(),
    }
}

/// Day, time and place of the item being edited.
fn edit_dialog(ui: &mut Ui, st: &mut CalendarState, action: &mut CalAction) {
    let private = st.private;
    let Some(ed) = &mut st.editing else { return };
    let mut save = false;
    let mut cancel = false;
    let resp = egui::Modal::new(egui::Id::unique("edit-calendar-item")).frame(theme::modal_frame(ui)).show(ui.ctx(), |ui| {
        ui.set_width(500.0);
        ui.horizontal(|ui| {
            theme::icon_badge(ui, icons::PENCIL_SIMPLE, theme::ACCENT, 40.0);
            ui.label(theme::serif("Edit", 22.0));
        });
        ui.add_space(4.0);
        if ed.details.title.is_none() {
            ui.label(if private { ed.event.anonymous_title() } else { &ed.event.title });
        }
        ui.add_space(8.0);
        let d = &mut ed.details;
        widgets::form_grid(ui, "edit-grid", |ui| {
            if let Some(title) = &mut d.title {
                ui.label("Title");
                widgets::text_field(ui, title, "What it is");
                ui.end_row();
            }
            ui.label("Date");
            widgets::opt_date(ui, "date", &mut ed.date);
            ui.end_row();
            ui.label("Time");
            widgets::opt_time(ui, "time", &mut d.time);
            ui.end_row();
            if let Some(place) = &mut d.place {
                match &mut place.online {
                    Some(online) => {
                        ui.label("Where");
                        ui.horizontal(|ui| {
                            ui.checkbox(online, format!("{} Online", icons::DESKTOP));
                            if *online {
                                for platform in Platform::ALL {
                                    ui.selectable_value(&mut place.platform, platform, platform.label());
                                }
                            }
                        });
                        ui.end_row();
                        if *online {
                            ui.label("Link");
                            ui.vertical(|ui| {
                                let hint = match place.platform {
                                    Platform::Teams => "Optional; otherwise the one in Settings",
                                    Platform::GoogleMeet => "meet.google.com/…",
                                    Platform::Zoom => "…zoom.us/j/…",
                                };
                                widgets::text_field(ui, &mut place.link, hint);
                                if !place.link.trim().is_empty() && !crate::ics::valid_link(&place.link) {
                                    ui.colored_label(theme::fg(ui, theme::AMBER), format!("{} This doesn't look like a link", icons::WARNING));
                                }
                            });
                        } else {
                            ui.label("Room");
                            widgets::text_field(ui, &mut place.location, "Room / location");
                        }
                        ui.end_row();
                    }
                    None => {
                        ui.label("Location");
                        widgets::text_field(ui, &mut place.location, "Room / location");
                        ui.end_row();
                    }
                }
            }
        });
        if ed.event.series.is_some() {
            ui.add_space(4.0);
            ui.add(
                egui::Label::new(
                    RichText::new(format!(
                        "{} A week of a repeating meeting: the date and time change this week only, the place every week. \
                         To change every week, open the student.",
                        icons::REPEAT
                    ))
                    .small()
                    .color(theme::muted(ui)),
                )
                .wrap(),
            );
        }
        ui.add_space(12.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add_enabled(ed.date.is_some(), theme::primary_button("Save")).clicked() {
                save = true;
            }
            if ui.add(egui::Button::new("Cancel").min_size(egui::vec2(0.0, 34.0))).clicked() {
                cancel = true;
            }
        });
    });
    if save && let Some(date) = ed.date {
        let details = EventDetails { date, ..ed.details.clone() };
        *action = CalAction::Edit(ed.event.clone(), details);
        st.editing = None;
    } else if cancel || resp.should_close() {
        st.editing = None;
    }
}

/// What deleting `e` does, in a sentence.
fn delete_effect(e: &Event) -> &'static str {
    if e.uid.starts_with("visit") {
        "The visit's date and time are cleared; its other details stay."
    } else if e.uid.starts_with("phd-viva-") {
        "The date and time are cleared; the outcome and other details stay."
    } else if e.series.is_some() {
        "Cancel just this week, or delete the meeting with all its weeks and notes."
    } else if e.kind == EventKind::Other {
        "It is removed from the calendar."
    } else {
        "The meeting and its notes are removed from the student's record."
    }
}

/// Confirms deleting one or more items.
fn delete_dialog(ui: &mut Ui, st: &mut CalendarState, action: &mut CalAction) {
    let Some(events) = &st.deleting else { return };
    let single = (events.len() == 1).then(|| &events[0]);
    let series_week = single.is_some_and(|e| e.series.is_some());
    let mut chosen = None;
    let mut keep = false;
    let resp = egui::Modal::new(egui::Id::unique("delete-calendar-items")).frame(theme::modal_frame(ui)).show(ui.ctx(), |ui| {
        ui.set_width(460.0);
        ui.horizontal(|ui| {
            theme::icon_badge(ui, icons::TRASH, theme::RED, 40.0);
            let title = match single {
                Some(_) => "Delete this item?".to_string(),
                None => format!("Delete {} items?", events.len()),
            };
            ui.label(theme::serif(title, 22.0));
        });
        ui.add_space(6.0);
        match single {
            Some(e) => {
                ui.label(st.title(e));
                ui.label(RichText::new(when_text(e)).small().color(theme::muted(ui)));
                ui.add_space(6.0);
                ui.label(delete_effect(e));
            }
            None => {
                for e in events.iter().take(6) {
                    ui.label(format!("• {} – {}", e.date.format("%a %d %b"), st.title(e)));
                }
                if events.len() > 6 {
                    ui.label(RichText::new(format!("and {} more", events.len() - 6)).color(theme::muted(ui)));
                }
                ui.add_space(6.0);
                let mut note = "Meetings are removed from the students' records; visits and vivas lose their date.".to_string();
                if events.iter().any(|e| e.series.is_some()) {
                    note.push_str(" Weeks of repeating meetings are cancelled, and the rest of the series stays.");
                }
                ui.add(egui::Label::new(RichText::new(note).small().color(theme::muted(ui))).wrap());
            }
        }
        ui.add_space(12.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if series_week {
                if ui.add(theme::danger_button("Delete every week")).clicked() {
                    chosen = Some(true);
                }
                if ui.add(theme::primary_button("Cancel this week")).clicked() {
                    chosen = Some(false);
                }
            } else if ui.add(theme::danger_button("Delete")).clicked() {
                chosen = Some(false);
            }
            if ui.add(egui::Button::new("Keep").min_size(egui::vec2(0.0, 34.0))).clicked() {
                keep = true;
            }
        });
    });
    if let Some(whole_series) = chosen {
        *action = CalAction::Delete(events.clone(), whole_series);
        st.deleting = None;
    } else if keep || resp.should_close() {
        st.deleting = None;
    }
}

fn list(
    ui: &mut Ui,
    db: &Database,
    st: &mut CalendarState,
    events: &[&Event],
    now: NaiveDateTime,
    action: &mut CalAction,
) {
    let today = now.date();
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

    // Group by the first day of each week.
    let mut weeks: BTreeMap<NaiveDate, Vec<&Event>> = BTreeMap::new();
    for e in events {
        weeks.entry(db.settings.week_start(e.date)).or_default().push(e);
    }

    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            for (first, items) in weeks {
                let width = ui.available_width();
                if let Some(&height) = st.week_heights.get(&first) {
                    let rect = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(width, height));
                    if !ui.is_rect_visible(rect) {
                        ui.allocate_space(egui::vec2(width, height));
                        ui.add_space(10.0);
                        continue;
                    }
                }
                let last = first + Duration::days(6);
                let dates = format!("{} – {}", first.format("%d %b"), last.format("%d %b %Y"));
                let heading = match teaching_week_of(db, first) {
                    Some((sem, week)) => {
                        let sem = if sem == Semester::S2 { 2 } else { 1 };
                        format!("Semester {sem}, week {week}")
                    }
                    None => "Outside teaching".to_string(),
                };
                let this_week = (first..=last).contains(&today);
                let selectable: Vec<&str> = items
                    .iter()
                    .filter(|e| !e.cancelled)
                    .map(|e| e.uid.as_str())
                    .collect();
                let card = theme::card_frame(ui).inner_margin(egui::Margin::symmetric(16, 12)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let mut on = !selectable.is_empty()
                        && selectable.iter().all(|uid| st.selected.contains(*uid));
                    ui.push_id(first, |ui| {
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
                    row(ui, db, e, st, now, action);
                }
                });
                st.week_heights.insert(first, card.response.rect.height());
                ui.add_space(10.0);
            }
        });
}

/// Teaching week of a calendar week, if any of its weekdays is taught.
fn teaching_week_of(db: &Database, first: NaiveDate) -> Option<(Semester, i64)> {
    (0..7)
        .map(|d| first + Duration::days(d))
        .filter(|d| d.weekday().num_days_from_monday() < 5)
        .find_map(|d| db.settings.teaching_week(d))
}

/// One item. Clicking its title opens the student; right-clicking it
/// offers the rest.
fn row(ui: &mut Ui, db: &Database, e: &Event, st: &mut CalendarState, now: NaiveDateTime, action: &mut CalAction) {
    let started = started(e, now);
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
            let mut title = RichText::new(st.title(e));
            if e.cancelled {
                title = title.strikethrough().color(theme::muted(ui));
            } else if e.done {
                title = title.color(theme::muted(ui));
            }
            let has_student = !e.student_id.is_nil();
            let hover = if has_student {
                "Open student · right-click for more"
            } else {
                "Right-click to edit or delete"
            };
            let cursor = if has_student {
                egui::CursorIcon::PointingHand
            } else {
                egui::CursorIcon::Default
            };
            let title = ui
                .add(egui::Label::new(title).sense(egui::Sense::click()))
                .on_hover_cursor(cursor)
                .on_hover_text(hover);
            if title.clicked() && has_student {
                *action = CalAction::Open(e.student_id);
            }
            title.context_menu(|ui| item_menu(ui, db, e, st, now, action));
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
                ui.label(RichText::new(format!("{} {}", icons::DESKTOP, e.platform.label())).small().color(theme::muted(ui)));
            } else if !st.place(e).is_empty() {
                ui.label(
                    RichText::new(format!("{} {}", icons::MAP_PIN, e.location))
                        .small()
                        .color(theme::muted(ui)),
                );
            }
            if started && !e.done {
                if ui
                    .small_button(format!("{} Held", icons::CHECK))
                    .on_hover_text("Mark this meeting as held")
                    .clicked()
                {
                    *action = CalAction::Change(e.clone(), MeetingChange::Held(true));
                }
                if ui
                    .small_button("Postpone…")
                    .on_hover_text("Move this meeting to another day")
                    .clicked()
                {
                    st.postpone = Some(Postpone::new(e, now.date()));
                }
            }
        });
    });
}

/// Short text for a month cell: the student, or the coursework for deadlines.
fn cell_text(db: &Database, st: &CalendarState, e: &Event) -> String {
    let what = match e.kind {
        _ if st.private => st.title(e).to_string(),
        EventKind::Deadline | EventKind::Reminder => e.title.split(" – ").next().unwrap_or(&e.title).to_string(),
        EventKind::Other => e.title.clone(),
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
    now: NaiveDateTime,
    action: &mut CalAction,
) {
    let mut by_day: BTreeMap<NaiveDate, Vec<&Event>> = BTreeMap::new();
    for e in events {
        by_day.entry(e.date).or_default().push(e);
    }
    let first = db.settings.week_start(st.month);
    let last = st.month + Months::new(1) - Duration::days(1);
    let gap = 4.0;
    let week_col = 40.0;
    // Leave room for the scroll bar and the cell borders.
    let col = ((ui.available_width() - week_col - gap * 8.0 - 16.0) / 7.0).max(90.0);

    ui.label(
        RichText::new(
            "Click an item to select it, double-click to open the student, drag it to another day to move it. \
             Right-click for more; press Delete to remove the selected items.",
        )
            .small()
            .color(theme::muted(ui)),
    );
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
            ui.horizontal(|ui| {
                ui.add_sized([week_col, 18.0], egui::Label::new(""));
                for day in day_names(db) {
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
            let mut week = first;
            while week <= last {
                let busiest = (0..7)
                    .map(|d| {
                        by_day
                            .get(&(week + Duration::days(d)))
                            .map_or(0, Vec::len)
                    })
                    .max()
                    .unwrap_or(0);
                let height = (24.0 + 22.0 * busiest as f32).max(84.0);
                ui.horizontal_top(|ui| {
                    let label = teaching_week_of(db, week)
                        .map_or_else(String::new, |(_, w)| format!("W{w}"));
                    ui.add_sized(
                        [week_col, 22.0],
                        egui::Label::new(theme::semibold(label).small().color(theme::fg(ui, theme::ACCENT))),
                    )
                    .on_hover_text("Teaching week");
                    for d in 0..7 {
                        let date = week + Duration::days(d);
                        let items = by_day.get(&date).map_or(&[][..], Vec::as_slice);
                        day_cell(ui, db, st, date, items, now, egui::vec2(col, height), action);
                    }
                });
                week += Duration::days(7);
            }
        });
}

/// One day of the month grid. Items can be dragged onto another day.
#[allow(clippy::too_many_arguments)]
fn day_cell(
    ui: &mut Ui,
    db: &Database,
    st: &mut CalendarState,
    date: NaiveDate,
    items: &[&Event],
    now: NaiveDateTime,
    size: egui::Vec2,
    action: &mut CalAction,
) {
    let today = now.date();
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
    let cell = egui::Frame::NONE
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
                    ui.push_id(&e.uid, |ui| pill(ui, db, st, e, now, action));
                }
            });
        })
        .response;
    if let Some(dragged) = cell.dnd_hover_payload::<Dragged>()
        && dragged.0.date != date
    {
        ui.painter().rect_stroke(
            cell.rect,
            egui::CornerRadius::same(10),
            egui::Stroke::new(2.0, theme::fg(ui, theme::ACCENT)),
            egui::StrokeKind::Inside,
        );
    }
    if let Some(dragged) = cell.dnd_release_payload::<Dragged>()
        && dragged.0.date != date
    {
        *action = CalAction::Move(dragged.0.clone(), date);
    }
}

/// One item of a month cell: click to select, double-click to open the
/// student, drag to move, right-click for more.
fn pill(ui: &mut Ui, db: &Database, st: &mut CalendarState, e: &Event, now: NaiveDateTime, action: &mut CalAction) {
    let (_, color) = theme::kind_style(e.kind);
    let mut text = RichText::new(cell_text(db, st, e)).small();
    text = if e.cancelled {
        text.strikethrough().color(theme::muted(ui))
    } else if e.done {
        text.color(theme::muted(ui))
    } else {
        text.color(theme::fg(ui, color))
    };
    let selected = st.selected.contains(&e.uid);
    let faded = e.cancelled || e.done;
    let movable = e.editable() && !e.cancelled;
    let mut hover = st.title(e).to_string();
    if e.cancelled {
        hover.push_str("\nCancelled");
    } else if e.moved() {
        hover.push_str("\nMoved");
    }
    if e.online {
        hover.push_str(&format!("\n{} {}", icons::DESKTOP, e.platform.label()));
    } else if !st.place(e).is_empty() {
        hover.push_str(&format!("\n{} {}", icons::MAP_PIN, e.location));
    }
    if movable {
        hover.push_str("\nDrag to another day to move it; right-click to edit or delete");
    } else if e.cancelled && e.series.is_some() {
        hover.push_str("\nRight-click to restore this week");
    }
    let sense = if movable {
        egui::Sense::click_and_drag()
    } else {
        egui::Sense::click()
    };
    let button = egui::Button::new(text.clone())
        .truncate()
        .sense(sense)
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
            ui.add(button)
        })
        .inner;
    if movable {
        resp.dnd_set_drag_payload(Dragged(e.clone()));
    }
    if resp.dragged()
        && let Some(pos) = ui.ctx().pointer_interact_pos()
    {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        egui::Area::new(egui::Id::unique("calendar-dragged"))
            .order(egui::Order::Tooltip)
            .interactable(false)
            .fixed_pos(pos + egui::vec2(12.0, 8.0))
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label(text);
                });
            });
    }
    let resp = resp.on_hover_text(hover);
    if resp.clicked() && !e.cancelled {
        if selected {
            st.selected.remove(&e.uid);
        } else {
            st.selected.insert(e.uid.clone());
        }
    }
    if resp.double_clicked() && !e.student_id.is_nil() {
        *action = CalAction::Open(e.student_id);
    }
    resp.context_menu(|ui| item_menu(ui, db, e, st, now, action));
}

/// The events of an imported calendar, each to be skipped, kept in the
/// calendar, or made a meeting of a student.
pub struct ImportDialog {
    file: String,
    rows: Vec<ImportRow>,
    show_past: bool,
    search: String,
}

struct ImportRow {
    event: ImportedEvent,
    take: bool,
    student: Option<(Uuid, MeetingRole)>,
    /// Why it may not be wanted (came from Supervisor, imported before…).
    known: Option<Known>,
    /// It is over (a repeating one, entirely).
    past: bool,
}

impl ImportRow {
    /// Ticked although it is in Supervisor already, so it would show twice.
    fn duplicates(&self) -> bool {
        self.take && self.known.is_some_and(Known::duplicate)
    }
}

impl ImportDialog {
    /// Ticks the events that are still to come and are with a student.
    pub fn new(file: String, events: Vec<ImportedEvent>, db: &Database) -> Self {
        let today = widgets::today();
        let rows = events
            .into_iter()
            .map(|event| {
                let known = db.known_import(&event.uid);
                let student = db.suggest_student(&event);
                let last = event.meeting.repeat_end(&db.settings).or(event.meeting.date);
                let past = last.is_some_and(|d| d < today);
                ImportRow {
                    take: known.is_none() && student.is_some() && !past,
                    event,
                    student,
                    known,
                    past,
                }
            })
            .collect();
        Self {
            file,
            rows,
            show_past: false,
            search: String::new(),
        }
    }

    fn visible(&self, row: &ImportRow) -> bool {
        let q = self.search.trim().to_lowercase();
        (self.show_past || !row.past) && (q.is_empty() || row.event.title.to_lowercase().contains(&q))
    }

    /// What to do with each event: ticked ones are added, unticked new
    /// ones are remembered as skipped.
    fn choices(&self) -> Vec<(ImportedEvent, ImportTarget)> {
        self.rows
            .iter()
            .filter_map(|r| {
                let target = match (r.take, r.student) {
                    (true, Some((id, role))) => ImportTarget::Student(id, role),
                    (true, None) => ImportTarget::Calendar,
                    (false, _) if r.known.is_none() => ImportTarget::Ignore,
                    (false, _) => return None,
                };
                Some((r.event.clone(), target))
            })
            .collect()
    }
}

/// A student who can have meetings: id, name and roles with meetings.
type Linkable = (Uuid, String, Vec<MeetingRole>);

fn import_dialog(ui: &mut Ui, db: &Database, st: &mut CalendarState, action: &mut CalAction) {
    let Some(dlg) = &mut st.import else { return };
    let mut students: Vec<Linkable> = db
        .students
        .iter()
        .filter(|s| !s.archived)
        .map(|s| (s.id, s.display_name(), MeetingRole::of(s)))
        .filter(|(_, _, roles)| !roles.is_empty())
        .collect();
    students.sort_by_key(|(_, name, _)| name.to_lowercase());
    let mut import = false;
    let mut cancel = false;
    let screen = ui.ctx().content_rect();
    let resp = egui::Modal::new(egui::Id::unique("import-calendar")).frame(theme::modal_frame(ui)).show(ui.ctx(), |ui| {
        ui.set_width((screen.width() - 120.0).clamp(560.0, 940.0));
        ui.horizontal(|ui| {
            theme::icon_badge(ui, icons::DOWNLOAD_SIMPLE, theme::ACCENT, 40.0);
            ui.vertical(|ui| {
                ui.label(theme::serif("Import calendar", 22.0));
                ui.label(RichText::new(&dlg.file).small().color(theme::muted(ui)));
            });
        });
        ui.add_space(6.0);
        ui.add(
            egui::Label::new(
                RichText::new(
                    "Tick the events to bring in. Linked to a student, an event becomes one of their meetings; \
                     without a student it stays in the calendar under “Other”. Events you leave unticked are \
                     remembered as skipped next time.",
                )
                .color(theme::muted(ui)),
            )
            .wrap(),
        );
        ui.add_space(8.0);
        let past = dlg.rows.iter().filter(|r| r.past).count();
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(icons::MAGNIFYING_GLASS).color(theme::muted(ui)));
            ui.add(egui::TextEdit::singleline(&mut dlg.search).hint_text("Search titles").desired_width(200.0));
            if past > 0 {
                ui.checkbox(&mut dlg.show_past, format!("Show past events ({past})"));
            }
            ui.separator();
            let shown: Vec<usize> = (0..dlg.rows.len()).filter(|&i| dlg.visible(&dlg.rows[i])).collect();
            if ui
                .button("Tick all shown")
                .on_hover_text("Events already in Supervisor stay unticked")
                .clicked()
            {
                for &i in &shown {
                    let row = &mut dlg.rows[i];
                    row.take = !row.known.is_some_and(Known::duplicate);
                }
            }
            if ui.button("Untick all shown").clicked() {
                for &i in &shown {
                    dlg.rows[i].take = false;
                }
            }
        });
        ui.add_space(6.0);

        let shown: Vec<usize> = (0..dlg.rows.len()).filter(|&i| dlg.visible(&dlg.rows[i])).collect();
        let row_height = 30.0;
        let list_height = (screen.height() - 360.0).clamp(160.0, 520.0);
        egui::Frame::NONE
            .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
            .corner_radius(egui::CornerRadius::same(10))
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                if shown.is_empty() {
                    ui.label(RichText::new("No events to show.").color(theme::muted(ui)));
                    return;
                }
                egui::ScrollArea::vertical()
                    .max_height(list_height)
                    .auto_shrink([false, true])
                    .show_rows(ui, row_height, shown.len(), |ui, range| {
                        for &i in &shown[range] {
                            ui.push_id(i, |ui| import_row(ui, &mut dlg.rows[i], &students, row_height));
                        }
                    });
            });

        ui.add_space(10.0);
        let taken = dlg.rows.iter().filter(|r| r.take).count();
        let linked = dlg.rows.iter().filter(|r| r.take && r.student.is_some()).count();
        let twice = dlg.rows.iter().filter(|r| r.duplicates()).count();
        ui.horizontal(|ui| {
            let summary = match (taken, linked) {
                (0, _) => "Nothing ticked".to_string(),
                (t, 0) => format!("{t} to import, none linked to a student"),
                (t, l) => format!("{t} to import, {l} as student meetings"),
            };
            ui.label(RichText::new(summary).color(theme::muted(ui)));
            if twice > 0 {
                let text = match twice {
                    1 => "1 is already in Supervisor".to_string(),
                    n => format!("{n} are already in Supervisor"),
                };
                ui.colored_label(theme::fg(ui, theme::AMBER), format!("{} {text}", icons::WARNING))
                    .on_hover_text("They will show twice in the calendar");
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = if taken == 0 { "Done".to_string() } else { format!("Import {taken}") };
                if ui.add(theme::primary_button(label)).clicked() {
                    import = true;
                }
                if ui.add(egui::Button::new("Cancel").min_size(egui::vec2(0.0, 34.0))).clicked() {
                    cancel = true;
                }
            });
        });
    });
    if import {
        *action = CalAction::Import(dlg.choices());
        st.import = None;
    } else if cancel || resp.should_close() {
        st.import = None;
    }
}

/// One event of the import list: tick, when, title, and the student it goes to.
fn import_row(ui: &mut Ui, row: &mut ImportRow, students: &[Linkable], height: f32) {
    let m = &row.event.meeting;
    ui.horizontal(|ui| {
        ui.set_height(height - 4.0);
        ui.checkbox(&mut row.take, "");
        let when = match (m.date, m.time) {
            (Some(d), Some(t)) => format!("{} {}", d.format("%a %d %b %Y"), t.format("%H:%M")),
            (Some(d), None) => format!("{} all day", d.format("%a %d %b %Y")),
            (None, _) => String::new(),
        };
        let color = if row.past || row.known.is_some() {
            theme::muted(ui)
        } else {
            ui.visuals().text_color()
        };
        let left = egui::Layout::left_to_right(egui::Align::Center);
        ui.allocate_ui_with_layout(egui::vec2(150.0, 20.0), left, |ui| {
            ui.set_min_width(150.0);
            ui.label(RichText::new(when).color(color));
        });
        let mut hover = row.event.title.clone();
        if m.online {
            hover.push_str(&format!("\n{} {} {}", icons::DESKTOP, m.platform.label(), m.link));
        } else if !m.location.is_empty() {
            hover.push_str(&format!("\n{} {}", icons::MAP_PIN, m.location));
        }
        if let Some(r) = &m.repeat {
            let until = r.until.map_or_else(|| "the end of teaching".to_string(), |d| widgets::fmt_date(Some(d)));
            let every = match r.every_weeks {
                0 | 1 => "Every week".to_string(),
                n => format!("Every {n} weeks"),
            };
            hover.push_str(&format!("\n{} {every} until {until}", icons::REPEAT));
        }
        if row.event.repeat_dropped {
            hover.push_str("\nRepeats in a way Supervisor can't follow: only the first date is imported");
        }
        // Room on the right for the student and role.
        let title_width = (ui.available_width() - 360.0).max(120.0);
        let title = if row.event.title.is_empty() { "(no title)" } else { &row.event.title };
        ui.allocate_ui_with_layout(egui::vec2(title_width, 20.0), left, |ui| {
            ui.set_min_width(title_width);
            ui.add(egui::Label::new(RichText::new(title).color(color)).truncate())
                .on_hover_text(hover);
        });
        if m.repeat.is_some() || row.event.repeat_dropped {
            ui.label(RichText::new(icons::REPEAT).color(theme::muted(ui)));
        }
        // Not `row.duplicates()`: the event is borrowed here.
        if row.take && row.known.is_some_and(Known::duplicate) {
            theme::chip(ui, format!("{} will show twice", icons::WARNING), theme::AMBER).on_hover_text(format!(
                "This event is {}: importing it again adds a second copy",
                match row.known {
                    Some(Known::FromSupervisor) => "from Supervisor",
                    _ => "imported already",
                }
            ));
        } else if let Some(known) = row.known {
            theme::chip(ui, known.label(), theme::GREY);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let linked = row
                .student
                .and_then(|(id, _)| students.iter().find(|(s, _, _)| *s == id));
            if let (Some((_, role)), Some((_, _, roles))) = (&mut row.student, linked)
                && roles.len() > 1
            {
                egui::ComboBox::from_id_salt("role")
                    .selected_text(role.label())
                    .width(140.0)
                    .show_ui(ui, |ui| {
                        for r in roles {
                            ui.selectable_value(role, *r, r.label());
                        }
                    });
            }
            let name = linked.map_or_else(|| "No student".to_string(), |(_, name, _)| name.clone());
            let before = row.student.map(|(id, _)| id);
            let mut chosen = before;
            egui::ComboBox::from_id_salt("student")
                .selected_text(name)
                .width(180.0)
                .height(320.0)
                .truncate()
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut chosen, None, "No student (calendar only)");
                    for (id, name, _) in students {
                        ui.selectable_value(&mut chosen, Some(*id), name);
                    }
                });
            if chosen != before {
                row.student = chosen.and_then(|id| {
                    let (_, _, roles) = students.iter().find(|(s, _, _)| *s == id)?;
                    Some((id, roles[0]))
                });
                row.take = true;
            }
        });
    });
}
