//! Data model: students, their roles (project, placement, advisee, PhD) and settings.

use std::collections::{HashMap, HashSet};

use chrono::{Datelike, Duration, Months, NaiveDate, NaiveTime};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const DB_VERSION: u32 = 1;

/// How long before a PhD student's visa ends to flag it on the dashboard.
const VISA_WARNING_DAYS: i64 = 90;
/// How long before a Brazilian grant ends to flag it on the dashboard...
const GRANT_WARNING_DAYS: i64 = 90;
/// ...and for how long after it ended.
const GRANT_ENDED_DAYS: i64 = 30;

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

/// Service used for an online meeting.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Platform {
    #[default]
    Teams,
    GoogleMeet,
    Zoom,
}

impl Platform {
    pub const ALL: [Platform; 3] = [Platform::Teams, Platform::GoogleMeet, Platform::Zoom];

    pub fn label(self) -> &'static str {
        match self {
            Platform::Teams => "Teams",
            Platform::GoogleMeet => "Google Meet",
            Platform::Zoom => "Zoom",
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

/// A course whose project students are supervised (BEng, MEng, …).
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
    /// Lock after this many minutes without input; 0 = never.
    pub auto_lock_minutes: u32,
    /// Deadlines suggested for external students on Brazilian programmes.
    pub br_rules: Vec<BrRule>,
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
            auto_lock_minutes: 10,
            br_rules: default_br_rules(),
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

    /// Deadline rules of a Brazilian level, or the defaults if not in the file.
    pub fn br_rule(&self, level: BrLevel) -> BrRule {
        self.br_rules
            .iter()
            .find(|r| r.level == level)
            .cloned()
            .unwrap_or_else(|| BrRule::default_for(level))
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
    /// Online (Teams, Google Meet or Zoom) rather than in a room; older files call this `teams`.
    #[serde(alias = "teams")]
    pub online: bool,
    pub platform: Platform,
    /// Meeting link; Teams meetings without one use the link in Settings.
    pub link: String,
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
            online: false,
            platform: Platform::Teams,
            link: String::new(),
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

    /// Where it happens: empty for online meetings, which say so themselves.
    pub fn place(&self) -> String {
        if self.online {
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

    /// The week planned for `planned`, with any changes made to it.
    pub fn occurrence(&self, planned: NaiveDate) -> Occurrence {
        self.repeat
            .as_ref()
            .and_then(|r| r.exceptions.iter().find(|o| o.planned == planned))
            .cloned()
            .unwrap_or_else(|| Occurrence::planned(planned))
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

// ---------------------------------------------------------------------------
// PhD / research students
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ResearchStatus {
    #[default]
    Registered,
    Suspended,
    WritingUp,
    Submitted,
    Awarded,
    Withdrawn,
}

impl ResearchStatus {
    pub const ALL: [ResearchStatus; 6] = [
        ResearchStatus::Registered,
        ResearchStatus::Suspended,
        ResearchStatus::WritingUp,
        ResearchStatus::Submitted,
        ResearchStatus::Awarded,
        ResearchStatus::Withdrawn,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ResearchStatus::Registered => "Registered",
            ResearchStatus::Suspended => "Suspended",
            ResearchStatus::WritingUp => "Writing up",
            ResearchStatus::Submitted => "Submitted",
            ResearchStatus::Awarded => "Awarded",
            ResearchStatus::Withdrawn => "Withdrawn",
        }
    }

    /// Still working towards the degree, so deadlines and visas matter.
    pub fn active(self) -> bool {
        !matches!(self, ResearchStatus::Awarded | ResearchStatus::Withdrawn)
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum StudyMode {
    #[default]
    FullTime,
    PartTime,
}

impl StudyMode {
    pub fn label(self) -> &'static str {
        match self {
            StudyMode::FullTime => "Full time",
            StudyMode::PartTime => "Part time",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ResearchDegree {
    #[default]
    PhD,
    MPhil,
    ProfDoc,
    MRes,
    MSc,
}

impl ResearchDegree {
    pub const ALL: [ResearchDegree; 5] = [
        ResearchDegree::PhD,
        ResearchDegree::MSc,
        ResearchDegree::MPhil,
        ResearchDegree::ProfDoc,
        ResearchDegree::MRes,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ResearchDegree::PhD => "PhD",
            ResearchDegree::MPhil => "MPhil",
            ResearchDegree::ProfDoc => "Professional doctorate",
            ResearchDegree::MRes => "MRes",
            ResearchDegree::MSc => "MSc",
        }
    }

    /// For event titles, e.g. "MSc supervision".
    pub fn short(self) -> &'static str {
        match self {
            ResearchDegree::ProfDoc => "Prof Doc",
            other => other.label(),
        }
    }

    /// Master's degrees are listed under MSc, the others under PhD.
    pub fn masters(self) -> bool {
        matches!(self, ResearchDegree::MSc | ResearchDegree::MRes)
    }
}

/// The research lists in the sidebar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResearchGroup {
    Phd,
    PhdExternal,
    Msc,
    MscExternal,
}

impl ResearchGroup {
    pub const ALL: [ResearchGroup; 4] = [
        ResearchGroup::Phd,
        ResearchGroup::PhdExternal,
        ResearchGroup::Msc,
        ResearchGroup::MscExternal,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ResearchGroup::Phd => "PhD",
            ResearchGroup::PhdExternal => "PhD External",
            ResearchGroup::Msc => "MSc",
            ResearchGroup::MscExternal => "MSc External",
        }
    }

    pub fn external(self) -> bool {
        matches!(self, ResearchGroup::PhdExternal | ResearchGroup::MscExternal)
    }

    /// A new record for this list. External students start without the
    /// university's standard assessments.
    pub fn new_record(self) -> PhdRecord {
        let degree = match self {
            ResearchGroup::Phd | ResearchGroup::PhdExternal => ResearchDegree::PhD,
            ResearchGroup::Msc | ResearchGroup::MscExternal => ResearchDegree::MSc,
        };
        PhdRecord {
            degree,
            external: self.external(),
            skip_standard: self.external(),
            ..Default::default()
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FeeStatus {
    #[default]
    National,
    Overseas,
}

impl FeeStatus {
    pub fn label(self) -> &'static str {
        match self {
            FeeStatus::National => "National",
            FeeStatus::Overseas => "Overseas",
        }
    }
}

/// UK immigration routes a research student may hold.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum VisaType {
    #[default]
    NotRequired,
    Student,
    Graduate,
    SkilledWorker,
    GlobalTalent,
    Dependant,
    EuSettlement,
    Other,
}

impl VisaType {
    pub const ALL: [VisaType; 8] = [
        VisaType::NotRequired,
        VisaType::Student,
        VisaType::Graduate,
        VisaType::SkilledWorker,
        VisaType::GlobalTalent,
        VisaType::Dependant,
        VisaType::EuSettlement,
        VisaType::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            VisaType::NotRequired => "Not required",
            VisaType::Student => "Student (Tier 4)",
            VisaType::Graduate => "Graduate",
            VisaType::SkilledWorker => "Skilled Worker",
            VisaType::GlobalTalent => "Global Talent",
            VisaType::Dependant => "Dependant",
            VisaType::EuSettlement => "EU Settlement Scheme",
            VisaType::Other => "Other",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Funder {
    #[default]
    NotSet,
    DfeQr,
    Dfe,
    Vcrs,
    MarieCurie,
    Erasmus,
    SelfFunded,
    Other,
}

impl Funder {
    pub const ALL: [Funder; 8] = [
        Funder::NotSet,
        Funder::DfeQr,
        Funder::Dfe,
        Funder::Vcrs,
        Funder::MarieCurie,
        Funder::Erasmus,
        Funder::SelfFunded,
        Funder::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Funder::NotSet => "—",
            Funder::DfeQr => "DfE QR",
            Funder::Dfe => "DfE",
            Funder::Vcrs => "VCRS",
            Funder::MarieCurie => "Marie Curie",
            Funder::Erasmus => "Erasmus",
            Funder::SelfFunded => "Self-funded",
            Funder::Other => "Other",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum VivaOutcome {
    #[default]
    Pending,
    Pass,
    MinorCorrections,
    MajorCorrections,
    Resubmit,
    MPhil,
    Fail,
}

impl VivaOutcome {
    pub const ALL: [VivaOutcome; 7] = [
        VivaOutcome::Pending,
        VivaOutcome::Pass,
        VivaOutcome::MinorCorrections,
        VivaOutcome::MajorCorrections,
        VivaOutcome::Resubmit,
        VivaOutcome::MPhil,
        VivaOutcome::Fail,
    ];

    pub fn label(self) -> &'static str {
        match self {
            VivaOutcome::Pending => "Not yet known",
            VivaOutcome::Pass => "Pass, no corrections",
            VivaOutcome::MinorCorrections => "Minor corrections",
            VivaOutcome::MajorCorrections => "Major corrections",
            VivaOutcome::Resubmit => "Revise and resubmit",
            VivaOutcome::MPhil => "Award of MPhil",
            VivaOutcome::Fail => "Fail",
        }
    }
}

/// A deadline from the university system, typed in, and when it was met.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(default)]
pub struct Milestone {
    pub deadline: Option<NaiveDate>,
    pub done: bool,
    pub done_on: Option<NaiveDate>,
}

impl Milestone {
    /// The reminder date, `lead` before the deadline.
    pub fn reminder(&self, lead: Lead) -> Option<NaiveDate> {
        lead.before(self.deadline?)
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LeadUnit {
    Days,
    #[default]
    Weeks,
    Months,
}

impl LeadUnit {
    pub const ALL: [LeadUnit; 3] = [LeadUnit::Days, LeadUnit::Weeks, LeadUnit::Months];

    pub fn label(self) -> &'static str {
        match self {
            LeadUnit::Days => "days",
            LeadUnit::Weeks => "weeks",
            LeadUnit::Months => "months",
        }
    }
}

/// How long before a deadline its reminder falls; 0 means no reminder.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(default)]
pub struct Lead {
    pub amount: u32,
    pub unit: LeadUnit,
}

impl Default for Lead {
    fn default() -> Self {
        Self {
            amount: 2,
            unit: LeadUnit::Weeks,
        }
    }
}

impl Lead {
    pub fn months(amount: u32) -> Self {
        Self {
            amount,
            unit: LeadUnit::Months,
        }
    }

    pub fn weeks(amount: u32) -> Self {
        Self {
            amount,
            unit: LeadUnit::Weeks,
        }
    }

    pub fn before(self, date: NaiveDate) -> Option<NaiveDate> {
        match (self.amount, self.unit) {
            (0, _) => None,
            (n, LeadUnit::Days) => date.checked_sub_signed(Duration::days(i64::from(n))),
            (n, LeadUnit::Weeks) => date.checked_sub_signed(Duration::weeks(i64::from(n))),
            (n, LeadUnit::Months) => date.checked_sub_months(Months::new(n)),
        }
    }
}

/// A deadline added by hand (e.g. from another university), with its own reminder.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct CustomStep {
    pub id: Uuid,
    pub title: String,
    pub milestone: Milestone,
    pub remind: Lead,
}

impl Default for CustomStep {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            title: String::new(),
            milestone: Milestone::default(),
            remind: Lead::default(),
        }
    }
}

/// One dated step of a research degree, for events, reminders and editing
/// (change a copy, then write it back with `PhdRecord::milestone_mut`).
pub struct PhdStep {
    /// Stable part of the event UID.
    pub key: String,
    /// E.g. "Initial assessment".
    pub label: String,
    pub milestone: Milestone,
    /// How long before the deadline the reminder falls.
    pub lead: Lead,
}

// ---------------------------------------------------------------------------
// Brazilian programmes (external MSc and PhD students)
// ---------------------------------------------------------------------------

/// Stricto sensu levels of a Brazilian postgraduate programme.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum BrLevel {
    #[default]
    AcademicMasters,
    ProfessionalMasters,
    AcademicDoctorate,
    ProfessionalDoctorate,
}

impl BrLevel {
    pub const ALL: [BrLevel; 4] = [
        BrLevel::AcademicMasters,
        BrLevel::ProfessionalMasters,
        BrLevel::AcademicDoctorate,
        BrLevel::ProfessionalDoctorate,
    ];

    pub fn label(self) -> &'static str {
        match self {
            BrLevel::AcademicMasters => "Academic master's",
            BrLevel::ProfessionalMasters => "Professional master's",
            BrLevel::AcademicDoctorate => "Academic doctorate",
            BrLevel::ProfessionalDoctorate => "Professional doctorate",
        }
    }

    /// The name used in Brazil, as a hint.
    pub fn portuguese(self) -> &'static str {
        match self {
            BrLevel::AcademicMasters => "Mestrado acadêmico",
            BrLevel::ProfessionalMasters => "Mestrado profissional",
            BrLevel::AcademicDoctorate => "Doutorado acadêmico",
            BrLevel::ProfessionalDoctorate => "Doutorado profissional",
        }
    }

    /// For event titles, e.g. "MSc supervision".
    pub fn short(self) -> &'static str {
        match self {
            BrLevel::AcademicMasters => "MSc",
            BrLevel::ProfessionalMasters => "Prof MSc",
            BrLevel::AcademicDoctorate => "PhD",
            BrLevel::ProfessionalDoctorate => "Prof Doc",
        }
    }

    pub fn masters(self) -> bool {
        matches!(self, BrLevel::AcademicMasters | BrLevel::ProfessionalMasters)
    }

    /// The work that ends the course.
    pub fn work(self) -> &'static str {
        match self {
            BrLevel::AcademicMasters => "dissertation",
            BrLevel::ProfessionalMasters => "final project",
            BrLevel::AcademicDoctorate | BrLevel::ProfessionalDoctorate => "thesis",
        }
    }

    /// The closest degree of this university, which picks the sidebar list.
    pub fn degree(self) -> ResearchDegree {
        match self {
            BrLevel::AcademicMasters | BrLevel::ProfessionalMasters => ResearchDegree::MSc,
            BrLevel::AcademicDoctorate => ResearchDegree::PhD,
            BrLevel::ProfessionalDoctorate => ResearchDegree::ProfDoc,
        }
    }

    pub fn from_degree(degree: ResearchDegree) -> Self {
        match degree {
            ResearchDegree::ProfDoc => BrLevel::ProfessionalDoctorate,
            d if d.masters() => BrLevel::AcademicMasters,
            _ => BrLevel::AcademicDoctorate,
        }
    }

    /// Usual length of a grant in months: up to 24 for a master's and 48 for
    /// a doctorate (CAPES Portaria 76/2010 art. 10, CNPq and most FAPs).
    pub fn typical_grant_months(self) -> u32 {
        if self.masters() { 24 } else { 48 }
    }
}

/// Deadlines of a Brazilian level, in months from enrolment; 0 = doesn't
/// apply. Each programme's regulations (regimento) set its own, so these are
/// only a common starting point and can be changed in Settings.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct BrRule {
    pub level: BrLevel,
    /// Complete the course credits.
    pub credits_months: u32,
    /// Foreign-language proficiency exam.
    pub proficiency_months: u32,
    /// Qualifying exam.
    pub qualification_months: u32,
    pub defence_months: u32,
}

impl Default for BrRule {
    fn default() -> Self {
        Self::default_for(BrLevel::AcademicMasters)
    }
}

impl BrRule {
    pub fn default_for(level: BrLevel) -> Self {
        let (credits, proficiency, qualification, defence) = match level {
            BrLevel::AcademicMasters => (12, 12, 12, 24),
            BrLevel::ProfessionalMasters => (12, 12, 15, 24),
            BrLevel::AcademicDoctorate => (24, 24, 24, 48),
            BrLevel::ProfessionalDoctorate => (24, 24, 30, 48),
        };
        Self {
            level,
            credits_months: credits,
            proficiency_months: proficiency,
            qualification_months: qualification,
            defence_months: defence,
        }
    }
}

pub fn default_br_rules() -> Vec<BrRule> {
    BrLevel::ALL.into_iter().map(BrRule::default_for).collect()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AgencyKind {
    Federal,
    /// A state research foundation (FAP).
    State,
    Other,
}

/// A Brazilian body that funds postgraduate scholarships.
pub struct Agency {
    /// Stored on grants.
    pub id: &'static str,
    pub name: &'static str,
    pub kind: AgencyKind,
    /// State (UF) of a FAP.
    pub uf: &'static str,
}

const fn agency(id: &'static str, name: &'static str, kind: AgencyKind, uf: &'static str) -> Agency {
    Agency { id, name, kind, uf }
}

/// Federal agencies, the 27 state research foundations (FAPs, one per
/// state, as listed by CONFAP) and other sources.
pub const AGENCIES: &[Agency] = &[
    agency("CNPq", "National Council for Scientific and Technological Development", AgencyKind::Federal, ""),
    agency("CAPES", "Coordination for the Improvement of Higher Education Personnel", AgencyKind::Federal, ""),
    agency("FINEP", "Funding Authority for Studies and Projects", AgencyKind::Federal, ""),
    agency("ANP", "National Petroleum Agency – PRH-ANP human resources programme", AgencyKind::Federal, ""),
    agency("FAPAC", "Acre Research Foundation", AgencyKind::State, "AC"),
    agency("FAPEAL", "Alagoas State Research Foundation", AgencyKind::State, "AL"),
    agency("FAPEAP", "Amapá Research Foundation", AgencyKind::State, "AP"),
    agency("FAPEAM", "Amazonas State Research Foundation", AgencyKind::State, "AM"),
    agency("FAPESB", "Bahia State Research Foundation", AgencyKind::State, "BA"),
    agency("FUNCAP", "Ceará Foundation for Scientific and Technological Development", AgencyKind::State, "CE"),
    agency("FAPDF", "Federal District Research Foundation", AgencyKind::State, "DF"),
    agency("FAPES", "Espírito Santo Research and Innovation Foundation", AgencyKind::State, "ES"),
    agency("FAPEG", "Goiás State Research Foundation", AgencyKind::State, "GO"),
    agency("FAPEMA", "Maranhão Foundation for Research and Scientific Development", AgencyKind::State, "MA"),
    agency("FAPEMAT", "Mato Grosso State Research Foundation", AgencyKind::State, "MT"),
    agency("FUNDECT", "Mato Grosso do Sul Foundation for Education, Science and Technology", AgencyKind::State, "MS"),
    agency("FAPEMIG", "Minas Gerais State Research Foundation", AgencyKind::State, "MG"),
    agency("FAPESPA", "Amazon Research Foundation of Pará", AgencyKind::State, "PA"),
    agency("FAPESQ", "Paraíba State Research Foundation", AgencyKind::State, "PB"),
    agency("Araucária", "Araucária Foundation (Paraná)", AgencyKind::State, "PR"),
    agency("FACEPE", "Pernambuco Science and Technology Foundation", AgencyKind::State, "PE"),
    agency("FAPEPI", "Piauí State Research Foundation", AgencyKind::State, "PI"),
    agency("FAPERJ", "Carlos Chagas Filho Research Foundation of Rio de Janeiro", AgencyKind::State, "RJ"),
    agency("FAPERN", "Rio Grande do Norte State Research Foundation", AgencyKind::State, "RN"),
    agency("FAPERGS", "Rio Grande do Sul State Research Foundation", AgencyKind::State, "RS"),
    agency("FAPERO", "Rondônia Research Foundation", AgencyKind::State, "RO"),
    agency("FAPERR", "Roraima State Research Foundation", AgencyKind::State, "RR"),
    agency("FAPESC", "Santa Catarina Research and Innovation Foundation", AgencyKind::State, "SC"),
    agency("FAPESP", "São Paulo Research Foundation", AgencyKind::State, "SP"),
    agency("FAPITEC", "Sergipe Foundation for Research and Technological Innovation", AgencyKind::State, "SE"),
    agency("FAPT", "Tocantins Research Foundation", AgencyKind::State, "TO"),
    agency("Institutional", "A grant from the student's own university", AgencyKind::Other, ""),
    agency("Company", "Company or partnership agreement", AgencyKind::Other, ""),
    agency("Other", "Other source", AgencyKind::Other, ""),
];

pub fn agency_by_id(id: &str) -> Option<&'static Agency> {
    AGENCIES.iter().find(|a| a.id == id)
}

/// "thesis" → "Thesis".
fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
}

