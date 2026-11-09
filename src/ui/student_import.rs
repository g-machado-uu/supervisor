//! Choosing which students of a spreadsheet to add: those with your email,
//! or your name, as their supervisor, academic visitor or mentor (or, for
//! marking, as second marker) are ticked. When neither is found, “Filter
//! by” picks the member of staff by hand.

use egui::{RichText, Ui};

use crate::model::{Database, MarkingKind};
use crate::sheet_import::{self, Choice, Existing, Sheet, SheetKind, SheetStudent};
use crate::ui::{icons, theme};

pub struct StudentImport {
    kind: SheetKind,
    file: String,
    rows: Vec<Row>,
    /// Courses a project student can be on.
    programmes: Vec<Course>,
    my_email: String,
    my_name: String,
    has_staff: bool,
    emails_filled: usize,
    merged: usize,
    /// The supervisors (or visitors) of the sheet: `name_key`, the name as
    /// the sheet first gives it, and how many rows.
    staff: Vec<(String, String, usize)>,
    filter: Filter,
    search: String,
}

/// Whose rows are shown.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Filter {
    /// The rows found to be yours.
    Mine,
    Everyone,
    /// One supervisor's rows (`name_key`), chosen by hand.
    Staff(String),
}

struct Row {
    student: SheetStudent,
    take: bool,
    mine: bool,
    /// `name_key` of each supervisor (or visitor) of the row.
    staff: Vec<String>,
    existing: Option<Existing>,
    /// The same student is on an earlier row.
    repeat: bool,
    programme: String,
}

impl Row {
    /// Nothing to add: in this list already with nothing to fill in, or a
    /// repeated row.
    fn locked(&self) -> bool {
        self.repeat || self.existing.as_ref().is_some_and(|e| e.nothing_to_do())
    }

