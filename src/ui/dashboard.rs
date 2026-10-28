//! Home screen: counts, upcoming items, things needing attention, and a side
//! column with the month, the teaching week and coursework progress.

use std::collections::BTreeMap;

use chrono::{Datelike, Duration, Local, Months, NaiveDate, Timelike};
use egui::{Align2, Color32, CornerRadius, FontId, RichText, Sense, Ui};
use uuid::Uuid;

use crate::model::{self, Attention, Database, Event, EventKind, ResearchGroup, Settings, Severity};
use crate::ui::student_list::Category;
use crate::ui::{icons, theme, widgets};

pub enum DashAction {
    None,
    Open(Uuid),
    Goto(Category),
    ExportIcs,
    /// Show this month in the calendar.
    Month(NaiveDate),
    /// Switch privacy mode.
    Privacy,
}

const GAP: f32 = 16.0;

/// `events` and `attention` are the lists derived from `db`, built once per
/// change. In privacy mode (`private`) no student is named.
pub fn show(ui: &mut Ui, db: &Database, events: &[Event], attention: &[Attention], private: bool) -> DashAction {
    let mut action = DashAction::None;
    let today = widgets::today();

    let first_name = db.settings.my_name.split_whitespace().next().unwrap_or("");
    let greeting = match Local::now().hour() {
        5..=11 => "Good morning",
        12..=17 => "Good afternoon",
        _ => "Good evening",
    };
    let title = if first_name.is_empty() {
        greeting.to_string()
    } else {
        format!("{greeting}, {first_name}")
    };
    let mut subtitle = format!("{} · {}", today.format("%A %d %B %Y"), db.settings.academic_year);
    if let Some((sem, week)) = db.settings.teaching_week(today) {
        subtitle.push_str(&format!(" · {}, week {week}", sem.label()));
    }
    theme::page_header(ui, &title, &subtitle, |ui| {
        let (text, hover) = if private {
            (
                format!("{}  Show names", icons::EYE),
                "Privacy mode is on: students' names and numbers are hidden here, in the lists, the calendar and their details. Click to show them again.",
            )
        } else {
            (
                format!("{}  Hide names", icons::EYE_SLASH),
                "Privacy mode: hide students' names and numbers here, in the lists, the calendar and their details, for when someone else can see your screen",
            )
        };
        if ui
            .add(egui::Button::new(text).selected(private).min_size(egui::vec2(0.0, 34.0)))
            .on_hover_text(hover)
            .clicked()
        {
            action = DashAction::Privacy;
        }
        if ui
            .add(theme::primary_button(format!("{}  Export calendar", icons::EXPORT)))
            .on_hover_text("Meetings, visits and deadlines as an .ics file. Note: the exported file is not encrypted.")
            .clicked()
        {
            action = DashAction::ExportIcs;
        }
    });

    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        // Keep the cards clear of the scroll bar.
        ui.set_max_width(ui.available_width() - 12.0);
        tiles(ui, db, events, today, &mut action);
        ui.add_space(GAP);

        let width = ui.available_width();
        let side = if width > 900.0 { (width * 0.36).clamp(300.0, 380.0) } else { 0.0 };
        let main = width - side - if side > 0.0 { GAP } else { 0.0 };
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            ui.allocate_ui(egui::vec2(main, 0.0), |ui| {
                ui.set_width(main);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = GAP;
                    upcoming(ui, db, events, today, private, &mut action);
                    needs_attention(ui, attention, private, &mut action);
                    if side == 0.0 {
                        side_column(ui, db, events, today, &mut action);
                    }
                });
            });
            if side > 0.0 {
                ui.allocate_ui(egui::vec2(side, 0.0), |ui| {
                    ui.set_width(side);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = GAP;
                        side_column(ui, db, events, today, &mut action);
                    });
                });
            }
        });
        ui.add_space(12.0);
    });
    action
}