/// Why a grant or the course deadlines were extended.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ExtensionReason {
    #[default]
    None,
    /// Birth, adoption or guardianship (Lei 13.536/2017, amended by Lei 14.925/2024).
    Parental,
    Health,
    /// Decided by the programme's board (colegiado).
    Board,
    Other,
}

impl ExtensionReason {
    pub const ALL: [ExtensionReason; 5] = [
        ExtensionReason::None,
        ExtensionReason::Parental,
        ExtensionReason::Health,
        ExtensionReason::Board,
        ExtensionReason::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ExtensionReason::None => "—",
            ExtensionReason::Parental => "Parental leave (Law 14,925/2024)",
            ExtensionReason::Health => "Health",
            ExtensionReason::Board => "Programme board decision",
            ExtensionReason::Other => "Other reason",
        }
    }

    /// A hint on how long the extension may be.
    pub fn hint(self) -> &'static str {
        match self {
            ExtensionReason::Parental => {
                "Law 14,925/2024 allows grants to be extended by up to 180 days (6 months) for childbirth, adoption or guardianship."
            }
            ExtensionReason::Health => {
                "At CAPES (Portaria 76/2010), a suspension for serious illness, childbirth or breastfeeding lasts up to 6 months and doesn't count towards the grant period."
            }
            _ => "",
        }
    }
}