    /// In this list already, with details to fill in.
    fn update(&self) -> bool {
        self.existing.as_ref().is_some_and(|e| e.has_role)
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
    /// its first course is the one suggested, unless a row's course names
    /// another. Students marked as second marker are never on SUST.
    pub fn new(kind: SheetKind, file: String, sheet: Sheet, db: &Database, list: Option<&str>) -> Self {
        let settings = &db.settings;
        let marking = kind == SheetKind::Marking(MarkingKind::Project);
        let programmes: Vec<Course> = settings
            .programmes
            .iter()
            .filter(|p| !(marking && settings.is_sust(&p.id)))
            .map(|p| (p.id.clone(), p.short.clone(), p.name.clone()))
            .collect();
        let ids: Vec<String> = programmes.iter().map(|(id, _, _)| id.clone()).collect();
        let suggested = settings
            .programmes
            .iter()
            .find(|p| Some(p.list_name()) == list)
            .map(|p| p.id.clone())
            .or_else(|| ids.first().cloned())
            .unwrap_or_default();
        let my_email = settings.my_email.trim().to_string();
        let my_name = settings.my_name.trim().to_string();
        let mut rows: Vec<Row> = Vec::with_capacity(sheet.students.len());
        let mut staff: Vec<(String, String, usize)> = Vec::new();
        let has_staff = sheet.has_person(kind);
        for student in sheet.students {
            let repeat = rows.iter().any(|r| sheet_import::same_student(&r.student, &student));
            let existing = sheet_import::existing(db, &student, kind);
            let mine = student.is_mine(kind, &my_email, &my_name);
            let programme = sheet_import::course_in(&student.course, settings, &ids).unwrap_or_else(|| suggested.clone());
            let mut keys = Vec::new();
            for name in student.staff(kind) {
                let key = sheet_import::name_key(&name);
                if keys.contains(&key) {
                    continue;
                }
                match staff.iter_mut().find(|(k, _, _)| *k == key) {
                    Some((_, _, n)) => *n += 1,
                    None => staff.push((key.clone(), name, 1)),
                }
                keys.push(key);
            }
            let mut row = Row { take: false, mine, staff: keys, existing, repeat, programme, student };
            row.take = mine && !row.locked();
            rows.push(row);
        }
        staff.sort_by_key(|(_, name, _)| name.to_lowercase());
        let any_mine = rows.iter().any(|r| r.mine);
        Self {
            kind,
            file,
            rows,
            programmes,
            my_email,
            my_name,
            has_staff,
            emails_filled: sheet.emails_filled,
            merged: sheet.merged,
            staff,
            filter: if any_mine { Filter::Mine } else { Filter::Everyone },
            search: String::new(),
        }
    }

    fn in_filter(&self, row: &Row) -> bool {
        match &self.filter {
            Filter::Mine => row.mine,
            Filter::Everyone => true,
            Filter::Staff(key) => row.staff.contains(key),
        }
    }

    fn visible(&self, row: &Row) -> bool {
        self.in_filter(row) && row.student.matches(&self.search)
    }

    /// Shows only `filter`'s rows. Choosing yours or one supervisor ticks
    /// their rows, and only theirs.
    fn set_filter(&mut self, filter: Filter) {
        if filter == self.filter {
            return;
        }
        self.filter = filter;
        if self.filter != Filter::Everyone {
            for i in 0..self.rows.len() {
                let take = self.in_filter(&self.rows[i]) && !self.rows[i].locked();
                self.rows[i].take = take;
            }
        }
    }

    fn filter_text(&self, filter: &Filter) -> String {
        match filter {
            Filter::Mine => format!("You ({})", self.rows.iter().filter(|r| r.mine).count()),
            Filter::Everyone => format!("Everyone ({})", self.rows.len()),
            Filter::Staff(key) => self
                .staff
                .iter()
                .find(|(k, _, _)| k == key)
                .map_or_else(String::new, |(_, name, n)| format!("{name} ({n})")),
        }
    }

    /// You, as Settings gives you: name and email.
    fn you(&self) -> String {
        [self.my_name.as_str(), self.my_email.as_str()].into_iter().filter(|t| !t.is_empty()).collect::<Vec<_>>().join(", ")
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
        if !self.has_staff {
            (format!("This sheet has no {staff} column, so nothing is ticked. Tick your students by hand."), true)
        } else if let Filter::Staff(_) = self.filter {
            (format!("The rows of the {staff} chosen under “Filter by” are ticked."), false)
        } else if self.my_email.is_empty() && self.my_name.is_empty() {
            (
                format!(
                    "Add your name and email in Settings › Invitations to have the rows with you as {staff} ticked. \
                     Until then, choose your name under “Filter by”, or tick your students by hand."
                ),
                true,
            )
        } else if mine == 0 {
            (
                format!(
                    "No row has you ({}) as the {staff}. Choose your name under “Filter by”, or tick your students \
                     by hand.",
                    self.you()
                ),
                true,
            )
        } else {
            let of = self.rows.len();
            (format!("{mine} of the {of} rows have you ({}) as the {staff}.", self.you()), false)
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
                        SheetKind::Advisees => "Import studies advisees",
                        SheetKind::Marking(MarkingKind::Project) => "Import projects you second-mark",
                        SheetKind::Marking(MarkingKind::Msc) => "Import MSc dissertations you second-mark",
                        SheetKind::Marking(MarkingKind::Placement) => "Import placement reports you mark",
                    };
                    ui.label(theme::serif(what, 22.0));
                    ui.label(RichText::new(&self.file).small().color(theme::muted(ui)));
                });
            });
            ui.add_space(6.0);
            let (explain, warn) = self.explanation();
            let color = if warn { theme::fg(ui, theme::AMBER) } else { theme::muted(ui) };
            ui.add(egui::Label::new(RichText::new(explain).color(color)).wrap());
            let mut about = "Students already in Supervisor are matched by their B number. Students are read from the \
                             first sheet"
                .to_string();
            if self.merged > 0 {
                about.push_str(&format!(
                    "; details missing there (such as poster numbers) were found for {} of them on the other sheets",
                    self.merged
                ));
            }
            if self.emails_filled > 0 {
                about.push_str(&format!("; {} missing emails were found by name on the other sheets", self.emails_filled));
            }
            about.push('.');
            ui.add(egui::Label::new(RichText::new(about).small().color(theme::muted(ui))).wrap());
            ui.add_space(8.0);
            let any_mine = self.rows.iter().any(|r| r.mine);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(icons::MAGNIFYING_GLASS).color(theme::muted(ui)));
                ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search").desired_width(200.0));
                if !self.staff.is_empty() {
                    ui.separator();
                    ui.label(format!("Filter by {}", self.kind.staff()));
                    let mut options = Vec::with_capacity(self.staff.len() + 2);
                    if any_mine {
                        options.push(Filter::Mine);
                    }
                    options.push(Filter::Everyone);
                    options.extend(self.staff.iter().map(|(key, _, _)| Filter::Staff(key.clone())));
                    let mut chosen = None;
                    egui::ComboBox::from_id_salt("filter-staff")
                        .selected_text(self.filter_text(&self.filter))
                        .width(220.0)
                        .height(360.0)
                        .show_ui(ui, |ui| {
                            for option in options {
                                let text = self.filter_text(&option);
                                if ui.selectable_label(self.filter == option, text).clicked() {
                                    chosen = Some(option);
                                }
                            }
                        })
                        .response
                        .on_hover_text(format!(
                            "Show one {}'s students, and tick them. “Everyone” shows every row and leaves the ticks as \
                             they are.",
                            self.kind.staff()
                        ));
                    if let Some(filter) = chosen {
                        self.set_filter(filter);
                    }
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
            let show_staff = self.filter == Filter::Everyone;
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
            let updates = self.rows.iter().filter(|r| r.take && !r.locked() && r.update()).count();
            let hidden = self.rows.iter().filter(|r| r.take && !r.locked() && !self.visible(r)).count();
            let already = self.rows.iter().filter(|r| r.locked() && !r.repeat).count();
            ui.horizontal(|ui| {
                let students = |n: usize| if n == 1 { "1 student".to_string() } else { format!("{n} students") };
                let mut summary = match (taken - updates, updates) {
                    (0, 0) => "Nothing ticked".to_string(),
                    (n, 0) => format!("{} to add", students(n)),
                    (0, u) => format!("{} to fill in", students(u)),
                    (n, u) => format!("{} to add, {u} to fill in", students(n)),
                };
                if hidden > 0 {
                    summary.push_str(&format!(" ({hidden} not shown)"));
                }
                if already > 0 {
                    summary.push_str(&format!(" · {already} already in this list"));
                }
                ui.label(RichText::new(summary).color(theme::muted(ui)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = match (taken, updates) {
                        (1, 1) => "Fill in 1 student".to_string(),
                        (n, u) if n == u => format!("Fill in {n} students"),
                        (1, _) => "Add 1 student".to_string(),
                        (n, _) => format!("Add {n} students"),
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
    let (existing, repeat) = (row.existing.clone(), row.repeat);
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
            SheetKind::Projects | SheetKind::Marking(MarkingKind::Project | MarkingKind::Msc) => &s.project_title,
            SheetKind::Placement | SheetKind::Marking(MarkingKind::Placement) => &s.company,
            SheetKind::Advisees => &s.course,
        };
        let staff = s.staff(kind).join(" / ");
        // The same room on every row, so that the columns line up.
        let course_picker = kind.has_course() && programmes.len() > 1;
        let staff_width = if show_staff { 150.0 } else { 0.0 };
        let right = if course_picker { 250.0 } else { 140.0 };
        let detail_width = (ui.available_width() - right - staff_width).max(100.0);
        let hover = (!detail.is_empty()).then(|| detail.clone());
        cell(ui, detail_width, detail, color, hover);
        if show_staff {
            cell(ui, staff_width, &staff, theme::muted(ui), (!staff.is_empty()).then(|| staff.clone()));
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
                SheetKind::Advisees => "the advisee role",
                SheetKind::Marking(_) => "the marking",
            };
            if repeat {
                theme::chip(ui, "Repeated row", theme::GREY).on_hover_text("This student is on an earlier row of the sheet");
            } else if let Some(e) = existing {
                if e.nothing_to_do() {
                    theme::chip(ui, "Already in this list", theme::GREY);
                } else if e.has_role {
                    theme::chip(ui, "Fills in details", theme::ACCENT).on_hover_text(format!(
                        "Already in this list. Ticked, the sheet fills in what is still empty: {}",
                        e.fills.join(", ")
                    ));
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