fn side_column(ui: &mut Ui, db: &Database, events: &[Event], today: NaiveDate, action: &mut DashAction) {
    mini_month(ui, &db.settings, events, today, action);
    teaching(ui, db, today);
    coursework(ui, db, today);
}

/// One tile per list, as many per row as fit, stretched to fill the row.
fn tiles(ui: &mut Ui, db: &Database, events: &[Event], today: NaiveDate, action: &mut DashAction) {
    let lists = db.settings.programme_lists().into_iter().map(Category::Projects);
    // PhD always; the other research lists once they have students.
    let research = ResearchGroup::ALL
        .into_iter()
        .map(Category::Research)
        .filter(|c| *c == Category::Research(ResearchGroup::Phd) || c.count(db) > 0);
    let tiles: Vec<Category> = lists
        .chain(research)
        .chain([Category::Placement, Category::Advisee])
        .collect();
    let width = ui.available_width();
    let fit = ((width + GAP) / (190.0 + GAP)).floor().clamp(1.0, tiles.len() as f32) as usize;
    // Balance the rows, e.g. 3 + 3 rather than 5 + 1.
    let per_row = tiles.len().div_ceil(tiles.len().div_ceil(fit));
    let tile_w = (width - GAP * (per_row - 1) as f32) / per_row as f32;
    let week_end = today + Duration::days(7);
    for (r, row) in tiles.chunks(per_row).enumerate() {
        if r > 0 {
            ui.add_space(GAP);
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for cat in row {
                let (icon, color, caption) = match cat {
                    Category::Projects(list) => (icons::BOOK_OPEN_TEXT, theme::ACCENT, format!("{list} projects")),
                    Category::Research(group) => (icons::GRADUATION_CAP, theme::PLUM, format!("{} students", group.label())),
                    Category::Placement => (icons::BUILDINGS, theme::AMBER, "Placements".into()),
                    Category::Advisee => (icons::CHAT_CIRCLE_TEXT, theme::GREEN, "Studies advisees".into()),
                    Category::All => (icons::USERS_THREE, theme::GREY, "All students".into()),
                };
                let this_week = events
                    .iter()
                    .filter(|e| {
                        !e.cancelled
                            && !matches!(e.kind, EventKind::Deadline | EventKind::Reminder)
                            && (today..week_end).contains(&e.date)
                            && db.student(e.student_id).is_some_and(|s| {
                                !s.archived
                                    && cat.includes(s, &db.settings)
                                    && match cat {
                                        Category::Placement => e.kind == EventKind::Visit,
                                        Category::Advisee => e.kind == EventKind::AdviseeMeeting,
                                        Category::Research(_) => e.kind == EventKind::PhdMeeting,
                                        _ => e.kind == EventKind::FypMeeting,
                                    }
                            })
                    })
                    .count();
                let note = match this_week {
                    0 => "nothing in the next 7 days".to_string(),
                    1 => "1 in the next 7 days".to_string(),
                    n => format!("{n} in the next 7 days"),
                };
                let resp = theme::card_frame(ui)
                    .inner_margin(egui::Margin::symmetric(16, 14))
                    .show(ui, |ui| {
                        ui.set_width(tile_w - 34.0);
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                theme::icon_badge(ui, icon, color, 40.0);
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    ui.add(egui::Label::new(RichText::new(caption).color(theme::muted(ui))).truncate());
                                    ui.label(theme::semibold(cat.count(db).to_string()).size(26.0));
                                });
                            });
                            ui.add_space(2.0);
                            ui.add(egui::Label::new(RichText::new(note).small().color(theme::muted(ui))).truncate());
                        });
                    })
                    .response
                    .interact(Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                if resp.hovered() {
                    ui.painter().rect_stroke(
                        resp.rect,
                        CornerRadius::same(14),
                        egui::Stroke::new(1.0, theme::fg(ui, color).gamma_multiply(0.6)),
                        egui::StrokeKind::Inside,
                    );
                }
                if resp.clicked() {
                    *action = DashAction::Goto(cat.clone());
                }
            }
        });
    }
}