/// A Brazilian scholarship (bolsa): who pays, which scheme, and for how long.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(default)]
pub struct Grant {
    /// `Agency::id`; empty when there is no grant.
    pub agency: String,
    /// Name of the source when `agency` is "Other".
    pub agency_other: String,
    /// E.g. "Demanda Social", "PROEX", "GM", "GD".
    pub modality: String,
    /// Process number at the agency.
    pub process: String,
    pub start: Option<NaiveDate>,
    /// Planned length in months; 0 = not set.
    pub months: u32,
    /// Months added by an extension.
    pub extension_months: u32,
    pub extension_reason: ExtensionReason,
}

impl Grant {
    pub fn active(&self) -> bool {
        !self.agency.is_empty()
    }

    /// Last day of the grant: start + months + extension, minus a day.
    pub fn end(&self) -> Option<NaiveDate> {
        if !self.active() || self.months == 0 {
            return None;
        }
        self.start?
            .checked_add_months(Months::new(self.months + self.extension_months))?
            .pred_opt()
    }

    /// "CAPES", or the name typed in for another source.
    pub fn agency_label(&self) -> String {
        match self.agency.as_str() {
            "Other" if !self.agency_other.trim().is_empty() => self.agency_other.trim().into(),
            "" => "No grant".into(),
            id => id.into(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DefenceOutcome {
    #[default]
    Pending,
    Approved,
    ApprovedWithCorrections,
    Resubmit,
    Failed,
}

impl DefenceOutcome {
    pub const ALL: [DefenceOutcome; 5] = [
        DefenceOutcome::Pending,
        DefenceOutcome::Approved,
        DefenceOutcome::ApprovedWithCorrections,
        DefenceOutcome::Resubmit,
        DefenceOutcome::Failed,
    ];

    pub fn label(self) -> &'static str {
        match self {
            DefenceOutcome::Pending => "Not yet known",
            DefenceOutcome::Approved => "Approved",
            DefenceOutcome::ApprovedWithCorrections => "Approved with corrections",
            DefenceOutcome::Resubmit => "Revise and defend again",
            DefenceOutcome::Failed => "Not approved",
        }
    }

    /// A final version has to be handed in after the defence.
    pub fn corrections(self) -> bool {
        matches!(self, DefenceOutcome::ApprovedWithCorrections | DefenceOutcome::Resubmit)
    }
}

/// What a Brazilian programme adds to a research record: its level, grant,
/// stages and defence. Dates shared with the UK record (start, annual
/// reports, submission, the defence date, final version) stay on `PhdRecord`.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(default)]
pub struct BrazilRecord {
    pub level: BrLevel,
    /// Postgraduate programme (PPG).
    pub programme: String,
    /// Months added to the course deadlines by an extension (prorrogação).
    pub extension_months: u32,
    pub extension_reason: ExtensionReason,
    pub grant: Grant,
    pub credits: Milestone,
    pub proficiency: Milestone,
    pub qualification: Milestone,
    /// The deadline for the defence.
    pub defence_deadline: Milestone,
    /// Examining committee (banca).
    pub committee: String,
    pub outcome: DefenceOutcome,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(default)]
pub struct PhdRecord {
    pub title: String,
    pub chair: String,
    /// The rest of the supervisory team.
    pub supervisors: String,
    pub status: ResearchStatus,
    pub mode: StudyMode,
    pub degree: ResearchDegree,
    /// Registered at another university (co-supervised from here).
    pub external: bool,
    /// Their university, for external students.
    pub university: String,
    /// An external student on a Brazilian programme: `brazil` then gives
    /// the level, grant, stages and defence.
    pub brazilian: bool,
    pub brazil: BrazilRecord,
    pub fee_status: FeeStatus,
    pub visa: VisaType,
    /// Name of the visa when `visa` is `Other`.
    pub visa_other: String,
    pub visa_start: Option<NaiveDate>,
    pub visa_end: Option<NaiveDate>,
    pub cas_number: String,
    pub funding_source: String,
    pub funder: Funder,
    /// Name of the funder when `funder` is `Other`.
    pub funder_other: String,
    pub start: Option<NaiveDate>,
    pub end: Option<NaiveDate>,
    pub initial: Milestone,
    pub confirmation: Milestone,
    pub final_assessment: Milestone,
    pub annual_reports: Vec<Milestone>,
    pub examiners: Milestone,
    pub intention: Milestone,
    /// The earliest date the thesis may be submitted.
    pub earliest_submission: Option<NaiveDate>,
    pub submission: Milestone,
    pub viva: Option<NaiveDate>,
    pub viva_time: Option<NaiveTime>,
    pub viva_location: String,
    pub outcome: VivaOutcome,
    pub amendments: Milestone,
    pub awarded_on: Option<NaiveDate>,
    /// Leave out this university's standard assessments (initial,
    /// confirmation, annual reports, …), e.g. for external students.
    pub skip_standard: bool,
    /// Deadlines added by hand.
    pub custom_steps: Vec<CustomStep>,
    pub meetings: Vec<Meeting>,
}

impl PhdRecord {
    /// Every step with a deadline, in the order of the degree, then those
    /// added by hand. Reminder lead times of the standard steps follow the
    /// university system (e.g. 3 months for the initial assessment); for
    /// Brazilian programmes, the stages of a mestrado or doutorado.
    pub fn steps(&self) -> Vec<PhdStep> {
        let step = |key: &str, label: &str, milestone: &Milestone, lead| PhdStep {
            key: key.into(),
            label: label.into(),
            milestone: milestone.clone(),
            lead,
        };
        let annual_reports = self.annual_reports.iter().enumerate().map(|(i, report)| {
            let label = match report.deadline {
                Some(d) => format!("Annual report {}", d.year()),
                None => format!("Annual report {}", i + 1),
            };
            step(&format!("annual{}", i + 1), &label, report, Lead::months(1))
        });
        let mut steps = Vec::new();
        match (self.skip_standard, self.brazilian_programme()) {
            (true, _) => {}
            (false, true) => {
                let br = &self.brazil;
                let work = capitalised(br.level.work());
                steps.extend([
                    step("credits", "Course credits", &br.credits, Lead::months(2)),
                    step("proficiency", "Language proficiency", &br.proficiency, Lead::months(2)),
                    step("qualification", "Qualifying exam", &br.qualification, Lead::months(3)),
                ]);
                steps.extend(annual_reports);
                steps.extend([
                    step("submission", &format!("{work} to the committee"), &self.submission, Lead::months(1)),
                    step("defence", &format!("{work} defence"), &br.defence_deadline, Lead::months(4)),
                ]);
            }
            (false, false) => {
                steps.extend([
                    step("initial", "Initial assessment", &self.initial, Lead::months(3)),
                    step("confirmation", "Confirmation assessment", &self.confirmation, Lead::months(4)),
                    step("final", "Final assessment", &self.final_assessment, Lead::months(7)),
                ]);
                steps.extend(annual_reports);
                steps.extend([
                    step("examiners", "Appoint examiners", &self.examiners, Lead::months(1)),
                    step("intention", "Intention to submit", &self.intention, Lead::months(1)),
                    step("submission", "Thesis submission", &self.submission, Lead::months(1)),
                ]);
            }
        }
        steps.push(if self.brazilian_programme() {
            step("amendments", "Final version", &self.amendments, Lead::weeks(2))
        } else {
            step("amendments", "Amendments", &self.amendments, Lead::months(1))
        });
        steps.extend(self.custom_steps.iter().map(|c| PhdStep {
            key: format!("custom-{}", c.id),
            label: match c.title.trim() {
                "" => "Deadline".into(),
                title => title.into(),
            },
            milestone: c.milestone.clone(),
            lead: c.remind,
        }));
        steps
    }

    /// External and on a Brazilian programme (the box only counts for
    /// external students).
    pub fn brazilian_programme(&self) -> bool {
        self.external && self.brazilian
    }

    /// The Brazilian grant, if the student is on a Brazilian programme and has one.
    pub fn brazilian_grant(&self) -> Option<&Grant> {
        Some(&self.brazil.grant).filter(|g| self.brazilian_programme() && g.active())
    }

    /// Which research list the student is shown in.
    pub fn group(&self) -> ResearchGroup {
        let masters = if self.brazilian_programme() {
            self.brazil.level.masters()
        } else {
            self.degree.masters()
        };
        match (masters, self.external) {
            (false, false) => ResearchGroup::Phd,
            (false, true) => ResearchGroup::PhdExternal,
            (true, false) => ResearchGroup::Msc,
            (true, true) => ResearchGroup::MscExternal,
        }
    }

    /// The degree's name, or the Brazilian level.
    pub fn degree_label(&self) -> &'static str {
        if self.brazilian_programme() {
            self.brazil.level.label()
        } else {
            self.degree.label()
        }
    }

    /// For event titles, e.g. "MSc supervision".
    pub fn degree_short(&self) -> &'static str {
        if self.brazilian_programme() {
            self.brazil.level.short()
        } else {
            self.degree.short()
        }
    }

    /// The viva, or the defence, has a result.
    pub fn examined(&self) -> bool {
        if self.brazilian_programme() {
            self.brazil.outcome != DefenceOutcome::Pending
        } else {
            self.outcome != VivaOutcome::Pending
        }
    }

    /// Tick or untick "Brazilian programme". Ticking brings in the standard
    /// stages of the course (external students otherwise start without
    /// any) and a level matching the degree.
    pub fn set_brazilian(&mut self, brazilian: bool) {
        if brazilian == self.brazilian {
            return;
        }
        self.brazilian = brazilian;
        self.skip_standard = !brazilian;
        if brazilian {
            self.brazil.level = BrLevel::from_degree(self.degree);
        }
    }

    pub fn set_br_level(&mut self, level: BrLevel) {
        self.brazil.level = level;
        self.degree = level.degree();
    }

    /// The defence deadline the rules give: enrolment + months of the
    /// level + any extension.
    pub fn br_rule_deadline(&self, rule: &BrRule) -> Option<NaiveDate> {
        (rule.defence_months > 0)
            .then(|| {
                self.start?
                    .checked_add_months(Months::new(rule.defence_months + self.brazil.extension_months))
            })
            .flatten()
    }

    /// Fills the empty Brazilian deadlines from the start (enrolment) and
    /// the rules of the level; dates already set are kept. Annual reports
    /// are added for each full year before the defence deadline.
    pub fn suggest_br(&mut self, rule: &BrRule) {
        let Some(start) = self.start else { return };
        let at = |months: u32| (months > 0).then(|| start.checked_add_months(Months::new(months))).flatten();
        let defence = self.br_rule_deadline(rule);
        let br = &mut self.brazil;
        for (m, months) in [
            (&mut br.credits, rule.credits_months),
            (&mut br.proficiency, rule.proficiency_months),
            (&mut br.qualification, rule.qualification_months),
        ] {
            if m.deadline.is_none() {
                m.deadline = at(months);
            }
        }
        if br.defence_deadline.deadline.is_none() {
            br.defence_deadline.deadline = defence;
        }
        let defence = br.defence_deadline.deadline;
        if self.submission.deadline.is_none() {
            self.submission.deadline = defence.and_then(|d| d.checked_sub_months(Months::new(1)));
        }
        if self.annual_reports.is_empty()
            && let Some(end) = defence
        {
            let mut year = 1;
            while let Some(date) = at(12 * year).filter(|d| *d < end) {
                self.annual_reports.push(Milestone {
                    deadline: Some(date),
                    ..Default::default()
                });
                year += 1;
            }
        }
        let grant = &mut self.brazil.grant;
        if grant.active() && grant.start.is_none() {
            grant.start = Some(start);
        }
        if grant.active() && grant.months == 0 {
            grant.months = self.brazil.level.typical_grant_months();
        }
    }

    /// Year of study (1-based) on `date`, counted from the start.
    pub fn year_of_study(&self, date: NaiveDate) -> Option<u32> {
        let start = self.start?;
        let mut year = 1;
        while start.checked_add_months(Months::new(12 * year)).is_some_and(|d| d <= date) {
            year += 1;
        }
        (date >= start).then_some(year)
    }

    /// The milestone a `PhdStep::key` refers to.
    pub fn milestone_mut(&mut self, key: &str) -> Option<&mut Milestone> {
        match key {
            "initial" => Some(&mut self.initial),
            "confirmation" => Some(&mut self.confirmation),
            "final" => Some(&mut self.final_assessment),
            "examiners" => Some(&mut self.examiners),
            "intention" => Some(&mut self.intention),
            "submission" => Some(&mut self.submission),
            "amendments" => Some(&mut self.amendments),
            "credits" => Some(&mut self.brazil.credits),
            "proficiency" => Some(&mut self.brazil.proficiency),
            "qualification" => Some(&mut self.brazil.qualification),
            "defence" => Some(&mut self.brazil.defence_deadline),
            _ if key.starts_with("custom-") => self
                .custom_steps
                .iter_mut()
                .find(|c| format!("custom-{}", c.id) == key)
                .map(|c| &mut c.milestone),
            _ => {
                let n: usize = key.strip_prefix("annual")?.parse().ok()?;
                self.annual_reports.get_mut(n.checked_sub(1)?)
            }
        }
    }

    /// The earliest deadline not yet met.
    pub fn next_step(&self) -> Option<PhdStep> {
        self.steps()
            .into_iter()
            .filter(|s| !s.milestone.done && s.milestone.deadline.is_some())
            .min_by_key(|s| s.milestone.deadline)
    }
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
    pub phd: Option<PhdRecord>,
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
            phd: None,
        }
    }
}

