//! Data model: students, their roles (project, placement, advisee) and settings.

use std::collections::{HashMap, HashSet};

use chrono::{Datelike, Duration, NaiveDate, NaiveTime};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const DB_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

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
    /// `Programme::id`s this module applies to.
    pub applies_to: Vec<String>,
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

/// A course whose project students are supervised (BEng, MSc, …).
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Programme {
    /// Key stored on students and modules. The built-in courses keep the
    /// names of the old fixed list, so older data files load unchanged.
    pub id: String,
    pub name: String,
    pub short: String,
    /// Sidebar list it is shown in; courses sharing a list appear together.
    pub list: String,
}

impl Default for Programme {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name: "New course".into(),
            short: "New".into(),
            list: "Other".into(),
        }
    }
}

impl Programme {
    /// The sidebar list, "Other" when left blank.
    pub fn list_name(&self) -> &str {
        match self.list.trim() {
            "" => "Other",
            list => list,
        }
    }
}

pub fn default_programmes() -> Vec<Programme> {
    let p = |id: &str, name: &str, short: &str, list: &str| Programme {
        id: id.into(),
        name: name.into(),
        short: short.into(),
        list: list.into(),
    };
    vec![
        p("BEng", "BEng (MEC526 + MEC528)", "BEng", "BEng"),
        p("MEng", "MEng (EEE840)", "MEng", "MEng"),
        p(
            "MEngGermany",
            "MEng – Germany (EEE802)",
            "MEng (DE)",
            "MEng",
        ),
        p("Sust", "SUST (MEC526 + MEC528, China)", "SUST", "SUST"),
        p("MScElectronics", "MSc Electronics", "MSc Elec", "MSc"),
        p("MScMechatronics", "MSc Mechatronics", "MSc Mech", "MSc"),
    ]
}

/// Teaching weeks per semester when no end of teaching is set.
pub const TEACHING_WEEKS: i64 = 12;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Settings {
    pub academic_year: String,
    pub sem1_start: Option<NaiveDate>,
    pub sem2_start: Option<NaiveDate>,
    /// Last day of teaching (exams follow); defaults to Friday of week 12.
    pub sem1_teaching_end: Option<NaiveDate>,
    pub sem2_teaching_end: Option<NaiveDate>,
    pub programmes: Vec<Programme>,
    pub modules: Vec<Module>,
    pub dark_mode: bool,
    pub upcoming_days: u32,
    /// Organiser of invite files. Kept in the encrypted file like everything else.
    pub my_name: String,
    pub my_email: String,
    /// Optional reusable Teams meeting link, put into invites for Teams meetings.
    pub teams_link: String,
    /// Calendar exports include you as organiser and the students as guests.
    pub export_with_guests: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            academic_year: "2026/27".into(),
            sem1_start: None,
            sem2_start: None,
            sem1_teaching_end: None,
            sem2_teaching_end: None,
            programmes: default_programmes(),
            modules: default_modules(),
            dark_mode: false,
            upcoming_days: 14,
            my_name: String::new(),
            my_email: String::new(),
            teams_link: String::new(),
            export_with_guests: true,
        }
    }
}

