//! Choosing which students of a spreadsheet to add: those with your email
//! as their supervisor or academic visitor are ticked.

use egui::{RichText, Ui};

use crate::model::Database;
use crate::sheet_import::{self, Choice, Existing, Sheet, SheetKind, SheetStudent};
use crate::ui::{icons, theme};

pub struct StudentImport {
    kind: SheetKind,
    file: String,
    rows: Vec<Row>,
    /// Courses a project student can be on.
    programmes: Vec<Course>,
    my_email: String,
    has_staff_email: bool,
    /// Rows of other members of staff are shown too.
    show_all: bool,
    search: String,
}

struct Row {
    student: SheetStudent,
    take: bool,
    mine: bool,
    existing: Option<Existing>,
    /// The same student is on an earlier row.
    repeat: bool,
    programme: String,
}

impl Row {
    /// Nothing to add: in this list already, or a repeated row.
    fn locked(&self) -> bool {
        self.repeat || self.existing.is_some_and(|e| e.has_role)
    }
}

/// A course to choose: `Programme::id`, short name and full name.
type Course = (String, String, String);

pub enum Outcome {
    Open,
    Closed,
    Add(Vec<Choice>),
}

impl StudentImport {
    /// `list` is the sidebar list of project students it was opened from;
    /// its first course is the one suggested.
    pub fn new(kind: SheetKind, file: String, sheet: Sheet, db: &Database, list: Option<&str>) -> Self {
        let settings = &db.settings;
        let programmes: Vec<Course> = settings
            .programmes
            .iter()
            .map(|p| (p.id.clone(), p.short.clone(), p.name.clone()))
            .collect();
        let suggested = settings
            .programmes
            .iter()
            .find(|p| Some(p.list_name()) == list)
            .map(|p| p.id.clone())
            .or_else(|| programmes.first().map(|(id, _, _)| id.clone()))
            .unwrap_or_default();
        let my_email = settings.my_email.trim().to_string();
        let mut rows: Vec<Row> = Vec::with_capacity(sheet.students.len());
        for student in sheet.students {
            let repeat = rows.iter().any(|r| sheet_import::same_student(&r.student, &student));
            let existing = sheet_import::existing(db, &student, kind);
            let mine = student.is_mine(&my_email);
            let mut row = Row { take: false, mine, existing, repeat, programme: suggested.clone(), student };
            row.take = mine && !row.locked();
            rows.push(row);
        }
        let any_mine = rows.iter().any(|r| r.mine);
        Self {
            kind,
            file,
            rows,
            programmes,
            my_email,
            has_staff_email: sheet.has_staff_email,
            show_all: !any_mine,
            search: String::new(),
        }
    }

    fn visible(&self, row: &Row) -> bool {
        (self.show_all || row.mine) && row.student.matches(&self.search)
    }

    fn choices(&self) -> Vec<Choice> {
        self.rows
            .iter()
            .filter(|r| r.take && !r.locked())
            .map(|r| Choice { student: r.student.clone(), programme: r.programme.clone() })
            .collect()
    }

    /// Why nothing was ticked, or how many rows are yours.
    fn explanation(&self) -> (String, bool) {
        let staff = self.kind.staff();
        let mine = self.rows.iter().filter(|r| r.mine).count();
        if !self.has_staff_email {
            (format!("This sheet has no {staff} email column, so nothing is ticked. Tick your students by hand."), true)
        } else if self.my_email.is_empty() {
            (
                format!(
                    "Add your email in Settings › Invitations to have the rows with you as {staff} ticked. \
                     Until then, tick your students by hand."
                ),
                true,
            )
        } else if mine == 0 {
            (format!("No row has {} as the {staff}. Tick your students by hand.", self.my_email), true)
        } else {
            let of = self.rows.len();
            (format!("{mine} of the {of} rows have {} as the {staff}.", self.my_email), false)
        }
    }