/// What the student number is called: this university's B number, or the
/// registration number at an external student's own university.
pub fn id_label(external: bool) -> &'static str {
    if external { "Registration #" } else { "B Number" }
}

impl Student {
    /// Registered at another university (an external research student).
    pub fn external(&self) -> bool {
        self.phd.as_ref().is_some_and(|p| p.external)
    }

    /// Every meeting across the project, advisee and PhD roles.
    pub fn meetings(&self) -> impl Iterator<Item = &Meeting> {
        let fyp = self.fyp.iter().flat_map(|f| &f.meetings);
        let advisee = self
            .advisee
            .iter()
            .flat_map(|a| a.s1.meetings.iter().chain(&a.s2.meetings));
        let phd = self.phd.iter().flat_map(|p| &p.meetings);
        fyp.chain(advisee).chain(phd)
    }

    pub fn meetings_mut(&mut self) -> impl Iterator<Item = &mut Meeting> {
        let fyp = self.fyp.iter_mut().flat_map(|f| &mut f.meetings);
        let advisee = self
            .advisee
            .iter_mut()
            .flat_map(|a| a.s1.meetings.iter_mut().chain(&mut a.s2.meetings));
        let phd = self.phd.iter_mut().flat_map(|p| &mut p.meetings);
        fyp.chain(advisee).chain(phd)
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
        let thesis = self.phd.as_ref().map(|p| p.title.as_str());
        [
            Some(self.name.as_str()),
            Some(self.b_number.as_str()),
            Some(self.email.as_str()),
            Some(self.course.as_str()),
            company,
            thesis,
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
    /// PhD supervision meetings and the viva.
    PhdMeeting,
    Deadline,
    /// Advance notice of a PhD deadline.
    Reminder,
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
    /// An online meeting or visit.
    pub online: bool,
    /// Service of an online item (online visits use Teams).
    pub platform: Platform,
    /// The meeting's own link, if it has one.
    pub link: String,
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

    /// The id of the meeting this is (a week of), if it is a meeting.
    pub fn meeting_id(&self) -> Option<Uuid> {
        let rest = self.uid.strip_prefix("meeting-")?;
        Uuid::parse_str(rest.get(..36)?).ok()
    }
}

/// A change the calendar can make to a meeting, or to one week of a
/// repeating meeting.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MeetingChange {
    Held(bool),
    /// Move it to this day and time.
    Postpone(NaiveDate, Option<NaiveTime>),
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
            online: m.online,
            platform: m.platform,
            link: m.link.trim().into(),
            location: m.place(),
            series,
            cancelled: occ.cancelled,
        });
    }
}