/// A rounded date block: weekday, day number and month, tinted by kind.
fn date_badge(ui: &mut Ui, date: NaiveDate, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(48.0, 52.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(10), theme::soft(ui, color));
    let c = theme::fg(ui, color);
    painter.text(
        rect.center_top() + egui::vec2(0.0, 10.0),
        Align2::CENTER_CENTER,
        date.format("%a").to_string().to_uppercase(),
        FontId::proportional(10.0),
        c,
    );
    painter.text(
        rect.center() + egui::vec2(0.0, 1.0),
        Align2::CENTER_CENTER,
        date.day().to_string(),
        FontId::new(19.0, egui::FontFamily::Name("semibold".into())),
        c,
    );
    painter.text(
        rect.center_bottom() - egui::vec2(0.0, 10.0),
        Align2::CENTER_CENTER,
        date.format("%b").to_string().to_uppercase(),
        FontId::proportional(10.0),
        c,
    );
}

fn upcoming(ui: &mut Ui, db: &Database, events: &[Event], today: NaiveDate, private: bool, action: &mut DashAction) {
    let days = db.settings.upcoming_days;
    let items = model::upcoming(events, today, days);
    theme::card(ui, &format!("Upcoming · next {days} days"), |ui| {
        if items.is_empty() {
            empty(ui, icons::CALENDAR_CHECK, "Nothing scheduled.");
        }
        for (i, e) in items.iter().enumerate() {
            if i > 0 {
                ui.separator();
            }
            let (icon, color) = theme::kind_style(e.kind);
            let row = ui
                .horizontal(|ui| {
                    date_badge(ui, e.date, color);
                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 3.0;
                        ui.add_space(4.0);
                        // Titles and places (a company, say) can name the student.
                        let title = if private { e.anonymous_title() } else { &e.title };
                        ui.add(egui::Label::new(theme::semibold(title)).truncate());
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 12.0;
                            let muted = theme::muted(ui);
                            let when = match e.time {
                                Some(t) => format!("{} {}", icons::CLOCK, t.format("%H:%M")),
                                None => format!("{} all day", icons::CLOCK),
                            };
                            ui.label(RichText::new(when).small().color(muted));
                            ui.label(RichText::new(format!("{icon} {}", kind_name(e.kind))).small().color(theme::fg(ui, color)));
                            if e.online {
                                ui.label(RichText::new(format!("{} {}", icons::DESKTOP, e.platform.label())).small().color(muted));
                            } else if !e.location.is_empty() && !private {
                                ui.label(RichText::new(format!("{} {}", icons::MAP_PIN, e.location)).small().color(muted));
                            }
                        });
                    });
                })
                .response
                .interact(Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("Open student");
            if row.clicked() {
                *action = DashAction::Open(e.student_id);
            }
        }
    });
}

fn kind_name(kind: EventKind) -> &'static str {
    match kind {
        EventKind::FypMeeting => "Project meeting",
        EventKind::Visit => "Placement visit",
        EventKind::AdviseeMeeting => "Advisee meeting",
        EventKind::PhdMeeting => "Research",
        EventKind::Deadline => "Deadline",
        EventKind::Reminder => "Reminder",
        EventKind::Other => "Event",
    }
}

fn empty(ui: &mut Ui, icon: &str, text: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(8.0);
        ui.label(RichText::new(icon).size(26.0).color(theme::muted(ui)));
        ui.label(RichText::new(text).color(theme::muted(ui)));
        ui.add_space(8.0);
    });
}