    pub fn show(&mut self, ui: &mut Ui) -> Outcome {
        let mut add = false;
        let mut cancel = false;
        let screen = ui.ctx().content_rect();
        let resp = egui::Modal::new(egui::Id::unique("import-students")).frame(theme::modal_frame(ui)).show(ui.ctx(), |ui| {
            ui.set_width((screen.width() - 120.0).clamp(560.0, 940.0));
            ui.horizontal(|ui| {
                theme::icon_badge(ui, icons::DOWNLOAD_SIMPLE, theme::ACCENT, 40.0);
                ui.vertical(|ui| {
                    let what = match self.kind {
                        SheetKind::Projects => "Import project students",
                        SheetKind::Placement => "Import placement students",
                    };
                    ui.label(theme::serif(what, 22.0));
                    ui.label(RichText::new(&self.file).small().color(theme::muted(ui)));
                });
            });
            ui.add_space(6.0);
            let (explain, warn) = self.explanation();
            let color = if warn { theme::fg(ui, theme::AMBER) } else { theme::muted(ui) };
            ui.add(egui::Label::new(RichText::new(explain).color(color)).wrap());
            ui.add(
                egui::Label::new(
                    RichText::new(
                        "Students already in Supervisor are matched by their B number. Only the first sheet of the \
                         file is read.",
                    )
                    .small()
                    .color(theme::muted(ui)),
                )
                .wrap(),
            );
            ui.add_space(8.0);
            let others = self.rows.iter().filter(|r| !r.mine).count();
            let any_mine = self.rows.iter().any(|r| r.mine);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(icons::MAGNIFYING_GLASS).color(theme::muted(ui)));
                ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search").desired_width(200.0));
                if any_mine && others > 0 {
                    ui.checkbox(&mut self.show_all, format!("Show other staff's students ({others})"));
                }
                ui.separator();
                let shown: Vec<usize> = (0..self.rows.len()).filter(|&i| self.visible(&self.rows[i])).collect();
                if ui.button("Tick all shown").on_hover_text("Students already in this list stay unticked").clicked() {
                    for &i in &shown {
                        let row = &mut self.rows[i];
                        row.take = !row.locked();
                    }
                }
                if ui.button("Untick all shown").clicked() {
                    for &i in &shown {
                        self.rows[i].take = false;
                    }
                }
            });
            ui.add_space(6.0);

            let shown: Vec<usize> = (0..self.rows.len()).filter(|&i| self.visible(&self.rows[i])).collect();
            let row_height = 30.0;
            let list_height = (screen.height() - 380.0).clamp(160.0, 520.0);
            let kind = self.kind;
            let show_staff = self.show_all;
            egui::Frame::NONE
                .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
                .corner_radius(egui::CornerRadius::same(10))
                .inner_margin(egui::Margin::same(8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if shown.is_empty() {
                        ui.label(RichText::new("No students to show.").color(theme::muted(ui)));
                        return;
                    }
                    egui::ScrollArea::vertical()
                        .max_height(list_height)
                        .auto_shrink([false, true])
                        .show_rows(ui, row_height, shown.len(), |ui, range| {
                            for &i in &shown[range] {
                                ui.push_id(i, |ui| {
                                    row_ui(ui, kind, &mut self.rows[i], &self.programmes, show_staff, row_height)
                                });
                            }
                        });
                });

            ui.add_space(10.0);
            let taken = self.rows.iter().filter(|r| r.take && !r.locked()).count();
            let hidden = self.rows.iter().filter(|r| r.take && !r.locked() && !self.visible(r)).count();
            let already = self.rows.iter().filter(|r| r.existing.is_some_and(|e| e.has_role) && !r.repeat).count();
            ui.horizontal(|ui| {
                let mut summary = match taken {
                    0 => "Nothing ticked".to_string(),
                    1 => "1 student to add".to_string(),
                    n => format!("{n} students to add"),
                };
                if hidden > 0 {
                    summary.push_str(&format!(" ({hidden} not shown)"));
                }
                if already > 0 {
                    summary.push_str(&format!(" · {already} already in this list"));
                }
                ui.label(RichText::new(summary).color(theme::muted(ui)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = match taken {
                        1 => "Add 1 student".to_string(),
                        n => format!("Add {n} students"),
                    };
                    if ui.add_enabled(taken > 0, theme::primary_button(label)).clicked() {
                        add = true;
                    }
                    if ui.add(egui::Button::new("Cancel").min_size(egui::vec2(0.0, 34.0))).clicked() {
                        cancel = true;
                    }
                });
            });
        });
        if add {
            Outcome::Add(self.choices())
        } else if cancel || resp.should_close() {
            Outcome::Closed
        } else {
            Outcome::Open
        }
    }
}