/// Items of `events` in the next `days` days that are still to happen.
pub fn upcoming(events: &[Event], today: NaiveDate, days: u32) -> Vec<&Event> {
    let end = today + Duration::days(i64::from(days));
    events
        .iter()
        .filter(|e| !e.done && !e.cancelled && e.date >= today && e.date <= end)
        .collect()
}

impl Database {
    pub fn student(&self, id: Uuid) -> Option<&Student> {
        self.students.iter().find(|s| s.id == id)
    }

    pub fn student_mut(&mut self, id: Uuid) -> Option<&mut Student> {
        self.students.iter_mut().find(|s| s.id == id)
    }

    /// Apply `change` to the meeting behind `event`, or to that week of a
    /// repeating meeting. Returns false if `event` isn't a meeting.
    pub fn change_meeting(&mut self, event: &Event, change: MeetingChange) -> bool {
        let Some(id) = event.meeting_id() else { return false };
        let Some(m) = self
            .student_mut(event.student_id)
            .and_then(|s| s.meetings_mut().find(|m| m.id == id))
        else {
            return false;
        };
        match &event.series {
            None => match change {
                MeetingChange::Held(held) => m.held = held,
                MeetingChange::Postpone(date, time) => {
                    m.date = Some(date);
                    m.time = time;
                }
            },
            Some((_, planned)) => {
                let mut week = m.occurrence(*planned);
                match change {
                    MeetingChange::Held(held) => week.held = held,
                    MeetingChange::Postpone(date, time) => {
                        week.moved_to = Some(date);
                        week.time = time;
                    }
                }
                m.set_occurrence(week);
            }
        }
        true
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
                            platform: Platform::Teams,
                            link: String::new(),
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
                        platform: Platform::Teams,
                        link: String::new(),
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
        if let Some(p) = &s.phd {
            self.phd_events(s, p, &name, out);
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

    fn phd_events(&self, s: &Student, p: &PhdRecord, name: &str, out: &mut Vec<Event>) {
        let degree = p.degree_short();
        for m in &p.meetings {
            push_meeting(
                out,
                &self.settings,
                s,
                m,
                EventKind::PhdMeeting,
                format!("{degree} supervision – {name}"),
            );
        }
        if let Some(date) = p.viva {
            let location = p.viva_location.trim();
            let viva = if p.brazilian_programme() { "defence" } else { "viva" };
            out.push(Event {
                uid: format!("phd-viva-{}", s.id),
                student_id: s.id,
                date,
                time: p.viva_time,
                kind: EventKind::PhdMeeting,
                title: format!("{degree} {viva} – {name}"),
                done: p.examined(),
                online: false,
                platform: Platform::Teams,
                link: String::new(),
                location: location.into(),
                series: None,
                cancelled: false,
            });
        }
        if !p.status.active() {
            return;
        }
        for step in p.steps() {
            let Some(due) = step.milestone.deadline else {
                continue;
            };
            let what = step.label.to_lowercase();
            let event = |uid: String, date, kind, title| Event {
                uid,
                student_id: s.id,
                date,
                time: None,
                kind,
                title,
                done: step.milestone.done,
                online: false,
                platform: Platform::Teams,
                link: String::new(),
                location: String::new(),
                series: None,
                cancelled: false,
            };
            out.push(event(
                format!("phd-{}-{}", step.key, s.id),
                due,
                EventKind::Deadline,
                format!("{degree} {what} due – {name}"),
            ));
            if let Some(remind) = step.milestone.reminder(step.lead) {
                out.push(event(
                    format!("phd-{}-reminder-{}", step.key, s.id),
                    remind,
                    EventKind::Reminder,
                    format!("Reminder: {degree} {what} due {} – {name}", due.format("%d %b %Y")),
                ));
            }
        }
        if let Some(end) = p.brazilian_grant().and_then(Grant::end) {
            out.push(Event {
                uid: format!("phd-grant-end-{}", s.id),
                student_id: s.id,
                date: end,
                time: None,
                kind: EventKind::Deadline,
                title: format!("{degree} {} grant ends – {name}", p.brazil.grant.agency_label()),
                done: false,
                online: false,
                platform: Platform::Teams,
                link: String::new(),
                location: String::new(),
                series: None,
                cancelled: false,
            });
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
            (EventKind::Deadline | EventKind::Reminder, _) => {}
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
            !matches!(e.kind, EventKind::Deadline | EventKind::Reminder)
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
            if let Some(p) = s.phd.as_ref().filter(|p| p.status.active()) {
                for step in p.steps() {
                    let m = step.milestone;
                    let Some(due) = m.deadline.filter(|_| !m.done) else {
                        continue;
                    };
                    if due < today {
                        push(s, Severity::Overdue, format!("{} overdue (due {due})", step.label));
                    } else if m.reminder(step.lead).is_some_and(|r| r <= today) {
                        push(s, Severity::Pending, format!("{} due {due}", step.label));
                    }
                }
                if let Some(end) = p.visa_end.filter(|_| !p.external) {
                    if end < today {
                        push(s, Severity::Overdue, format!("visa ended {end}"));
                    } else if end <= today + Duration::days(VISA_WARNING_DAYS) {
                        push(s, Severity::Pending, format!("visa ends {end}"));
                    }
                }
                if let Some(grant) = p.brazilian_grant()
                    && let Some(end) = grant.end()
                {
                    let who = grant.agency_label();
                    if end < today && today - end <= Duration::days(GRANT_ENDED_DAYS) {
                        push(s, Severity::Pending, format!("{who} grant ended {end}"));
                    } else if end >= today && end <= today + Duration::days(GRANT_WARNING_DAYS) {
                        push(s, Severity::Pending, format!("{who} grant ends {end}"));
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
        assert_eq!(s.programme_lists(), ["BEng", "MEng", "SUST"]);
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
    fn calendar_marks_meetings_held_and_postpones_them() {
        let one_off = Meeting {
            date: Some(d(2026, 10, 5)),
            time: NaiveTime::from_hms_opt(14, 0, 0),
            ..Default::default()
        };
        let weekly = Meeting {
            date: Some(d(2026, 10, 6)),
            repeat: Some(Repeat {
                until: Some(d(2026, 11, 3)),
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut db = Database::default();
        db.students.push(Student {
            advisee: Some(AdviseeRecord {
                s1: SemesterContact {
                    meetings: vec![one_off.clone(), weekly.clone()],
                    ..Default::default()
                },
                ..Default::default()
            }),
            phd: Some(PhdRecord {
                viva: Some(d(2026, 10, 7)),
                ..Default::default()
            }),
            ..Default::default()
        });
        let find = |db: &Database, uid: &str| db.events().into_iter().find(|e| e.uid == uid).unwrap();

        let e = find(&db, &one_off.uid());
        assert_eq!(e.meeting_id(), Some(one_off.id));
        assert!(db.change_meeting(&e, MeetingChange::Held(true)));
        assert!(find(&db, &one_off.uid()).done);
        let later = NaiveTime::from_hms_opt(15, 30, 0);
        assert!(db.change_meeting(&e, MeetingChange::Postpone(d(2026, 10, 9), later)));
        let moved = find(&db, &one_off.uid());
        assert_eq!((moved.date, moved.time), (d(2026, 10, 9), later));

        // One week of a repeating meeting: only that week changes.
        let week = find(&db, &weekly.occurrence_uid(d(2026, 10, 13)));
        assert!(db.change_meeting(&week, MeetingChange::Held(true)));
        assert!(db.change_meeting(&week, MeetingChange::Postpone(d(2026, 10, 15), None)));
        let week = find(&db, &weekly.occurrence_uid(d(2026, 10, 13)));
        assert!(week.done && week.moved());
        assert_eq!(week.date, d(2026, 10, 15));
        assert!(!find(&db, &weekly.occurrence_uid(d(2026, 10, 20))).done);
        assert!(db.change_meeting(&week, MeetingChange::Held(false)));
        assert!(!find(&db, &weekly.occurrence_uid(d(2026, 10, 13))).done);

        // Not meetings.
        let viva = find(&db, &format!("phd-viva-{}", db.students[0].id));
        assert_eq!(viva.meeting_id(), None);
        assert!(!db.change_meeting(&viva, MeetingChange::Held(true)));
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
        let all = db.events();
        let meetings = upcoming(&all, d(2026, 11, 1), 30)
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
            online: true,
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

        db.students[0].fyp.as_mut().unwrap().meetings[0].online = false;
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

    /// The example from the university system: start 14 Sep 2026.
    fn example_phd() -> PhdRecord {
        let due = |date| Milestone {
            deadline: Some(date),
            ..Default::default()
        };
        PhdRecord {
            start: Some(d(2026, 9, 14)),
            end: Some(d(2029, 9, 14)),
            initial: due(d(2027, 3, 3)),
            confirmation: due(d(2027, 12, 3)),
            final_assessment: due(d(2029, 3, 3)),
            annual_reports: vec![due(d(2027, 5, 1))],
            examiners: due(d(2029, 6, 3)),
            intention: due(d(2029, 6, 3)),
            submission: due(d(2029, 9, 3)),
            ..Default::default()
        }
    }

    #[test]
    fn phd_reminders_match_the_university_system() {
        let p = example_phd();
        let reminders: Vec<(String, Option<NaiveDate>)> = p
            .steps()
            .iter()
            .map(|s| (s.label.clone(), s.milestone.reminder(s.lead)))
            .collect();
        assert_eq!(
            reminders,
            [
                ("Initial assessment", Some(d(2026, 12, 3))),
                ("Confirmation assessment", Some(d(2027, 8, 3))),
                ("Final assessment", Some(d(2028, 8, 3))),
                ("Annual report 2027", Some(d(2027, 4, 1))),
                ("Appoint examiners", Some(d(2029, 5, 3))),
                ("Intention to submit", Some(d(2029, 5, 3))),
                ("Thesis submission", Some(d(2029, 8, 3))),
                ("Amendments", None),
            ]
            .map(|(label, date)| (label.to_string(), date))
        );
        assert_eq!(p.year_of_study(d(2026, 9, 13)), None);
        assert_eq!(p.year_of_study(d(2026, 9, 14)), Some(1));
        assert_eq!(p.year_of_study(d(2027, 9, 14)), Some(2));
        assert_eq!(p.next_step().map(|s| s.label), Some("Initial assessment".into()));
    }

    #[test]
    fn phd_milestones_are_editable_by_key() {
        let mut p = example_phd();
        for step in p.steps() {
            assert!(p.milestone_mut(&step.key).is_some(), "{}", step.key);
        }
        p.milestone_mut("annual1").unwrap().done = true;
        assert!(p.annual_reports[0].done);
        assert!(p.milestone_mut("annual2").is_none());
        assert!(p.milestone_mut("annual0").is_none());
    }

    #[test]
    fn phd_deadlines_reminders_and_attention() {
        let mut p = example_phd();
        p.visa_end = Some(d(2027, 1, 10));
        let db = Database {
            students: vec![Student {
                name: "Priya".into(),
                phd: Some(p),
                ..Default::default()
            }],
            ..Default::default()
        };
        let events = db.events();
        let deadlines = events.iter().filter(|e| e.kind == EventKind::Deadline).count();
        let reminders: Vec<&Event> = events.iter().filter(|e| e.kind == EventKind::Reminder).collect();
        assert_eq!(deadlines, 7);
        assert_eq!(reminders.len(), 7);
        assert_eq!(reminders[0].date, d(2026, 12, 3));
        assert_eq!(reminders[0].title, "Reminder: PhD initial assessment due 03 Mar 2027 – Priya");

        // Reminder date passed: pending. Visa within 90 days: pending.
        let items = db.attention(d(2026, 12, 10));
        let messages: Vec<&str> = items.iter().map(|a| a.message.as_str()).collect();
        assert_eq!(
            messages,
            ["Priya – Initial assessment due 2027-03-03", "Priya – visa ends 2027-01-10"]
        );
        // Deadline passed: overdue, listed first.
        let items = db.attention(d(2027, 3, 4));
        assert_eq!(items[0].severity, Severity::Overdue);
        assert!(items[0].message.contains("Initial assessment overdue"));

        // Met deadlines, and students who have finished, raise nothing.
        let mut db = db;
        let p = db.students[0].phd.as_mut().unwrap();
        p.initial.done = true;
        p.visa_end = None;
        assert!(db.attention(d(2026, 12, 10)).is_empty());
        db.students[0].phd.as_mut().unwrap().status = ResearchStatus::Awarded;
        assert!(db.attention(d(2027, 3, 4)).is_empty());
        assert!(db.events().iter().all(|e| e.kind != EventKind::Deadline));
    }

    #[test]
    fn files_without_phd_records_still_load() {
        let json = r#"{"version":1,"students":[{"name":"Old","fyp":{"programme":"BEng"}}]}"#;
        let db: Database = serde_json::from_str(json).unwrap();
        assert!(db.students[0].phd.is_none());
        let p: PhdRecord = serde_json::from_str("{}").unwrap();
        assert_eq!(p, PhdRecord::default());
    }

    #[test]
    fn older_teams_meetings_load_as_online() {
        let m: Meeting = serde_json::from_str(r#"{"teams":true}"#).unwrap();
        assert!(m.online);
        assert_eq!(m.platform, Platform::Teams);
    }

    #[test]
    fn external_students_start_blank_with_their_own_deadlines() {
        let mut p = ResearchGroup::MscExternal.new_record();
        assert_eq!((p.degree, p.external, p.group()), (ResearchDegree::MSc, true, ResearchGroup::MscExternal));
        let keys = |p: &PhdRecord| p.steps().into_iter().map(|s| s.key).collect::<Vec<_>>();
        assert_eq!(keys(&p), ["amendments"], "no standard assessments");
        assert_eq!(ResearchGroup::Phd.new_record().group(), ResearchGroup::Phd);
        assert!(keys(&ResearchGroup::Phd.new_record()).contains(&"initial".to_string()));

        let exam = CustomStep {
            title: "Qualifying exam".into(),
            milestone: Milestone {
                deadline: Some(d(2027, 4, 30)),
                ..Default::default()
            },
            remind: Lead { amount: 10, unit: LeadUnit::Days },
            ..Default::default()
        };
        let key = format!("custom-{}", exam.id);
        p.custom_steps.push(exam);
        let mut db = Database {
            students: vec![Student {
                name: "Ana".into(),
                phd: Some(p),
                ..Default::default()
            }],
            ..Default::default()
        };
        let events = db.events();
        let titles: Vec<_> = events.iter().map(|e| (e.date, e.title.as_str())).collect();
        assert_eq!(
            titles,
            [
                (d(2027, 4, 20), "Reminder: MSc qualifying exam due 30 Apr 2027 – Ana"),
                (d(2027, 4, 30), "MSc qualifying exam due – Ana"),
            ]
        );
        assert!(db.attention(d(2027, 4, 19)).is_empty());
        assert_eq!(db.attention(d(2027, 4, 20)).len(), 1);

        // Completed through the step key, like the standard steps.
        db.students[0].phd.as_mut().unwrap().milestone_mut(&key).unwrap().done = true;
        assert!(db.attention(d(2027, 5, 1)).is_empty());

        // No reminder when the lead is 0.
        assert_eq!(Lead { amount: 0, unit: LeadUnit::Weeks }.before(d(2027, 4, 30)), None);
        assert_eq!(Lead::months(1).before(d(2027, 3, 31)), Some(d(2027, 2, 28)));
    }

    /// An external doctorate in Brazil started on 1 Mar 2026, with a CAPES
    /// grant and the default rules.
    fn example_brazilian() -> PhdRecord {
        let mut p = ResearchGroup::PhdExternal.new_record();
        p.set_brazilian(true);
        p.start = Some(d(2026, 3, 1));
        p.brazil.grant.agency = "CAPES".into();
        p.suggest_br(&BrRule::default_for(BrLevel::AcademicDoctorate));
        p
    }

    #[test]
    fn brazilian_programmes_suggest_their_own_stages() {
        let p = example_brazilian();
        assert!(p.brazilian_programme() && !p.skip_standard, "ticking brings in the stages");
        assert_eq!((p.brazil.level, p.group()), (BrLevel::AcademicDoctorate, ResearchGroup::PhdExternal));
        let deadlines: Vec<(String, Option<NaiveDate>)> =
            p.steps().iter().map(|s| (s.label.clone(), s.milestone.deadline)).collect();
        assert_eq!(
            deadlines,
            [
                ("Course credits", Some(d(2028, 3, 1))),
                ("Language proficiency", Some(d(2028, 3, 1))),
                ("Qualifying exam", Some(d(2028, 3, 1))),
                ("Annual report 2027", Some(d(2027, 3, 1))),
                ("Annual report 2028", Some(d(2028, 3, 1))),
                ("Annual report 2029", Some(d(2029, 3, 1))),
                ("Thesis to the committee", Some(d(2030, 2, 1))),
                ("Thesis defence", Some(d(2030, 3, 1))),
                ("Final version", None),
            ]
            .map(|(label, date)| (label.to_string(), date))
        );
        for step in p.steps() {
            assert!(p.clone().milestone_mut(&step.key).is_some(), "{}", step.key);
        }
        // CAPES: 48 months for a doctorate, ending the day before.
        assert_eq!(p.brazil.grant.months, 48);
        assert_eq!(p.brazil.grant.start, Some(d(2026, 3, 1)));
        assert_eq!(p.brazil.grant.end(), Some(d(2030, 2, 28)));

        // A parental-leave extension adds to the grant and to the course.
        let mut q = p.clone();
        q.brazil.grant.extension_months = 6;
        q.brazil.extension_months = 6;
        assert_eq!(q.brazil.grant.end(), Some(d(2030, 8, 31)));
        let rule = BrRule::default_for(q.brazil.level);
        assert_eq!(q.br_rule_deadline(&rule), Some(d(2030, 9, 1)));

        // Dates already set are kept.
        q.brazil.qualification.deadline = Some(d(2027, 12, 15));
        q.suggest_br(&rule);
        assert_eq!(q.brazil.qualification.deadline, Some(d(2027, 12, 15)));
        assert_eq!(q.annual_reports.len(), 3);
    }

    #[test]
    fn brazilian_level_picks_the_list_and_titles() {
        let mut p = ResearchGroup::MscExternal.new_record();
        p.set_brazilian(true);
        assert_eq!(p.brazil.level, BrLevel::AcademicMasters);
        p.set_br_level(BrLevel::ProfessionalMasters);
        assert_eq!((p.degree, p.group()), (ResearchDegree::MSc, ResearchGroup::MscExternal));
        assert_eq!((p.degree_label(), p.degree_short()), ("Professional master's", "Prof MSc"));
        let labels: Vec<String> = p.steps().into_iter().map(|s| s.label).collect();
        assert!(labels.contains(&"Final project defence".to_string()), "{labels:?}");
        p.set_br_level(BrLevel::AcademicDoctorate);
        assert_eq!(p.group(), ResearchGroup::PhdExternal);

        // The box only counts for external students; unticking goes back to
        // an external student without standard assessments.
        p.external = false;
        assert!(!p.brazilian_programme());
        assert_eq!(p.degree_label(), "PhD");
        assert!(p.steps().iter().any(|s| s.key == "initial"));
        p.external = true;
        p.set_brazilian(false);
        assert!(p.skip_standard);
        assert_eq!(p.steps().into_iter().map(|s| s.key).collect::<Vec<_>>(), ["amendments"]);
    }

    #[test]
    fn brazilian_events_grants_and_attention() {
        let mut p = example_brazilian();
        p.viva = Some(d(2030, 2, 20));
        let mut db = Database {
            students: vec![Student {
                name: "Ana".into(),
                phd: Some(p),
                ..Default::default()
            }],
            ..Default::default()
        };
        let id = db.students[0].id;
        let events = db.events();
        let title = |uid: String| events.iter().find(|e| e.uid == uid).map(|e| e.title.clone());
        assert_eq!(title(format!("phd-viva-{id}")).as_deref(), Some("PhD defence – Ana"));
        assert_eq!(title(format!("phd-defence-{id}")).as_deref(), Some("PhD thesis defence due – Ana"));
        assert_eq!(title(format!("phd-grant-end-{id}")).as_deref(), Some("PhD CAPES grant ends – Ana"));
        assert_eq!(
            title(format!("phd-qualification-reminder-{id}")).as_deref(),
            Some("Reminder: PhD qualifying exam due 01 Mar 2028 – Ana")
        );

        // Grant ending within 90 days, or ended in the last 30.
        let grant = |db: &Database, today| {
            db.attention(today)
                .into_iter()
                .filter(|a| a.message.contains("grant"))
                .map(|a| a.message)
                .collect::<Vec<_>>()
        };
        assert!(grant(&db, d(2029, 11, 29)).is_empty());
        assert_eq!(grant(&db, d(2029, 12, 1)), ["Ana – CAPES grant ends 2030-02-28"]);
        assert_eq!(grant(&db, d(2030, 3, 30)), ["Ana – CAPES grant ended 2030-02-28"]);
        assert!(grant(&db, d(2030, 4, 1)).is_empty());

        // The defence has a result once its own outcome is set.
        let viva = |db: &Database| db.events().into_iter().find(|e| e.uid == format!("phd-viva-{id}")).unwrap();
        assert!(!viva(&db).done);
        db.students[0].phd.as_mut().unwrap().brazil.outcome = DefenceOutcome::Approved;
        assert!(viva(&db).done);

        // Not on a Brazilian programme any more: no grant, a viva again.
        db.students[0].phd.as_mut().unwrap().brazilian = false;
        let events = db.events();
        assert!(events.iter().all(|e| !e.uid.starts_with("phd-grant-end")));
        assert!(events.iter().any(|e| e.title == "PhD viva – Ana"));
        assert!(grant(&db, d(2029, 12, 1)).is_empty());
    }

    #[test]
    fn brazilian_records_round_trip_and_older_files_load() {
        let mut db = Database::default();
        db.students.push(Student {
            phd: Some(example_brazilian()),
            ..Default::default()
        });
        let json = serde_json::to_string(&db).unwrap();
        assert_eq!(serde_json::from_str::<Database>(&json).unwrap(), db);

        // Files from before: no Brazilian fields, no rules in Settings.
        let json = r#"{"version":1,"settings":{"academic_year":"2026/27"},
            "students":[{"name":"Old","phd":{"external":true,"skip_standard":true}}]}"#;
        let db: Database = serde_json::from_str(json).unwrap();
        let p = db.students[0].phd.as_ref().unwrap();
        assert!(!p.brazilian && !p.brazilian_programme());
        assert_eq!(p.brazil, BrazilRecord::default());
        assert_eq!(db.settings.br_rules, default_br_rules());
        let settings = Settings {
            br_rules: Vec::new(),
            ..Default::default()
        };
        assert_eq!(settings.br_rule(BrLevel::ProfessionalDoctorate).qualification_months, 30);
    }

    #[test]
    fn external_students_have_no_visa_warnings() {
        let mut p = example_phd();
        p.visa_end = Some(d(2026, 12, 1));
        p.external = true;
        p.skip_standard = true;
        let db = Database {
            students: vec![Student {
                phd: Some(p),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(db.attention(d(2026, 11, 20)).is_empty());
    }
}