fn needs_attention(ui: &mut Ui, items: &[Attention], private: bool, action: &mut DashAction) {
    theme::card(ui, "Needs attention", |ui| {
        if items.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(8.0);
                ui.label(RichText::new(icons::CHECK_CIRCLE).size(26.0).color(theme::fg(ui, theme::GREEN)));
                ui.label(RichText::new("All clear").color(theme::fg(ui, theme::GREEN)));
                ui.add_space(8.0);
            });
        }
        for a in items {
            let (icon, color, tag) = match a.severity {
                Severity::Overdue => (icons::WARNING_CIRCLE, theme::RED, "overdue"),
                Severity::Pending => (icons::HOURGLASS, theme::AMBER, "pending"),
            };
            let row = egui::Frame::new()
                .fill(ui.visuals().faint_bg_color)
                .corner_radius(CornerRadius::same(10))
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(icon).size(17.0).color(theme::fg(ui, color)));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            theme::chip(ui, tag, color);
                            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                let message = if private { &a.detail } else { &a.message };
                                ui.add(egui::Label::new(message).truncate());
                            });
                        });
                    });
                })
                .response
                .interact(Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if row.clicked() {
                *action = DashAction::Open(a.student_id);
            }
        }
    });
}

/// This month, with a dot per kind of item on each day.
fn mini_month(ui: &mut Ui, settings: &Settings, events: &[Event], today: NaiveDate, action: &mut DashAction) {
    let month = today.with_day(1).unwrap_or(today);
    let next = month + Months::new(1);
    let mut kinds: BTreeMap<NaiveDate, Vec<EventKind>> = BTreeMap::new();
    for e in events.iter().filter(|e| !e.cancelled && e.date >= month && e.date < next) {
        let k = kinds.entry(e.date).or_default();
        if !k.contains(&e.kind) {
            k.push(e.kind);
        }
    }
    theme::card(ui, "", |ui| {
        ui.horizontal(|ui| {
            ui.label(theme::serif(month.format("%B %Y").to_string(), 20.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(egui::Button::new(RichText::new(icons::ARROW_RIGHT).size(15.0)).frame(false))
                    .on_hover_text("Open in the calendar")
                    .clicked()
                {
                    *action = DashAction::Month(month);
                }
            });
        });
        ui.add_space(4.0);
        let cell = (ui.available_width() / 7.0).floor();
        let muted = theme::muted(ui);
        let letters = if settings.week_starts_sunday {
            ["S", "M", "T", "W", "T", "F", "S"]
        } else {
            ["M", "T", "W", "T", "F", "S", "S"]
        };
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for d in letters {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(cell, 18.0), Sense::hover());
                ui.painter().text(rect.center(), Align2::CENTER_CENTER, d, FontId::proportional(11.0), muted);
            }
        });
        let lead = month.signed_duration_since(settings.week_start(month)).num_days();
        let days = (next - month).num_days();
        let rows = (lead + days + 6) / 7;
        let ink = ui.visuals().text_color();
        for r in 0..rows {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for c in 0..7 {
                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(cell, 34.0), Sense::click());
                    let n = r * 7 + c - lead;
                    if !(0..days).contains(&n) {
                        continue;
                    }
                    let date = month + Duration::days(n);
                    let painter = ui.painter();
                    let centre = rect.center() - egui::vec2(0.0, 4.0);
                    let is_today = date == today;
                    if is_today {
                        painter.circle_filled(centre, 12.0, theme::ACCENT);
                    } else if resp.hovered() {
                        painter.circle_filled(centre, 12.0, ui.visuals().widgets.hovered.bg_fill);
                    }
                    let weekend = matches!(date.weekday(), chrono::Weekday::Sat | chrono::Weekday::Sun);
                    let color = if is_today {
                        Color32::WHITE
                    } else if weekend {
                        muted
                    } else {
                        ink
                    };
                    painter.text(centre, Align2::CENTER_CENTER, date.day().to_string(), FontId::proportional(12.5), color);
                    if let Some(k) = kinds.get(&date) {
                        let w = 6.0 * k.len() as f32;
                        for (i, kind) in k.iter().enumerate() {
                            let x = rect.center().x - w / 2.0 + 3.0 + 6.0 * i as f32;
                            painter.circle_filled(egui::pos2(x, rect.bottom() - 3.0), 2.0, theme::fg(ui, theme::kind_style(*kind).1));
                        }
                    }
                    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        *action = DashAction::Month(month);
                    }
                }
            });
        }
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            for (kind, label) in [
                (EventKind::FypMeeting, "Project"),
                (EventKind::Visit, "Visit"),
                (EventKind::AdviseeMeeting, "Advisee"),
                (EventKind::PhdMeeting, "Research"),
                (EventKind::Deadline, "Deadline"),
                (EventKind::Reminder, "Reminder"),
                (EventKind::Other, "Other"),
            ] {
                let color = theme::fg(ui, theme::kind_style(kind).1);
                // Keep each dot with its name when the legend wraps.
                ui.add(egui::Label::new(RichText::new(format!("● {label}")).small().color(color)).wrap_mode(egui::TextWrapMode::Extend));
            }
        });
    });
}