/// One student: tick, B number, name, project title or company, and what
/// adding them does.
fn row_ui(ui: &mut Ui, kind: SheetKind, row: &mut Row, programmes: &[Course], show_staff: bool, height: f32) {
    let locked = row.locked();
    let (existing, repeat) = (row.existing, row.repeat);
    let Row { student: s, take, programme, .. } = row;
    ui.horizontal(|ui| {
        ui.set_height(height - 4.0);
        ui.add_enabled(!locked, egui::Checkbox::without_text(take));
        let color = if locked { theme::muted(ui) } else { ui.visuals().text_color() };
        let left = egui::Layout::left_to_right(egui::Align::Center);
        let cell = |ui: &mut Ui, width: f32, text: &str, color: egui::Color32, hover: Option<String>| {
            ui.allocate_ui_with_layout(egui::vec2(width, 20.0), left, |ui| {
                ui.set_min_width(width);
                ui.set_max_width(width);
                let label = ui.add(egui::Label::new(RichText::new(text).color(color)).truncate());
                if let Some(hover) = hover {
                    label.on_hover_text(hover);
                }
            });
        };
        cell(ui, 100.0, &s.b_number, color, None);
        let name = if s.name.is_empty() { "(no name)" } else { &s.name };
        let mut about = vec![s.name.clone()];
        about.extend([&s.email, &s.course].into_iter().filter(|t| !t.is_empty()).cloned());
        about.extend(s.details());
        cell(ui, 170.0, name, color, Some(about.join("\n")));
        let detail = match kind {
            SheetKind::Projects => &s.project_title,
            SheetKind::Placement => &s.company,
        };
        let staff = match (s.staff_name.is_empty(), s.staff_email.is_empty()) {
            (_, true) => s.staff_name.clone(),
            (true, false) => s.staff_email.clone(),
            (false, false) => format!("{} ({})", s.staff_name, s.staff_email),
        };
        // The same room on every row, so that the columns line up.
        let course_picker = kind == SheetKind::Projects && programmes.len() > 1;
        let staff_width = if show_staff { 150.0 } else { 0.0 };
        let right = if course_picker { 250.0 } else { 140.0 };
        let detail_width = (ui.available_width() - right - staff_width).max(100.0);
        let hover = (!detail.is_empty()).then(|| detail.clone());
        cell(ui, detail_width, detail, color, hover);
        if show_staff {
            let shown = if s.staff_name.is_empty() { &s.staff_email } else { &s.staff_name };
            cell(ui, staff_width, shown, theme::muted(ui), (!staff.is_empty()).then_some(staff));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if course_picker && !locked {
                let (short, name) =
                    programmes.iter().find(|(id, _, _)| id == programme).map_or(("", ""), |(_, s, n)| (s, n));
                egui::ComboBox::from_id_salt("course")
                    .selected_text(short)
                    .width(100.0)
                    .show_ui(ui, |ui| {
                        for (id, _, name) in programmes {
                            ui.selectable_value(programme, id.clone(), name);
                        }
                    })
                    .response
                    .on_hover_text(name);
            } else if course_picker {
                ui.add_space(108.0);
            }
            let role = match kind {
                SheetKind::Projects => "the project",
                SheetKind::Placement => "the placement",
            };
            if repeat {
                theme::chip(ui, "Repeated row", theme::GREY).on_hover_text("This student is on an earlier row of the sheet");
            } else if let Some(e) = existing {
                if e.has_role {
                    theme::chip(ui, "Already in this list", theme::GREY);
                } else if e.archived {
                    theme::chip(ui, "Archived student", theme::AMBER)
                        .on_hover_text(format!("Adding {role} brings this student back from the archive"));
                } else {
                    theme::chip(ui, "In another list", theme::ACCENT)
                        .on_hover_text(format!("Already in Supervisor: {role} is added to the same student"));
                }
            }
        });
    });
}
