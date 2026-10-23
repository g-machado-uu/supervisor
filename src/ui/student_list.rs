//! Category list views with status chips.

use chrono::NaiveDate;
use egui::{Color32, RichText, Sense, Ui};
use egui_extras::{Column, TableBuilder};
use uuid::Uuid;

use crate::model::{
    Database, FypRecord, Meeting, ReplyStatus, SemesterContact, Settings, Student, Visit,
};
use crate::ui::{theme, widgets};

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Category {
    /// Project students on the courses shown in this sidebar list.
    Projects(String),
    Placement,
    Advisee,
    All,
}

impl Category {
    pub fn title(&self) -> String {
        match self {
            Category::Projects(list) => format!("Project students – {list}"),
            Category::Placement => "Placement Students".into(),
            Category::Advisee => "Studies Advisees".into(),
            Category::All => "All Students".into(),
        }
    }

    pub fn includes(&self, s: &Student, settings: &Settings) -> bool {
        match self {
            Category::Projects(list) => s.fyp.as_ref().is_some_and(|f| {
                settings
                    .programme(&f.programme)
                    .is_some_and(|p| p.list_name() == list)
            }),
            Category::Placement => s.placement.is_some(),
            Category::Advisee => s.advisee.is_some(),
            Category::All => true,
        }
    }

    pub fn count(&self, db: &Database) -> usize {
        db.students
            .iter()
            .filter(|s| !s.archived && self.includes(s, &db.settings))
            .count()
    }
}

pub struct ListState<'a> {
    pub search: &'a mut String,
    pub show_archived: &'a mut bool,
    pub selected: &'a mut Option<Uuid>,
}

/// Returns true when "Add student" was clicked.
pub fn show(ui: &mut Ui, db: &Database, cat: &Category, st: ListState<'_>) -> bool {
    let mut add = false;
    ui.horizontal(|ui| {
        ui.heading(cat.title());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(egui::Button::new(RichText::new("➕ Add student").strong()).fill(theme::ACCENT.gamma_multiply(0.85)))
                .clicked()
            {
                add = true;
            }
        });
    });
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(st.search)
                .hint_text("🔍 Search name, B number, email, course, company…")
                .desired_width(360.0),
        );
        ui.checkbox(st.show_archived, "Show archived");
    });
    ui.add_space(6.0);

    let rows: Vec<&Student> = db
        .students
        .iter()
        .filter(|s| {
            (*st.show_archived || !s.archived)
                && cat.includes(s, &db.settings)
                && s.matches(st.search)
        })
        .collect();
    let mut rows = rows;
    rows.sort_by_key(|s| s.name.to_lowercase());

    if rows.is_empty() {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("No students here yet.")
                    .color(theme::GREY)
                    .size(16.0),
            );
        });
        return add;
    }

    let today = widgets::today();
    // Show which course when a list holds several.
    let mixed = match cat {
        Category::Projects(list) => {
            db.settings
                .programmes
                .iter()
                .filter(|p| p.list_name() == list)
                .count()
                > 1
        }
        _ => false,
    };
    let headers: &[&str] = match cat {
        Category::Projects(_) => &[
            "Name",
            "B Number",
            "Course",
            "Initial contact",
            "Meetings",
            "Coursework",
        ],
        Category::Placement => &["Name", "B Number", "Company", "Visit 1", "Visit 2"],
        Category::Advisee => &["Name", "B Number", "Course", "Semester 1", "Semester 2"],
        Category::All => &["Name", "B Number", "Email", "Course", "Roles"],
    };

    let mut table = TableBuilder::new(ui)
        .striped(true)
        .sense(Sense::click())
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::initial(170.0).at_least(110.0).resizable(true).clip(true))
        .column(Column::initial(100.0).resizable(true));
    for _ in 2..headers.len() - 1 {
        table = table.column(Column::initial(140.0).at_least(80.0).resizable(true).clip(true));
    }
    table
        .column(Column::remainder().at_least(140.0))
        .header(26.0, |mut header| {
            for h in headers {
                header.col(|ui| {
                    ui.label(RichText::new(*h).strong());
                });
            }
        })
        .body(|body| {
            body.rows(30.0, rows.len(), |mut row| {
                let s = rows[row.index()];
                row.set_selected(*st.selected == Some(s.id));
                row.col(|ui| {
                    let text = if s.name.is_empty() { "(no name)" } else { &s.name };
                    let mut rt = RichText::new(text).strong();
                    if s.archived {
                        rt = rt.color(theme::GREY);
                    }
                    ui.label(rt);
                });
                row.col(|ui| {
                    ui.label(&s.b_number);
                });
                match cat {
                    Category::Projects(_) => {
                        let f = s.fyp.as_ref().expect("filtered");
                        row.col(|ui| {
                            ui.label(&s.course);
                            if mixed {
                                theme::chip(
                                    ui,
                                    db.settings.programme_short(&f.programme),
                                    theme::ACCENT,
                                );
                            }
                        });
                        row.col(|ui| {
                            if f.initial_contact {
                                theme::chip(ui, "✔ contacted", theme::GREEN);
                            } else {
                                theme::chip(ui, "✖ not yet", theme::AMBER);
                            }
                        });
                        row.col(|ui| meetings_chip(ui, &f.meetings, &db.settings, today));
                        row.col(|ui| coursework_chip(ui, f, &db.settings, today));
                    }
                    Category::Placement => {
                        let p = s.placement.as_ref().expect("filtered");
                        row.col(|ui| {
                            ui.label(&p.company);
                        });
                        for v in &p.visits {
                            row.col(|ui| {
                                let (text, color) = visit_status(v);
                                theme::chip(ui, text, color);
                            });
                        }
                    }
                    Category::Advisee => {
                        let a = s.advisee.as_ref().expect("filtered");
                        row.col(|ui| {
                            ui.label(&s.course);
                        });
                        for c in [&a.s1, &a.s2] {
                            row.col(|ui| {
                                let (text, color) = semester_status(c, &db.settings, today);
                                theme::chip(ui, text, color);
                            });
                        }
                    }
                    Category::All => {
                        row.col(|ui| {
                            ui.label(&s.email);
                        });
                        row.col(|ui| {
                            ui.label(&s.course);
                        });
                        row.col(|ui| {
                            if let Some(f) = &s.fyp {
                                theme::chip(
                                    ui,
                                    db.settings.programme_short(&f.programme),
                                    theme::ACCENT,
                                );
                            }
                            if s.placement.is_some() {
                                theme::chip(ui, "Placement", theme::GREEN);
                            }
                            if s.advisee.is_some() {
                                theme::chip(ui, "Advisee", theme::AMBER);
                            }
                        });
                    }
                }
                if row.response().clicked() {
                    *st.selected = Some(s.id);
                }
            });
        });
    add
}