pub fn default_modules() -> Vec<Module> {
    let module = |code: &str, name: &str, semester, programmes: &[&str], cws| Module {
        code: code.into(),
        name: name.into(),
        semester,
        applies_to: programmes.iter().map(|p| p.to_string()).collect(),
        courseworks: cws,
        ..Default::default()
    };
    vec![
        module(
            "MEC526",
            "Research Methods and Management",
            Semester::S1,
            &["BEng"],
            vec![
                CourseworkTemplate::new("Coursework 1", Some(8)),
                CourseworkTemplate::new("Coursework 2", Some(12)),
            ],
        ),
        module(
            "MEC528",
            "BEng Hons Final Year Project",
            Semester::S2,
            &["BEng"],
            vec![
                CourseworkTemplate::new("Poster", None),
                CourseworkTemplate::new("Coursework 2", None),
            ],
        ),
        module(
            "MEC526",
            "Research Methods and Management (SUST)",
            Semester::S1,
            &["Sust"],
            Vec::new(),
        ),
        module(
            "MEC528",
            "BEng Hons Final Year Project (SUST)",
            Semester::S2,
            &["Sust"],
            Vec::new(),
        ),
        module(
            "EEE840",
            "MEng Final Year Dissertation",
            Semester::FullYear,
            &["MEng"],
            Vec::new(),
        ),
        module(
            "EEE802",
            "MEng Final Year Dissertation",
            Semester::FullYear,
            &["MEngGermany"],
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

    /// Friday of the last teaching week, counted from the week-1 start date.
    pub fn default_teaching_end(start: NaiveDate) -> NaiveDate {
        let monday = start - Duration::days(i64::from(start.weekday().num_days_from_monday()));
        monday + Duration::weeks(TEACHING_WEEKS - 1) + Duration::days(4)
    }

    /// Last day of teaching: the date set in Settings, or Friday of week 12.
    pub fn teaching_end(&self, semester: Semester) -> Option<NaiveDate> {
        let end = match semester {
            Semester::S1 | Semester::FullYear => self.sem1_teaching_end,
            Semester::S2 => self.sem2_teaching_end,
        };
        end.or_else(|| {
            self.semester_start(semester)
                .map(Self::default_teaching_end)
        })
    }

    /// The semester a date belongs to (semester 2 once it has started).
    pub fn semester_of(&self, date: NaiveDate) -> Semester {
        if self.sem2_start.is_some_and(|s| date >= s) {
            Semester::S2
        } else {
            Semester::S1
        }
    }

    /// Teaching week number of `date`, if it falls within teaching.
    pub fn teaching_week(&self, date: NaiveDate) -> Option<(Semester, i64)> {
        [Semester::S1, Semester::S2].into_iter().find_map(|sem| {
            let start = self.semester_start(sem)?;
            let end = self.teaching_end(sem)?;
            (start..=end)
                .contains(&date)
                .then(|| (sem, (date - start).num_days() / 7 + 1))
        })
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

    pub fn modules_for<'a>(&'a self, programme: &'a str) -> impl Iterator<Item = &'a Module> {
        self.modules
            .iter()
            .filter(move |m| m.applies_to.iter().any(|p| p == programme))
    }

    pub fn programme(&self, id: &str) -> Option<&Programme> {
        self.programmes.iter().find(|p| p.id == id)
    }

    /// Short label of a course, for chips and event titles.
    pub fn programme_short(&self, id: &str) -> String {
        self.programme(id)
            .map_or_else(|| id.to_string(), |p| p.short.clone())
    }

    /// Sidebar lists of project students, in the order courses are defined.
    pub fn programme_lists(&self) -> Vec<String> {
        let mut lists: Vec<String> = Vec::new();
        for p in &self.programmes {
            if !lists.iter().any(|l| l == p.list_name()) {
                lists.push(p.list_name().to_string());
            }
        }
        lists
    }
}

// ---------------------------------------------------------------------------
// Student records
// ---------------------------------------------------------------------------

/// Upper bound on the weeks generated for one repeating meeting.
pub const MAX_OCCURRENCES: usize = 60;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Meeting {
    pub id: Uuid,
    /// The date, or the first date of a repeating meeting.
    pub date: Option<NaiveDate>,
    pub time: Option<NaiveTime>,
    /// For one-off meetings; repeating ones track this per week.
    pub held: bool,
    pub teams: bool,
    /// Room or place for in-person meetings.
    pub location: String,
    pub repeat: Option<Repeat>,
    pub notes: String,
}

impl Default for Meeting {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            date: None,
            time: None,
            held: false,
            teams: false,
            location: String::new(),
            repeat: None,
            notes: String::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Repeat {
    /// 1 = weekly, 2 = fortnightly, …
    pub every_weeks: u8,
    /// Last possible date; `None` means the end of teaching of the semester
    /// the meeting starts in.
    pub until: Option<NaiveDate>,
    /// Weeks that differ from the plan, keyed by their planned date.
    pub exceptions: Vec<Occurrence>,
}

impl Default for Repeat {
    fn default() -> Self {
        Self {
            every_weeks: 1,
            until: None,
            exceptions: Vec::new(),
        }
    }
}

/// One week of a meeting: moved, cancelled, held or with its own notes.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(default)]
pub struct Occurrence {
    pub planned: NaiveDate,
    pub cancelled: bool,
    pub moved_to: Option<NaiveDate>,
    /// Replaces the meeting's usual time for this week.
    pub time: Option<NaiveTime>,
    pub held: bool,
    pub notes: String,
}

impl Occurrence {
    fn planned(date: NaiveDate) -> Self {
        Self {
            planned: date,
            ..Default::default()
        }
    }

    pub fn date(&self) -> NaiveDate {
        self.moved_to.unwrap_or(self.planned)
    }

    /// Moved or retimed (not just held or annotated).
    pub fn rescheduled(&self) -> bool {
        !self.cancelled && (self.moved_to.is_some_and(|d| d != self.planned) || self.time.is_some())
    }

    fn is_plain(&self) -> bool {
        *self == Self::planned(self.planned)
    }
}

impl Meeting {
    /// Calendar UID; shared by the full export and invite files.
    pub fn uid(&self) -> String {
        format!("meeting-{}", self.id)
    }

    /// UID of one week of a repeating meeting, when exported on its own.
    pub fn occurrence_uid(&self, planned: NaiveDate) -> String {
        format!("meeting-{}-{}", self.id, planned.format("%Y%m%d"))
    }

    /// Where it happens: empty for Teams meetings, which say so themselves.
    pub fn place(&self) -> String {
        if self.teams {
            String::new()
        } else {
            self.location.trim().into()
        }
    }

    /// Last date a repeating meeting can fall on.
    pub fn repeat_end(&self, settings: &Settings) -> Option<NaiveDate> {
        let repeat = self.repeat.as_ref()?;
        let start = self.date?;
        repeat
            .until
            .or_else(|| settings.teaching_end(settings.semester_of(start)))
    }

    /// Every week of a repeating meeting with its changes, or the single
    /// meeting itself. Without a known end only the first week is listed.
    pub fn occurrences(&self, settings: &Settings) -> Vec<Occurrence> {
        let Some(start) = self.date else {
            return Vec::new();
        };
        let Some(repeat) = &self.repeat else {
            return vec![Occurrence {
                held: self.held,
                ..Occurrence::planned(start)
            }];
        };
        let end = self.repeat_end(settings).unwrap_or(start);
        let step = Duration::weeks(i64::from(repeat.every_weeks.max(1)));
        let mut out = Vec::new();
        let mut date = start;
        while date <= end && out.len() < MAX_OCCURRENCES {
            out.push(
                repeat
                    .exceptions
                    .iter()
                    .find(|o| o.planned == date)
                    .cloned()
                    .unwrap_or_else(|| Occurrence::planned(date)),
            );
            date += step;
        }
        if out.is_empty() {
            out.push(Occurrence::planned(start));
        }
        out
    }

    /// Store a week's changes; a week back to plain is forgotten.
    pub fn set_occurrence(&mut self, occurrence: Occurrence) {
        if let Some(repeat) = &mut self.repeat {
            repeat
                .exceptions
                .retain(|o| o.planned != occurrence.planned);
            if !occurrence.is_plain() {
                repeat.exceptions.push(occurrence);
                repeat.exceptions.sort_by_key(|o| o.planned);
            }
        }
    }
}

pub fn visit_uid(student_id: Uuid, index: usize) -> String {
    format!("visit{}-{student_id}", index + 1)
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(default)]
pub struct CwStatus {
    pub submitted: bool,
    pub submitted_on: Option<NaiveDate>,
    pub deadline_override: Option<NaiveDate>,
    pub notes: String,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct FypRecord {
    /// `Programme::id`; older files called this `kind`.
    #[serde(alias = "kind")]
    pub programme: String,
    pub project_title: String,
    pub initial_contact: bool,
    pub initial_contact_on: Option<NaiveDate>,
    pub meetings: Vec<Meeting>,
    /// Keyed by `CourseworkTemplate::id`.
    pub coursework: HashMap<Uuid, CwStatus>,
}

impl Default for FypRecord {
    fn default() -> Self {
        Self {
            programme: "BEng".into(),
            project_title: String::new(),
            initial_contact: false,
            initial_contact_on: None,
            meetings: Vec::new(),
            coursework: HashMap::new(),
        }
    }
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
    /// Every meeting across the project and advisee roles.
    pub fn meetings(&self) -> impl Iterator<Item = &Meeting> {
        let fyp = self.fyp.iter().flat_map(|f| &f.meetings);
        let advisee = self
            .advisee
            .iter()
            .flat_map(|a| a.s1.meetings.iter().chain(&a.s2.meetings));
        fyp.chain(advisee)
    }

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

/// A dated item derived from the records, used for the dashboard, the
/// calendar and .ics export. Repeating meetings give one event per week.
#[derive(Clone, Debug)]
pub struct Event {
    pub uid: String,
    pub student_id: Uuid,
    pub date: NaiveDate,
    pub time: Option<NaiveTime>,
    pub kind: EventKind,
    pub title: String,
    pub done: bool,
    /// A Teams meeting or an online visit.
    pub online: bool,
    /// Room or company for in-person items; empty otherwise.
    pub location: String,
    /// For a week of a repeating meeting: the series UID and its planned date.
    pub series: Option<(String, NaiveDate)>,
    pub cancelled: bool,
}

impl Event {
    /// A week of a repeating meeting that was moved to another day.
    pub fn moved(&self) -> bool {
        self.series
            .as_ref()
            .is_some_and(|(_, planned)| *planned != self.date)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Attendee {
    pub name: String,
    pub email: String,
}

/// How a repeating meeting recurs, for an .ics series.
#[derive(Clone, Debug)]
pub struct SeriesRule {
    pub every_weeks: u8,
    pub until: NaiveDate,
    /// Cancelled and rescheduled weeks.
    pub exceptions: Vec<Occurrence>,
}

/// One entry of an .ics file: a single event or a whole repeating series,
/// with the people to invite.
#[derive(Clone, Debug)]
pub struct CalItem {
    /// For a series, its first week with the usual time.
    pub event: Event,
    pub series: Option<SeriesRule>,
    pub attendees: Vec<Attendee>,
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

/// One event per week of `m` (only the meeting itself if it doesn't repeat).
fn push_meeting(
    out: &mut Vec<Event>,
    settings: &Settings,
    s: &Student,
    m: &Meeting,
    kind: EventKind,
    title: String,
) {
    for occ in m.occurrences(settings) {
        let (uid, series) = match m.repeat {
            Some(_) => (m.occurrence_uid(occ.planned), Some((m.uid(), occ.planned))),
            None => (m.uid(), None),
        };
        out.push(Event {
            uid,
            student_id: s.id,
            date: occ.date(),
            time: occ.time.or(m.time),
            kind,
            title: title.clone(),
            done: occ.held,
            online: m.teams,
            location: m.place(),
            series,
            cancelled: occ.cancelled,
        });
    }
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

    /// Every dated item across all active (non-archived) students, including
    /// cancelled weeks of repeating meetings.
    pub fn events(&self) -> Vec<Event> {
        let mut out = Vec::new();
        for s in self.students.iter().filter(|s| !s.archived) {
            self.student_events(s, &mut out);
        }
        out.sort_by_key(|e| (e.date, e.time));
        out
    }

    fn student_events(&self, s: &Student, out: &mut Vec<Event>) {
        let settings = &self.settings;
        let name = s.display_name();
        if let Some(fyp) = &s.fyp {
            let short = settings.programme_short(&fyp.programme);
            for m in &fyp.meetings {
                push_meeting(
                    out,
                    settings,
                    s,
                    m,
                    EventKind::FypMeeting,
                    format!("{short} project meeting – {name}"),
                );
            }
            for module in settings.modules_for(&fyp.programme) {
                for cw in &module.courseworks {
                    let status = fyp.coursework.get(&cw.id);
                    if let Some(date) = settings.effective_deadline(module, cw, status) {
                        out.push(Event {
                            uid: format!("cw-{}-{}", s.id, cw.id),
                            student_id: s.id,
                            date,
                            time: None,
                            kind: EventKind::Deadline,
                            title: format!("{} {} due – {name}", module.code, cw.title),
                            done: status.is_some_and(|st| st.submitted),
                            online: false,
                            location: String::new(),
                            series: None,
                            cancelled: false,
                        });
                    }
                }
            }
        }
        if let Some(p) = &s.placement {
            for (i, v) in p.visits.iter().enumerate() {
                if let Some(date) = v.date {
                    out.push(Event {
                        uid: visit_uid(s.id, i),
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
                        online: v.mode == VisitMode::Online,
                        location: match v.mode {
                            VisitMode::Online => String::new(),
                            VisitMode::FaceToFace => p.company.trim().into(),
                        },
                        series: None,
                        cancelled: false,
                    });
                }
            }
        }
        if let Some(a) = &s.advisee {
            for (sem, contact) in [("S1", &a.s1), ("S2", &a.s2)] {
                for (i, m) in contact.meetings.iter().enumerate() {
                    let label = if i == 0 { "meeting" } else { "follow-on" };
                    push_meeting(
                        out,
                        settings,
                        s,
                        m,
                        EventKind::AdviseeMeeting,
                        format!("Advisee {sem} {label} – {name}"),
                    );
                }
            }
        }
    }

    /// The people to invite to one of a student's events.
    fn attendees(s: &Student, event: &Event) -> Vec<Attendee> {
        let mut attendees: Vec<Attendee> = Vec::new();
        let mut add = |name: &str, email: &str| {
            let email = email.trim();
            if !email.is_empty()
                && !attendees
                    .iter()
                    .any(|a| a.email.eq_ignore_ascii_case(email))
            {
                attendees.push(Attendee {
                    name: name.trim().into(),
                    email: email.into(),
                });
            }
        };
        match (event.kind, &s.placement) {
            (EventKind::Deadline, _) => {}
            (EventKind::Visit, Some(p)) => {
                let email = if s.email.trim().is_empty() {
                    &p.professional_email
                } else {
                    &s.email
                };
                add(&s.name, email);
                let visit = p
                    .visits
                    .iter()
                    .enumerate()
                    .find(|(i, _)| visit_uid(s.id, *i) == event.uid);
                if let Some((_, v)) = visit
                    && v.line_manager_attending != TriState::No
                {
                    add(&p.line_manager_name, &p.line_manager_email);
                }
            }
            _ => add(&s.name, &s.email),
        }
        attendees
    }

    /// Calendar entries for the events `pick` selects (cancelled weeks are
    /// skipped). With `as_series`, any week of a repeating meeting brings in
    /// the whole series, once.
    pub fn cal_items(&self, pick: impl Fn(&Event) -> bool, as_series: bool) -> Vec<CalItem> {
        let mut out = Vec::new();
        let mut series_done = HashSet::new();
        for s in self.students.iter().filter(|s| !s.archived) {
            let mut events = Vec::new();
            self.student_events(s, &mut events);
            for e in events.into_iter().filter(|e| !e.cancelled && pick(e)) {
                let attendees = Self::attendees(s, &e);
                match &e.series {
                    Some((uid, _)) if as_series => {
                        if series_done.insert(uid.clone())
                            && let Some(item) = self.series_item(s, uid, e.clone(), attendees)
                        {
                            out.push(item);
                        }
                    }
                    _ => out.push(CalItem {
                        event: e,
                        series: None,
                        attendees,
                    }),
                }
            }
        }
        out.sort_by_key(|i| (i.event.date, i.event.time));
        out
    }

    /// The whole repeating meeting `uid`, from the event of one of its weeks.
    fn series_item(
        &self,
        s: &Student,
        uid: &str,
        week: Event,
        attendees: Vec<Attendee>,
    ) -> Option<CalItem> {
        let m = s.meetings().find(|m| m.uid() == uid)?;
        let repeat = m.repeat.as_ref()?;
        let occurrences = m.occurrences(&self.settings);
        let last = occurrences.last()?.planned;
        Some(CalItem {
            event: Event {
                uid: uid.into(),
                date: m.date?,
                time: m.time,
                done: false,
                series: None,
                cancelled: false,
                ..week
            },
            series: Some(SeriesRule {
                every_weeks: repeat.every_weeks.max(1),
                until: last,
                exceptions: occurrences
                    .into_iter()
                    .filter(|o| o.cancelled || o.rescheduled())
                    .collect(),
            }),
            attendees,
        })
    }

    /// The meeting (whole series if it repeats) or visit `uid` of one
    /// student, ready for an invite file.
    pub fn invite(&self, student_id: Uuid, uid: &str) -> Option<CalItem> {
        let s = self.student(student_id)?;
        let mut events = Vec::new();
        self.student_events(s, &mut events);
        let e = events.into_iter().find(|e| {
            e.kind != EventKind::Deadline
                && (e.uid == uid || e.series.as_ref().is_some_and(|(series, _)| series == uid))
        })?;
        let attendees = Self::attendees(s, &e);
        match e.series.as_ref().map(|(series, _)| series.clone()) {
            Some(series) => self.series_item(s, &series, e, attendees),
            None => Some(CalItem {
                event: e,
                series: None,
                attendees,
            }),
        }
    }

    pub fn upcoming(&self, today: NaiveDate, days: u32) -> Vec<Event> {
        let end = today + Duration::days(i64::from(days));
        self.events()
            .into_iter()
            .filter(|e| !e.done && !e.cancelled && e.date >= today && e.date <= end)
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
                for module in self.settings.modules_for(&fyp.programme) {
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
    fn modules_per_programme() {
        let s = Settings::default();
        let codes = |p| s.modules_for(p).map(|m| m.code.clone()).collect::<Vec<_>>();
        assert_eq!(codes("BEng"), ["MEC526", "MEC528"]);
        assert_eq!(codes("Sust"), ["MEC526", "MEC528"]);
        assert_eq!(codes("MEng"), ["EEE840"]);
        assert_eq!(codes("MEngGermany"), ["EEE802"]);
        assert!(codes("MScElectronics").is_empty());
        assert_eq!(s.programme_lists(), ["BEng", "MEng", "SUST", "MSc"]);
    }

    #[test]
    fn older_files_still_load() {
        // Before courses were editable: `kind` enum names and no `programmes`.
        let json = r#"{"version":1,"settings":{"academic_year":"2026/27","modules":[
            {"code":"EEE802","name":"x","semester":"FullYear","applies_to":["MEngGermany"],"courseworks":[]}]},
            "students":[{"name":"Tom","fyp":{"kind":"MEngGermany","meetings":[{"date":"2026-10-01","held":true}]}}]}"#;
        let db: Database = serde_json::from_str(json).unwrap();
        assert_eq!(
            db.students[0].fyp.as_ref().unwrap().programme,
            "MEngGermany"
        );
        assert_eq!(db.settings.programme("MEngGermany").unwrap().list, "MEng");
        assert_eq!(db.settings.modules_for("MEngGermany").count(), 1);
        assert!(
            db.students[0].fyp.as_ref().unwrap().meetings[0]
                .repeat
                .is_none()
        );
    }

    #[test]
    fn teaching_calendar() {
        let mut s = Settings {
            sem1_start: Some(d(2026, 9, 21)),
            sem2_start: Some(d(2027, 1, 25)),
            ..Default::default()
        };
        // Friday of week 12 unless set.
        assert_eq!(s.teaching_end(Semester::S1), Some(d(2026, 12, 11)));
        assert_eq!(s.teaching_week(d(2026, 9, 21)), Some((Semester::S1, 1)));
        assert_eq!(s.teaching_week(d(2026, 10, 26)), Some((Semester::S1, 6)));
        assert_eq!(s.teaching_week(d(2026, 12, 14)), None);
        s.sem1_teaching_end = Some(d(2026, 12, 18));
        assert_eq!(s.teaching_week(d(2026, 12, 14)), Some((Semester::S1, 13)));
        assert_eq!(s.semester_of(d(2027, 2, 1)), Semester::S2);
        assert_eq!(s.teaching_end(Semester::S2), Some(d(2027, 4, 16)));
        // Week 1 starting mid-week still ends on the Friday of week 12.
        assert_eq!(
            Settings::default_teaching_end(d(2026, 9, 23)),
            d(2026, 12, 11)
        );
    }

    #[test]
    fn repeating_meeting_weeks() {
        let settings = Settings {
            sem1_start: Some(d(2026, 9, 21)),
            ..Default::default()
        };
        let mut m = Meeting {
            date: Some(d(2026, 11, 2)),
            time: NaiveTime::from_hms_opt(14, 0, 0),
            repeat: Some(Repeat::default()),
            ..Default::default()
        };
        // Weekly from 2 Nov up to the end of teaching (Fri 11 Dec): 6 Mondays.
        let weeks = m.occurrences(&settings);
        assert_eq!(weeks.len(), 6);
        assert_eq!(weeks.last().unwrap().planned, d(2026, 12, 7));

        m.repeat.as_mut().unwrap().every_weeks = 2;
        assert_eq!(m.occurrences(&settings).len(), 3);
        m.repeat.as_mut().unwrap().every_weeks = 1;

        m.set_occurrence(Occurrence {
            planned: d(2026, 11, 9),
            cancelled: true,
            ..Default::default()
        });
        m.set_occurrence(Occurrence {
            planned: d(2026, 11, 16),
            moved_to: Some(d(2026, 11, 17)),
            ..Default::default()
        });
        m.set_occurrence(Occurrence {
            planned: d(2026, 11, 23),
            held: true,
            ..Default::default()
        });
        assert_eq!(m.repeat.as_ref().unwrap().exceptions.len(), 3);
        // Back to plain: forgotten.
        m.set_occurrence(Occurrence {
            planned: d(2026, 11, 23),
            ..Default::default()
        });
        assert_eq!(m.repeat.as_ref().unwrap().exceptions.len(), 2);

        let mut db = Database {
            settings,
            ..Default::default()
        };
        let student = Student {
            name: "Ada".into(),
            email: "ada@ulster.ac.uk".into(),
            fyp: Some(FypRecord {
                meetings: vec![m.clone()],
                ..Default::default()
            }),
            ..Default::default()
        };
        let id = student.id;
        db.students.push(student);

        let events: Vec<_> = db
            .events()
            .into_iter()
            .filter(|e| e.kind == EventKind::FypMeeting)
            .collect();
        assert_eq!(events.len(), 6);
        assert_eq!(events.iter().filter(|e| e.cancelled).count(), 1);
        assert!(
            events
                .iter()
                .any(|e| e.moved() && e.date == d(2026, 11, 17))
        );
        let upcoming = db.upcoming(d(2026, 11, 1), 30);
        let meetings = upcoming
            .iter()
            .filter(|e| e.kind == EventKind::FypMeeting)
            .count();
        assert_eq!(meetings, 4, "cancelled week not upcoming");

        // Invite for the meeting: the whole series with its exceptions.
        let item = db.invite(id, &m.uid()).unwrap();
        let rule = item.series.unwrap();
        assert_eq!(item.event.uid, m.uid());
        assert_eq!(item.event.date, d(2026, 11, 2));
        assert_eq!(rule.until, d(2026, 12, 7));
        assert_eq!(rule.exceptions.len(), 2);

        // Picking two weeks: one series as a series, two events otherwise.
        let picked = [
            m.occurrence_uid(d(2026, 11, 2)),
            m.occurrence_uid(d(2026, 11, 30)),
        ];
        assert_eq!(db.cal_items(|e| picked.contains(&e.uid), true).len(), 1);
        let singles = db.cal_items(|e| picked.contains(&e.uid), false);
        assert_eq!(singles.len(), 2);
        assert!(singles.iter().all(|i| i.series.is_none()));
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
    fn invite_attendees() {
        let mut db = Database::default();
        let meeting = Meeting {
            date: Some(d(2026, 10, 30)),
            teams: true,
            location: "MG.201".into(),
            ..Default::default()
        };
        let mut placement = PlacementRecord {
            company: "Bombardier".into(),
            line_manager_name: "J. Smith".into(),
            line_manager_email: "j.smith@company.com".into(),
            ..Default::default()
        };
        placement.visits[1].date = Some(d(2026, 11, 5));
        placement.visits[1].line_manager_attending = TriState::Yes;
        let student = Student {
            name: "Ada".into(),
            email: "ada@ulster.ac.uk".into(),
            fyp: Some(FypRecord {
                meetings: vec![meeting.clone()],
                ..Default::default()
            }),
            placement: Some(placement),
            ..Default::default()
        };
        let id = student.id;
        db.students.push(student);

        let inv = db.invite(id, &meeting.uid()).unwrap();
        assert!(inv.event.online);
        assert_eq!(inv.event.location, "", "Teams meetings ignore the room");
        assert_eq!(inv.attendees.len(), 1);
        assert_eq!(inv.attendees[0].email, "ada@ulster.ac.uk");

        // Face-to-face visit: line manager invited, company as the location.
        let inv = db.invite(id, &visit_uid(id, 1)).unwrap();
        assert!(!inv.event.online);
        assert_eq!(inv.event.location, "Bombardier");
        assert_eq!(inv.attendees.len(), 2);
        db.students[0].placement.as_mut().unwrap().visits[1].line_manager_attending = TriState::No;
        assert_eq!(db.invite(id, &visit_uid(id, 1)).unwrap().attendees.len(), 1);

        assert!(db.invite(id, "meeting-unknown").is_none());

        db.students[0].fyp.as_mut().unwrap().meetings[0].teams = false;
        assert_eq!(
            db.invite(id, &meeting.uid()).unwrap().event.location,
            "MG.201"
        );
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