/// Where we are in the teaching semester.
fn teaching(ui: &mut Ui, db: &Database, today: NaiveDate) {
    let s = &db.settings;
    theme::card(ui, "Teaching", |ui| {
        let sem = s.semester_of(today);
        let (Some(start), Some(end)) = (s.semester_start(sem), s.teaching_end(sem)) else {
            ui.label(RichText::new("Set the semester dates in Settings to see the teaching weeks.").color(theme::muted(ui)));
            return;
        };
        let total = ((end - start).num_days() / 7 + 1).max(1);
        let label = sem.label();
        let (headline, fraction, note) = if today < start {
            let days = (start - today).num_days();
            (format!("{label} starts soon"), 0.0, format!("Week 1 begins {} · in {days} days", start.format("%a %d %b")))
        } else if today > end {
            (format!("{label} teaching is over"), 1.0, format!("Teaching ended {}", end.format("%a %d %b")))
        } else {
            let week = (today - start).num_days() / 7 + 1;
            (
                format!("Week {week} of {total}"),
                week as f32 / total as f32,
                format!("{label} · teaching ends {}", end.format("%a %d %b")),
            )
        };
        ui.label(theme::semibold(headline).size(18.0));
        ui.add_space(2.0);
        theme::progress(ui, fraction, theme::ACCENT, 8.0);
        ui.add_space(2.0);
        ui.label(RichText::new(note).small().color(theme::muted(ui)));
    });
}

/// Submitted, upcoming and overdue coursework across current project students.
fn coursework(ui: &mut Ui, db: &Database, today: NaiveDate) {
    let (mut done, mut due, mut overdue) = (0usize, 0usize, 0usize);
    for f in db.students.iter().filter(|s| !s.archived).filter_map(|s| s.fyp.as_ref()) {
        for module in db.settings.modules_for(&f.programme) {
            for cw in &module.courseworks {
                let status = f.coursework.get(&cw.id);
                if status.is_some_and(|s| s.submitted) {
                    done += 1;
                } else if db.settings.effective_deadline(module, cw, status).is_some_and(|d| d < today) {
                    overdue += 1;
                } else {
                    due += 1;
                }
            }
        }
    }
    theme::card(ui, "Coursework", |ui| {
        let total = done + due + overdue;
        if total == 0 {
            ui.label(RichText::new("No coursework for current project students.").color(theme::muted(ui)));
            return;
        }
        ui.horizontal(|ui| {
            let pct = format!("{}%", (done * 100 + total / 2) / total);
            theme::ring(
                ui,
                104.0,
                &[(done as f32, theme::GREEN), (due as f32, theme::ACCENT), (overdue as f32, theme::RED)],
                &pct,
                "submitted",
            );
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.add_space(10.0);
                for (n, label, color) in [
                    (done, "Submitted", theme::GREEN),
                    (due, "Still to come", theme::ACCENT),
                    (overdue, "Overdue", theme::RED),
                ] {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("●").color(theme::fg(ui, color)));
                        ui.label(label);
                        ui.label(theme::semibold(n.to_string()));
                    });
                }
            });
        });
    });
}