/// Next planned meeting on or after `today` and how many have been held,
/// counting each week of repeating meetings.
fn meeting_summary(
    meetings: &[Meeting],
    settings: &Settings,
    today: NaiveDate,
) -> (Option<NaiveDate>, usize) {
    let weeks: Vec<_> = meetings
        .iter()
        .flat_map(|m| m.occurrences(settings))
        .filter(|o| !o.cancelled)
        .collect();
    let next = weeks
        .iter()
        .filter(|o| !o.held && o.date() >= today)
        .map(|o| o.date())
        .min();
    (next, weeks.iter().filter(|o| o.held).count())
}

fn meetings_chip(ui: &mut Ui, meetings: &[Meeting], settings: &Settings, today: NaiveDate) {
    let (next, held) = meeting_summary(meetings, settings, today);
    match next {
        Some(d) => theme::chip(ui, format!("next {}", widgets::fmt_short(d)), theme::ACCENT),
        None => theme::chip(ui, "none booked", theme::GREY),
    };
    ui.label(RichText::new(format!("{held} held")).small().color(theme::GREY));
}

fn coursework_chip(ui: &mut Ui, f: &FypRecord, settings: &Settings, today: NaiveDate) {
    let mut total = 0;
    let mut done = 0;
    let mut overdue = 0;
    for module in settings.modules_for(&f.programme) {
        for cw in &module.courseworks {
            total += 1;
            let status = f.coursework.get(&cw.id);
            if status.is_some_and(|s| s.submitted) {
                done += 1;
            } else if settings
                .effective_deadline(module, cw, status)
                .is_some_and(|d| d < today)
            {
                overdue += 1;
            }
        }
    }
    if total == 0 {
        theme::chip(ui, "no CWs defined", theme::GREY);
        return;
    }
    let color = if overdue > 0 {
        theme::RED
    } else if done == total {
        theme::GREEN
    } else {
        theme::ACCENT
    };
    theme::chip(ui, format!("{done}/{total} submitted"), color);
    if overdue > 0 {
        theme::chip(ui, format!("{overdue} overdue"), theme::RED);
    }
}

pub fn visit_status(v: &Visit) -> (String, Color32) {
    if v.completed {
        return ("✔ done".into(), theme::GREEN);
    }
    if let Some(d) = v.date {
        return (format!("booked {}", widgets::fmt_short(d)), theme::ACCENT);
    }
    match (v.request_sent, v.response) {
        (None, _) => ("not requested".into(), theme::GREY),
        (Some(_), ReplyStatus::Replied) => ("replied – book date".into(), theme::ACCENT),
        (Some(_), ReplyStatus::Awaiting) => ("awaiting reply".into(), theme::AMBER),
        (Some(_), ReplyStatus::NoReply) => ("no reply".into(), theme::RED),
    }
}

pub fn semester_status(
    c: &SemesterContact,
    settings: &Settings,
    today: NaiveDate,
) -> (String, Color32) {
    let (next, held) = meeting_summary(&c.meetings, settings, today);
    if let Some(d) = next {
        return (format!("meeting {}", widgets::fmt_short(d)), theme::ACCENT);
    }
    if held > 0 {
        return (format!("✔ met ({held})"), theme::GREEN);
    }
    match (c.contacted_on, c.reply) {
        (None, _) => ("not contacted".into(), theme::GREY),
        (Some(_), ReplyStatus::Replied) => ("replied".into(), theme::ACCENT),
        (Some(_), ReplyStatus::Awaiting) => ("awaiting reply".into(), theme::AMBER),
        (Some(_), ReplyStatus::NoReply) => ("no reply".into(), theme::RED),
    }
}
