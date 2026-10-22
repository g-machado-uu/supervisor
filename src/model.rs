//! Data model: students, their roles (FYP, placement, advisee) and settings.

use std::collections::HashMap;

use chrono::{Duration, NaiveDate, NaiveTime};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const DB_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum FypKind {
    #[default]
    BEng,
    MEng,
    /// MEng with a semester in Germany (EEE802).
    MEngGermany,
    /// SUST (China) students, co-supervised from here.
    Sust,
}

impl FypKind {
    pub const ALL: [FypKind; 4] = [
        FypKind::BEng,
        FypKind::MEng,
        FypKind::MEngGermany,
        FypKind::Sust,
    ];

    pub fn label(self) -> &'static str {
        match self {
            FypKind::BEng => "BEng (MEC526 + MEC528)",
            FypKind::MEng => "MEng (EEE840)",
            FypKind::MEngGermany => "MEng – Germany (EEE802)",
            FypKind::Sust => "SUST (MEC526 + MEC528, China)",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            FypKind::BEng => "BEng",
            FypKind::MEng => "MEng",
            FypKind::MEngGermany => "MEng (DE)",
            FypKind::Sust => "SUST",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Semester {
    #[default]
    S1,
    S2,
    FullYear,
}

impl Semester {
    pub const ALL: [Semester; 3] = [Semester::S1, Semester::S2, Semester::FullYear];

    pub fn label(self) -> &'static str {
        match self {
            Semester::S1 => "Semester 1",
            Semester::S2 => "Semester 2",
            Semester::FullYear => "Full year",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TriState {
    #[default]
    Unknown,
    Yes,
    No,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ReplyStatus {
    /// Not contacted yet, or contacted and waiting.
    #[default]
    Awaiting,
    Replied,
    /// The student ignored the message.
    NoReply,
}

impl ReplyStatus {
    pub fn label(self) -> &'static str {
        match self {
            ReplyStatus::Awaiting => "Awaiting reply",
            ReplyStatus::Replied => "Replied",
            ReplyStatus::NoReply => "No reply",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum VisitMode {
    #[default]
    Online,
    FaceToFace,
}

impl VisitMode {
    pub fn label(self) -> &'static str {
        match self {
            VisitMode::Online => "Online",
            VisitMode::FaceToFace => "Face-to-face",
        }
    }
}

// ---------------------------------------------------------------------------
// Settings & modules
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct CourseworkTemplate {
    pub id: Uuid,
    pub title: String,
    /// Teaching week the coursework is due (relative to the semester start).
    pub week: Option<u8>,
    /// An explicit deadline; takes precedence over `week`.
    pub deadline: Option<NaiveDate>,
}

impl Default for CourseworkTemplate {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            title: "New coursework".into(),
            week: None,
            deadline: None,
        }
    }
}

impl CourseworkTemplate {
    /// The explicit date, or else the start of teaching `week` counted from
    /// the semester's week-1 start date.
    pub fn resolve_deadline(&self, semester_start: Option<NaiveDate>) -> Option<NaiveDate> {
        self.deadline.or_else(|| {
            let week = self.week?;
            Some(semester_start? + Duration::weeks(i64::from(week.max(1)) - 1))
        })
    }

    fn new(title: &str, week: Option<u8>) -> Self {
        Self {
            title: title.into(),
            week,
            ..Default::default()
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Module {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub semester: Semester,
    pub applies_to: Vec<FypKind>,
    pub courseworks: Vec<CourseworkTemplate>,
}

impl Default for Module {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            code: "NEW".into(),
            name: "New module".into(),
            semester: Semester::S1,
            applies_to: Vec::new(),
            courseworks: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Settings {
    pub academic_year: String,
    pub sem1_start: Option<NaiveDate>,
    pub sem2_start: Option<NaiveDate>,
    pub modules: Vec<Module>,
    pub dark_mode: bool,
    pub upcoming_days: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            academic_year: "2026/27".into(),
            sem1_start: None,
            sem2_start: None,
            modules: default_modules(),
            dark_mode: false,
            upcoming_days: 14,
        }
    }
}

pub fn default_modules() -> Vec<Module> {
    let module = |code: &str, name: &str, semester, kinds: &[FypKind], cws| Module {
        code: code.into(),
        name: name.into(),
        semester,
        applies_to: kinds.to_vec(),
        courseworks: cws,
        ..Default::default()
    };
    vec![
        module(
            "MEC526",
            "Research Methods and Management",
            Semester::S1,
            &[FypKind::BEng],
            vec![
                CourseworkTemplate::new("Coursework 1", Some(8)),
                CourseworkTemplate::new("Coursework 2", Some(12)),
            ],
        ),
        module(
            "MEC528",
            "BEng Hons Final Year Project",
            Semester::S2,
            &[FypKind::BEng],
            vec![
                CourseworkTemplate::new("Poster", None),
                CourseworkTemplate::new("Coursework 2", None),
            ],
        ),
        module(
            "MEC526",
            "Research Methods and Management (SUST)",
            Semester::S1,
            &[FypKind::Sust],
            Vec::new(),
        ),
        module(
            "MEC528",
            "BEng Hons Final Year Project (SUST)",
            Semester::S2,
            &[FypKind::Sust],
            Vec::new(),
        ),
        module(
            "EEE840",
            "MEng Final Year Dissertation",
            Semester::FullYear,
            &[FypKind::MEng],
            Vec::new(),
        ),
        module(
            "EEE802",
            "MEng Final Year Dissertation",
            Semester::FullYear,
            &[FypKind::MEngGermany],
            Vec::new(),
        ),
    ]
}

impl Settings {
    pub fn semester_start(&self, semester: Semester) -> Option<NaiveDate> {
        match semester {
            Semester::S1 | Semester::FullYear => self.sem1_start,
            Semester::S2 => self.sem2_start,
        }
    }

    /// Deadline from the template alone (no per-student extension).
    pub fn template_deadline(&self, module: &Module, cw: &CourseworkTemplate) -> Option<NaiveDate> {
        cw.resolve_deadline(self.semester_start(module.semester))
    }

    /// Deadline for one student, honouring a per-student override (extension).
    pub fn effective_deadline(
        &self,
        module: &Module,
        cw: &CourseworkTemplate,
        status: Option<&CwStatus>,
    ) -> Option<NaiveDate> {
        status
            .and_then(|s| s.deadline_override)
            .or_else(|| self.template_deadline(module, cw))
    }

    pub fn modules_for(&self, kind: FypKind) -> impl Iterator<Item = &Module> {
        self.modules
            .iter()
            .filter(move |m| m.applies_to.contains(&kind))
    }
}

// ---------------------------------------------------------------------------
// Student records
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Meeting {
    pub id: Uuid,
    pub date: Option<NaiveDate>,
    pub time: Option<NaiveTime>,
    pub held: bool,
    pub notes: String,
}

impl Default for Meeting {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            date: None,
            time: None,
            held: false,
            notes: String::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(default)]
pub struct CwStatus {
    pub submitted: bool,
    pub submitted_on: Option<NaiveDate>,
    pub deadline_override: Option<NaiveDate>,
    pub notes: String,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(default)]
pub struct FypRecord {
    pub kind: FypKind,
    pub project_title: String,
    pub initial_contact: bool,
    pub initial_contact_on: Option<NaiveDate>,
    pub meetings: Vec<Meeting>,
    /// Keyed by `CourseworkTemplate::id`.
    pub coursework: HashMap<Uuid, CwStatus>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(default)]
pub struct Visit {
    pub mode: VisitMode,
    pub request_sent: Option<NaiveDate>,
    pub response: ReplyStatus,
    pub responded_on: Option<NaiveDate>,
    pub date: Option<NaiveDate>,
    pub time: Option<NaiveTime>,
    pub line_manager_attending: TriState,
    pub completed: bool,
    pub notes: String,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct PlacementRecord {
    pub company: String,
    pub professional_email: String,
    pub line_manager_name: String,
    pub line_manager_email: String,
    pub visits: [Visit; 2],
}

impl Default for PlacementRecord {
    fn default() -> Self {
        Self {
            company: String::new(),
            professional_email: String::new(),
            line_manager_name: String::new(),
            line_manager_email: String::new(),
            visits: [
                Visit::default(),
                Visit {
                    mode: VisitMode::FaceToFace,
                    ..Default::default()
                },
            ],
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(default)]
pub struct SemesterContact {
    pub contacted_on: Option<NaiveDate>,
    pub reply: ReplyStatus,
    pub replied_on: Option<NaiveDate>,
    /// First entry is the semester meeting; the rest are follow-ons.
    pub meetings: Vec<Meeting>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(default)]
pub struct AdviseeRecord {
    pub s1: SemesterContact,
    pub s2: SemesterContact,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Student {
    pub id: Uuid,
    pub b_number: String,
    pub name: String,
    pub email: String,
    pub course: String,
    pub archived: bool,
    pub notes: String,
    pub fyp: Option<FypRecord>,
    pub placement: Option<PlacementRecord>,
    pub advisee: Option<AdviseeRecord>,
}

impl Default for Student {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            b_number: String::new(),
            name: String::new(),
            email: String::new(),
            course: String::new(),
            archived: false,
            notes: String::new(),
            fyp: None,
            placement: None,
            advisee: None,
        }
    }
}

impl Student {
    pub fn display_name(&self) -> String {
        match (self.name.is_empty(), self.b_number.is_empty()) {
            (false, false) => format!("{} ({})", self.name, self.b_number),
            (false, true) => self.name.clone(),
            (true, false) => self.b_number.clone(),
            (true, true) => "(unnamed student)".into(),
        }
    }

    pub fn matches(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        let q = query.to_lowercase();
        let company = self.placement.as_ref().map(|p| p.company.as_str());
        [
            Some(self.name.as_str()),
            Some(self.b_number.as_str()),
            Some(self.email.as_str()),
            Some(self.course.as_str()),
            company,
        ]
        .into_iter()
        .flatten()
        .any(|field| field.to_lowercase().contains(&q))
    }
}

// ---------------------------------------------------------------------------
// Database and derived views
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Database {
    pub version: u32,
    pub settings: Settings,
    pub students: Vec<Student>,
}

impl Default for Database {
    fn default() -> Self {
        Self {
            version: DB_VERSION,
            settings: Settings::default(),
            students: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EventKind {
    FypMeeting,
    Visit,
    AdviseeMeeting,
    Deadline,
}

/// A dated item derived from the records, used for the dashboard and .ics export.
#[derive(Clone, Debug)]
pub struct Event {
    pub uid: String,
    pub student_id: Uuid,
    pub date: NaiveDate,
    pub time: Option<NaiveTime>,
    pub kind: EventKind,
    pub title: String,
    pub done: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Severity {
    Overdue,
    Pending,
}

#[derive(Clone, Debug)]
pub struct Attention {
    pub student_id: Uuid,
    pub severity: Severity,
    pub message: String,
}

impl Database {
    pub fn student(&self, id: Uuid) -> Option<&Student> {
        self.students.iter().find(|s| s.id == id)
    }

    pub fn student_mut(&mut self, id: Uuid) -> Option<&mut Student> {
        self.students.iter_mut().find(|s| s.id == id)
    }

    pub fn b_number_taken(&self, b_number: &str, except: Option<Uuid>) -> bool {
        let b = b_number.trim();
        !b.is_empty()
            && self
                .students
                .iter()
                .any(|s| Some(s.id) != except && s.b_number.trim().eq_ignore_ascii_case(b))
    }

    /// Every dated item across all active (non-archived) students.
    pub fn events(&self) -> Vec<Event> {
        let mut out = Vec::new();
        for s in self.students.iter().filter(|s| !s.archived) {
            let name = s.display_name();
            if let Some(fyp) = &s.fyp {
                for m in &fyp.meetings {
                    if let Some(date) = m.date {
                        out.push(Event {
                            uid: format!("meeting-{}", m.id),
                            student_id: s.id,
                            date,
                            time: m.time,
                            kind: EventKind::FypMeeting,
                            title: format!("FYP meeting – {name}"),
                            done: m.held,
                        });
                    }
                }
                for module in self.settings.modules_for(fyp.kind) {
                    for cw in &module.courseworks {
                        let status = fyp.coursework.get(&cw.id);
                        if let Some(date) = self.settings.effective_deadline(module, cw, status) {
                            out.push(Event {
                                uid: format!("cw-{}-{}", s.id, cw.id),
                                student_id: s.id,
                                date,
                                time: None,
                                kind: EventKind::Deadline,
                                title: format!("{} {} due – {name}", module.code, cw.title),
                                done: status.is_some_and(|st| st.submitted),
                            });
                        }
                    }
                }
            }
            if let Some(p) = &s.placement {
                for (i, v) in p.visits.iter().enumerate() {
                    if let Some(date) = v.date {
                        out.push(Event {
                            uid: format!("visit{}-{}", i + 1, s.id),
                            student_id: s.id,
                            date,
                            time: v.time,
                            kind: EventKind::Visit,
                            title: format!(
                                "Placement visit {} ({}) – {name} @ {}",
                                i + 1,
                                v.mode.label(),
                                p.company
                            ),
                            done: v.completed,
                        });
                    }
                }
            }
            if let Some(a) = &s.advisee {
                for (sem, contact) in [("S1", &a.s1), ("S2", &a.s2)] {
                    for (i, m) in contact.meetings.iter().enumerate() {
                        if let Some(date) = m.date {
                            let label = if i == 0 { "meeting" } else { "follow-on" };
                            out.push(Event {
                                uid: format!("meeting-{}", m.id),
                                student_id: s.id,
                                date,
                                time: m.time,
                                kind: EventKind::AdviseeMeeting,
                                title: format!("Advisee {sem} {label} – {name}"),
                                done: m.held,
                            });
                        }
                    }
                }
            }
        }
        out.sort_by_key(|e| (e.date, e.time));
        out
    }

    pub fn upcoming(&self, today: NaiveDate, days: u32) -> Vec<Event> {
        let end = today + Duration::days(i64::from(days));
        self.events()
            .into_iter()
            .filter(|e| !e.done && e.date >= today && e.date <= end)
            .collect()
    }

    /// Things that need chasing, most urgent first.
    pub fn attention(&self, today: NaiveDate) -> Vec<Attention> {
        let mut out = Vec::new();
        let mut push = |student: &Student, severity, message: String| {
            out.push(Attention {
                student_id: student.id,
                severity,
                message: format!("{} – {message}", student.display_name()),
            })
        };
        for s in self.students.iter().filter(|s| !s.archived) {
            if let Some(fyp) = &s.fyp {
                if !fyp.initial_contact {
                    push(s, Severity::Pending, "no initial contact yet".into());
                }
                for module in self.settings.modules_for(fyp.kind) {
                    for cw in &module.courseworks {
                        let status = fyp.coursework.get(&cw.id);
                        let submitted = status.is_some_and(|st| st.submitted);
                        if let Some(due) = self.settings.effective_deadline(module, cw, status)
                            && !submitted
                            && due < today
                        {
                            push(
                                s,
                                Severity::Overdue,
                                format!("{} {} not submitted (due {due})", module.code, cw.title),
                            );
                        }
                    }
                }
            }
            if let Some(p) = &s.placement {
                for (i, v) in p.visits.iter().enumerate() {
                    if v.completed {
                        continue;
                    }
                    if v.request_sent.is_some() && v.response != ReplyStatus::Replied {
                        let severity = if v.response == ReplyStatus::NoReply {
                            Severity::Overdue
                        } else {
                            Severity::Pending
                        };
                        push(s, severity, format!("visit {} request: {}", i + 1, v.response.label().to_lowercase()));
                    }
                    if v.date.is_some() && v.line_manager_attending == TriState::Unknown {
                        push(s, Severity::Pending, format!("visit {}: line manager attendance unknown", i + 1));
                    }
                }
            }
            if let Some(a) = &s.advisee {
                for (sem, c) in [("S1", &a.s1), ("S2", &a.s2)] {
                    if c.contacted_on.is_some() && c.reply != ReplyStatus::Replied {
                        let severity = if c.reply == ReplyStatus::NoReply {
                            Severity::Overdue
                        } else {
                            Severity::Pending
                        };
                        push(s, severity, format!("advisee {sem}: {}", c.reply.label().to_lowercase()));
                    }
                }
            }
        }
        out.sort_by_key(|a| a.severity);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn deadline_from_week_and_override() {
        let mut settings = Settings {
            sem1_start: Some(d(2026, 9, 21)),
            ..Default::default()
        };
        let module = settings.modules[0].clone();
        let cw = &module.courseworks[0]; // Week 8
        assert_eq!(settings.template_deadline(&module, cw), Some(d(2026, 11, 9)));

        let status = CwStatus {
            deadline_override: Some(d(2026, 11, 20)),
            ..Default::default()
        };
        assert_eq!(
            settings.effective_deadline(&module, cw, Some(&status)),
            Some(d(2026, 11, 20))
        );

        settings.sem1_start = None;
        assert_eq!(settings.template_deadline(&module, cw), None);
    }

    #[test]
    fn modules_per_kind() {
        let s = Settings::default();
        let codes = |k| s.modules_for(k).map(|m| m.code.clone()).collect::<Vec<_>>();
        assert_eq!(codes(FypKind::BEng), ["MEC526", "MEC528"]);
        assert_eq!(codes(FypKind::Sust), ["MEC526", "MEC528"]);
        assert_eq!(codes(FypKind::MEng), ["EEE840"]);
        assert_eq!(codes(FypKind::MEngGermany), ["EEE802"]);
    }

    #[test]
    fn serde_round_trip() {
        let mut db = Database::default();
        let mut s = Student {
            name: "Ada".into(),
            fyp: Some(FypRecord::default()),
            placement: Some(PlacementRecord::default()),
            advisee: Some(AdviseeRecord::default()),
            ..Default::default()
        };
        s.fyp.as_mut().unwrap().meetings.push(Meeting {
            date: Some(d(2026, 10, 1)),
            ..Default::default()
        });
        db.students.push(s);
        let json = serde_json::to_string(&db).unwrap();
        let back: Database = serde_json::from_str(&json).unwrap();
        assert_eq!(db, back);
    }

    #[test]
    fn attention_flags_no_reply_and_missing_contact() {
        let mut db = Database::default();
        db.students.push(Student {
            name: "Bob".into(),
            fyp: Some(FypRecord::default()),
            advisee: Some(AdviseeRecord {
                s1: SemesterContact {
                    contacted_on: Some(d(2026, 9, 25)),
                    reply: ReplyStatus::NoReply,
                    ..Default::default()
                },
                ..Default::default()
            }),
            ..Default::default()
        });
        let items = db.attention(d(2026, 10, 21));
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].severity, Severity::Overdue);
    }
}
