//! Reading students from a spreadsheet (Excel or LibreOffice): the
//! department's project allocations or its list of placement visits. Only
//! the first sheet is read. Columns are found by their headings, so their
//! order does not matter, and the rows that are yours are those with your
//! email in the supervisor (or academic visitor) column.

use std::io::{Read, Seek};
use std::path::Path;
use std::sync::LazyLock;

use calamine::{Data, Reader, Sheets};
use regex::Regex;
use uuid::Uuid;

use crate::model::{Database, FypRecord, PlacementRecord, Student};

/// The list the students are imported into.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SheetKind {
    Projects,
    Placement,
}

impl SheetKind {
    /// The member of staff a row belongs to, as the sheet calls them.
    pub fn staff(self) -> &'static str {
        match self {
            SheetKind::Projects => "supervisor",
            SheetKind::Placement => "academic visitor",
        }
    }
}

/// One student row of the sheet. Fields the sheet has no column for are empty.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct SheetStudent {
    pub b_number: String,
    pub name: String,
    pub email: String,
    /// A SUST student's own SUST email.
    pub sust_email: String,
    /// A placement student's email at the company.
    pub professional_email: String,
    pub course: String,
    pub project_title: String,
    pub company: String,
    /// The supervisor (at this university) or academic visitor.
    pub staff_name: String,
    pub staff_email: String,
    pub sust_supervisor_name: String,
    pub sust_supervisor_email: String,
    pub second_marker_name: String,
    pub second_marker_email: String,
    pub line_manager_name: String,
    pub line_manager_email: String,
}

impl SheetStudent {
    /// The row has your email as the supervisor or academic visitor (a cell
    /// may hold several addresses).
    pub fn is_mine(&self, my_email: &str) -> bool {
        let mine = my_email.trim();
        !mine.is_empty()
            && self
                .staff_email
                .split(|c: char| c == ';' || c == ',' || c == '/' || c.is_whitespace())
                .any(|e| e.eq_ignore_ascii_case(mine))
    }

    pub fn matches(&self, query: &str) -> bool {
        let q = query.trim().to_lowercase();
        q.is_empty()
            || [&self.b_number, &self.name, &self.project_title, &self.company, &self.staff_name, &self.staff_email]
                .iter()
                .any(|f| f.to_lowercase().contains(&q))
    }

    /// The other people and addresses on the row, one line each.
    pub fn details(&self) -> Vec<String> {
        let person = |name: &str, email: &str| match (name.is_empty(), email.is_empty()) {
            (_, true) => name.to_string(),
            (true, false) => email.to_string(),
            (false, false) => format!("{name} ({email})"),
        };
        [
            ("SUST email", self.sust_email.clone()),
            ("Professional email", self.professional_email.clone()),
            ("SUST supervisor", person(&self.sust_supervisor_name, &self.sust_supervisor_email)),
            ("Second marker", person(&self.second_marker_name, &self.second_marker_email)),
            ("Line manager", person(&self.line_manager_name, &self.line_manager_email)),
        ]
        .into_iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(what, v)| format!("{what}: {v}"))
        .collect()
    }
}

/// The students of a sheet.
#[derive(Debug)]
pub struct Sheet {
    pub students: Vec<SheetStudent>,
    /// There is a column with the supervisor's (or visitor's) email.
    pub has_staff_email: bool,
}

/// The cells of the first sheet of a workbook (.xlsx, .xlsm, .xls, .ods…),
/// as text.
pub fn read_first_sheet(path: &Path) -> Result<Vec<Vec<String>>, String> {
    let book = calamine::open_workbook_auto(path).map_err(|e| format!("This file can't be read as a spreadsheet ({e})"))?;
    first_sheet(book)
}

fn first_sheet<RS: Read + Seek>(mut book: Sheets<RS>) -> Result<Vec<Vec<String>>, String> {
    let range = book
        .worksheet_range_at(0)
        .ok_or("The spreadsheet has no sheets")?
        .map_err(|e| format!("The first sheet can't be read ({e})"))?;
    Ok(range.rows().map(|row| row.iter().map(cell_text).collect()).collect())
}

fn cell_text(cell: &Data) -> String {
    match cell {
        Data::Empty | Data::Error(_) => String::new(),
        Data::String(s) => s.trim().to_string(),
        // Numbers typed into a cell come back as floats: 12345678.0 shows as 12345678.
        other => other.to_string(),
    }
}

/// What a column holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Field {
    Number,
    Name,
    First,
    Last,
    Email,
    SustEmail,
    ProfessionalEmail,
    Course,
    Title,
    Company,
    Staff,
    StaffEmail,
    SustSupervisor,
    SustSupervisorEmail,
    SecondMarker,
    SecondMarkerEmail,
    LineManager,
    LineManagerEmail,
}

const FIELDS: usize = Field::LineManagerEmail as usize + 1;

/// A heading holds `field` when it matches every pattern of `all`, and is
/// (or isn't, as `email` says) about an email address. The patterns see
/// the heading in lower case, with anything but letters and digits as one
/// space: "UU Supervisor E-Mail" is seen as "uu supervisor e mail".
struct Rule {
    field: Field,
    email: bool,
    all: &'static [&'static str],
}

const SUST: &str = r"\bsust\b";
const SUPERVISOR: &str = r"\b(supervisor|tutor)s?\b";
const STAFF: &str = r"\b(supervisor|tutor|visitor)s?\b";
const SECOND_MARKER: &str = r"\b(second|2nd) markers?\b";
const MANAGER: &str = r"\b(manager|mentor)s?\b";

/// The rules in order: a heading holds the field of the first rule it
/// meets, so "SUST Supervisor" is caught before the plain "Supervisor".
/// When several headings hold the same field, the leftmost column is used.
const RULES: &[Rule] = &[
    Rule { field: Field::SustSupervisorEmail, email: true, all: &[SUST, SUPERVISOR] },
    Rule { field: Field::SustSupervisor, email: false, all: &[SUST, SUPERVISOR] },
    Rule { field: Field::SecondMarkerEmail, email: true, all: &[SECOND_MARKER] },
    Rule { field: Field::SecondMarker, email: false, all: &[SECOND_MARKER] },
    Rule { field: Field::LineManagerEmail, email: true, all: &[MANAGER] },
    Rule { field: Field::LineManager, email: false, all: &[MANAGER] },
    // The supervisor here ("Supervisor", "UU Supervisor") or academic visitor.
    Rule { field: Field::StaffEmail, email: true, all: &[STAFF] },
    Rule { field: Field::Staff, email: false, all: &[STAFF] },
    Rule { field: Field::SustEmail, email: true, all: &[SUST] },
    Rule { field: Field::ProfessionalEmail, email: true, all: &[r"\b(professional|work)\b"] },
    // Any other email is the student's own.
    Rule { field: Field::Email, email: true, all: &[] },
    Rule {
        field: Field::Number,
        email: false,
        all: &[r"^(b ?(number|no|num)|student (number|no|num|id)|reg(istration)?( (number|no|num))?|regno)$"],
    },
    Rule { field: Field::Name, email: false, all: &[r"^(student )?(full )?names?$"] },
    Rule { field: Field::First, email: false, all: &[r"\b(first|given|fore) ?names?\b"] },
    Rule { field: Field::Last, email: false, all: &[r"\b(last|family|sur) ?names?\b"] },
    Rule { field: Field::Title, email: false, all: &[r"^title$|\bproject (title|name)\b"] },
    Rule { field: Field::Course, email: false, all: &[r"^(course|programme|program)( (title|name))?$"] },
    Rule {
        field: Field::Company,
        email: false,
        all: &[r"^(placement|company|employer|organi[sz]ation)( (company|provider|organi[sz]ation|name))?$"],
    },
];

/// `RULES` with their patterns compiled.
static COMPILED: LazyLock<Vec<(Field, bool, Vec<Regex>)>> = LazyLock::new(|| {
    RULES
        .iter()
        .map(|r| (r.field, r.email, r.all.iter().map(|p| Regex::new(p).expect("valid pattern")).collect()))
        .collect()
});

static EMAIL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\be ?mail\b").expect("valid pattern"));

/// Lower case, with anything but letters and digits as one space.
fn heading(text: &str) -> String {
    let spaced: String = text.chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect();
    spaced.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// The field a heading holds, if any.
fn field_of(text: &str) -> Option<Field> {
    let h = heading(text);
    if h.is_empty() {
        return None;
    }
    let email = EMAIL.is_match(&h);
    COMPILED
        .iter()
        .find(|(_, e, all)| *e == email && all.iter().all(|re| re.is_match(&h)))
        .map(|(field, _, _)| *field)
}

/// Which column holds each field.
#[derive(Debug)]
struct Columns([Option<usize>; FIELDS]);

impl Columns {
    fn of(headings: &[String]) -> Self {
        let mut c = [None; FIELDS];
        for (i, h) in headings.iter().enumerate() {
            if let Some(field) = field_of(h) {
                c[field as usize].get_or_insert(i);
            }
        }
        Columns(c)
    }

    fn get(&self, field: Field) -> Option<usize> {
        self.0[field as usize]
    }

    fn found(&self) -> bool {
        let has = |f| self.get(f).is_some();
        has(Field::Number) || has(Field::Name) || (has(Field::First) && has(Field::Last))
    }
}

/// "Last, First" becomes "First Last"; other names stay as they are.
fn natural_name(name: &str) -> String {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    match name.split_once(',') {
        Some((last, first)) if !first.contains(',') && !last.trim().is_empty() && !first.trim().is_empty() => {
            format!("{} {}", first.trim(), last.trim())
        }
        _ => name,
    }
}

/// The students of a sheet's rows. The headings are the first row (within
/// the first few) naming the student number or name columns.
pub fn students(rows: &[Vec<String>]) -> Result<Sheet, String> {
    let (at, cols) = rows
        .iter()
        .take(20)
        .enumerate()
        .map(|(i, r)| (i, Columns::of(r)))
        .find(|(_, c)| c.found())
        .ok_or(
            "No column headings were found for the student number or name. The sheet's first row should have \
             headings such as “B Number” and “Name”.",
        )?;
    let get = |row: &[String], field: Field| {
        cols.get(field)
            .and_then(|i| row.get(i))
            .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
            .unwrap_or_default()
    };
    let students = rows[at + 1..]
        .iter()
        .filter_map(|row| {
            let full = natural_name(&get(row, Field::Name));
            let name = if full.is_empty() {
                format!("{} {}", get(row, Field::First), get(row, Field::Last)).trim().to_string()
            } else {
                full
            };
            let s = SheetStudent {
                b_number: get(row, Field::Number),
                name,
                email: get(row, Field::Email),
                sust_email: get(row, Field::SustEmail),
                professional_email: get(row, Field::ProfessionalEmail),
                course: get(row, Field::Course),
                project_title: get(row, Field::Title),
                company: get(row, Field::Company),
                staff_name: get(row, Field::Staff),
                staff_email: get(row, Field::StaffEmail),
                sust_supervisor_name: get(row, Field::SustSupervisor),
                sust_supervisor_email: get(row, Field::SustSupervisorEmail),
                second_marker_name: get(row, Field::SecondMarker),
                second_marker_email: get(row, Field::SecondMarkerEmail),
                line_manager_name: get(row, Field::LineManager),
                line_manager_email: get(row, Field::LineManagerEmail),
            };
            (!s.b_number.is_empty() || !s.name.is_empty()).then_some(s)
        })
        .collect();
    Ok(Sheet { students, has_staff_email: cols.get(Field::StaffEmail).is_some() })
}

/// A student of the sheet who is in Supervisor already.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Existing {
    pub id: Uuid,
    /// Already in this list: nothing to add.
    pub has_role: bool,
    pub archived: bool,
}

fn same_number(a: &str, b: &str) -> bool {
    let a: String = a.chars().filter(|c| !c.is_whitespace()).collect();
    let b: String = b.chars().filter(|c| !c.is_whitespace()).collect();
    !a.is_empty() && a.eq_ignore_ascii_case(&b)
}

/// The student in Supervisor with the same B number, or else the same email.
pub fn existing(db: &Database, s: &SheetStudent, kind: SheetKind) -> Option<Existing> {
    let email = s.email.trim();
    let found = db.students.iter().find(|x| same_number(&x.b_number, &s.b_number)).or_else(|| {
        (!email.is_empty()).then(|| db.students.iter().find(|x| x.email.trim().eq_ignore_ascii_case(email))).flatten()
    })?;
    Some(Existing {
        id: found.id,
        has_role: match kind {
            SheetKind::Projects => found.fyp.is_some(),
            SheetKind::Placement => found.placement.is_some(),
        },
        archived: found.archived,
    })
}

/// The same student as another row of the sheet (same B number).
pub fn same_student(a: &SheetStudent, b: &SheetStudent) -> bool {
    same_number(&a.b_number, &b.b_number)
}

/// A ticked row: the student, and for projects the course (`Programme::id`).
#[derive(Clone, Debug)]
pub struct Choice {
    pub student: SheetStudent,
    pub programme: String,
}

/// How many students were added, and how many already in Supervisor were
/// added to this list.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Added {
    pub new: usize,
    pub existing: usize,
}

/// Adds the students, as project or placement students. One already in
/// Supervisor gets the role (and is no longer archived), and the sheet fills
/// in details left empty; one already in this list is left alone.
///
/// A SUST student's SUST supervisor goes where other students have their
/// second marker.
pub fn add_students(db: &mut Database, kind: SheetKind, choices: Vec<Choice>) -> Added {
    let mut added = Added::default();
    for Choice { student: row, programme } in choices {
        let sust_supervisor = !row.sust_supervisor_name.is_empty() || !row.sust_supervisor_email.is_empty();
        let (marker_name, marker_email) = if db.settings.is_sust(&programme) && sust_supervisor {
            (&row.sust_supervisor_name, &row.sust_supervisor_email)
        } else {
            (&row.second_marker_name, &row.second_marker_email)
        };
        let fyp = || FypRecord {
            programme: programme.clone(),
            project_title: row.project_title.clone(),
            sust_email: row.sust_email.clone(),
            second_marker_name: marker_name.clone(),
            second_marker_email: marker_email.clone(),
            ..Default::default()
        };
        let placement = || PlacementRecord {
            company: row.company.clone(),
            professional_email: row.professional_email.clone(),
            line_manager_name: row.line_manager_name.clone(),
            line_manager_email: row.line_manager_email.clone(),
            ..Default::default()
        };
        match existing(db, &row, kind) {
            Some(e) if e.has_role => {}
            Some(e) => {
                let s = db.student_mut(e.id).expect("found above");
                for (field, value) in [
                    (&mut s.b_number, &row.b_number),
                    (&mut s.name, &row.name),
                    (&mut s.email, &row.email),
                    (&mut s.course, &row.course),
                ] {
                    if field.trim().is_empty() {
                        field.clone_from(value);
                    }
                }
                match kind {
                    SheetKind::Projects => s.fyp = Some(fyp()),
                    SheetKind::Placement => s.placement = Some(placement()),
                }
                s.archived = false;
                added.existing += 1;
            }
            None => {
                let mut s = Student {
                    b_number: row.b_number.clone(),
                    name: row.name.clone(),
                    email: row.email.clone(),
                    course: row.course.clone(),
                    ..Default::default()
                };
                match kind {
                    SheetKind::Projects => s.fyp = Some(fyp()),
                    SheetKind::Placement => s.placement = Some(placement()),
                }
                db.students.push(s);
                added.new += 1;
            }
        }
    }
    added
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(cells: &[&[&str]]) -> Vec<Vec<String>> {
        cells.iter().map(|r| r.iter().map(|c| c.to_string()).collect()).collect()
    }

    /// The layout of the project allocations sheet.
    fn allocations() -> Vec<Vec<String>> {
        rows(&[
            &["B Number", "Name", "Project Title", "Supervisor Email", "Supervisor Name"],
            &["B00111111", "Khan, Aisha", "Drone arm", "me@ulster.ac.uk", "Me"],
            &["B00222222", "Murphy, Sean", "Wind tunnel", "other@ulster.ac.uk", "Other"],
            &["", "", "", "", ""],
            &["B00333333", "Silva,  Ana", "Heat pipes", " ME@Ulster.ac.uk ", "Me"],
        ])
    }

    /// The layout of the placement visits sheet, with a stray cell below.
    fn visits() -> Vec<Vec<String>> {
        rows(&[
            &[
                "Registration", "First Name", "Last Name", "Course Code", " Course", "E-Mail", "Placement", "Address",
                "Academic visitors", "Academic visitors email",
            ],
            &[
                "B00444444", "Test", "Student", "ignore", "BEng Mech", "s1@ulster.ac.uk", "Acme Ltd", "Ignore",
                "Me", "me@ulster.ac.uk",
            ],
            &[
                "B00555555", "Test2", "Student2", "ignore", "MEng Mech", "s2@ulster.ac.uk", "Widgets plc", "Ignore",
                "Other", "other@ulster.ac.uk",
            ],
            &["", "", "", "", "", "", "", "", "", "", "Exempt"],
        ])
    }

    #[test]
    fn project_allocations_are_read_by_their_headings() {
        let sheet = students(&allocations()).unwrap();
        assert!(sheet.has_staff_email);
        assert_eq!(sheet.students.len(), 3, "the blank row is skipped");
        let aisha = &sheet.students[0];
        assert_eq!(
            (aisha.b_number.as_str(), aisha.name.as_str(), aisha.project_title.as_str()),
            ("B00111111", "Aisha Khan", "Drone arm")
        );
        assert_eq!(sheet.students[2].name, "Ana Silva");
        let mine: Vec<&str> =
            sheet.students.iter().filter(|s| s.is_mine("me@ulster.ac.uk")).map(|s| s.b_number.as_str()).collect();
        assert_eq!(mine, ["B00111111", "B00333333"], "the email matches whatever its case and spaces");
        assert!(!sheet.students[0].is_mine(""), "with no email of your own, nothing is yours");
    }

    #[test]
    fn placement_visits_are_read_by_their_headings() {
        let sheet = students(&visits()).unwrap();
        assert_eq!(sheet.students.len(), 2, "a row with neither number nor name is skipped");
        let s = &sheet.students[0];
        assert_eq!(s.name, "Test Student");
        assert_eq!(s.email, "s1@ulster.ac.uk");
        assert_eq!(s.course, "BEng Mech", "the course name, not its code");
        assert_eq!(s.company, "Acme Ltd");
        assert_eq!((s.staff_name.as_str(), s.staff_email.as_str()), ("Me", "me@ulster.ac.uk"));
        assert!(s.is_mine("me@ulster.ac.uk") && !sheet.students[1].is_mine("me@ulster.ac.uk"));
    }

    #[test]
    fn headings_may_be_below_a_title_and_columns_in_any_order() {
        let sheet = students(&rows(&[
            &["Allocations 2026/27"],
            &[],
            &["Supervisor Email", "Student Name", "Student Number"],
            &["me@ulster.ac.uk; other@ulster.ac.uk", "Ana Silva", "B00333333"],
        ]))
        .unwrap();
        let s = &sheet.students[0];
        assert_eq!((s.b_number.as_str(), s.name.as_str()), ("B00333333", "Ana Silva"));
        assert!(s.is_mine("other@ulster.ac.uk"), "one of several addresses");
    }

    #[test]
    fn headings_are_recognised_by_their_words() {
        let cases = [
            ("B Number", Some(Field::Number)),
            ("B-No.", Some(Field::Number)),
            ("Registration", Some(Field::Number)),
            ("Reg. No", Some(Field::Number)),
            ("Student ID", Some(Field::Number)),
            ("Student Name", Some(Field::Name)),
            ("Forename", Some(Field::First)),
            ("Surname", Some(Field::Last)),
            ("E-Mail", Some(Field::Email)),
            ("UU Email Address", Some(Field::Email)),
            ("SUST e-mail", Some(Field::SustEmail)),
            ("Supervisor", Some(Field::Staff)),
            ("UU Supervisor", Some(Field::Staff)),
            ("UU Supervisor Email", Some(Field::StaffEmail)),
            ("Academic visitors email", Some(Field::StaffEmail)),
            ("SUST Supervisor", Some(Field::SustSupervisor)),
            ("Supervisor (SUST) E-mail", Some(Field::SustSupervisorEmail)),
            ("2nd Marker", Some(Field::SecondMarker)),
            ("Second marker email", Some(Field::SecondMarkerEmail)),
            ("Line Manager", Some(Field::LineManager)),
            ("Line manager e-mail", Some(Field::LineManagerEmail)),
            ("Professional Email", Some(Field::ProfessionalEmail)),
            ("Project Title", Some(Field::Title)),
            (" Course", Some(Field::Course)),
            ("Placement", Some(Field::Company)),
            ("Placement Provider", Some(Field::Company)),
            // Not about the student, or not wanted.
            ("Course Code", None),
            ("Address", None),
            ("Placement Address", None),
            ("Academic Year", None),
            ("", None),
        ];
        for (text, want) in cases {
            assert_eq!(field_of(text), want, "heading {text:?}");
        }
    }

    #[test]
    fn a_sust_supervisor_is_kept_apart_from_your_own() {
        let cells = rows(&[
            &["B Number", "Name", "Project Title", "UU Supervisor Email", "SUST Supervisor", "SUST Supervisor Email", "SUST Email"],
            &["B00666666", "Li, Wei", "Solar still", "me@ulster.ac.uk", "Dr Zhang", "zhang@sust.edu.cn", "wei@sust.edu.cn"],
        ]);
        let sheet = students(&cells).unwrap();
        let s = &sheet.students[0];
        assert!(s.is_mine("me@ulster.ac.uk"), "the UU supervisor decides whose student it is");
        assert!(!s.is_mine("zhang@sust.edu.cn"));
        assert_eq!(s.details(), ["SUST email: wei@sust.edu.cn", "SUST supervisor: Dr Zhang (zhang@sust.edu.cn)"]);

        let mut db = Database::default();
        let choice = |programme: &str| Choice { student: s.clone(), programme: programme.into() };
        add_students(&mut db, SheetKind::Projects, vec![choice("Sust")]);
        let fyp = db.students[0].fyp.as_ref().unwrap();
        assert_eq!(
            (fyp.second_marker_name.as_str(), fyp.second_marker_email.as_str(), fyp.sust_email.as_str()),
            ("Dr Zhang", "zhang@sust.edu.cn", "wei@sust.edu.cn"),
            "the SUST supervisor fills the field of the SUST supervisor"
        );
        // Not on the SUST course: the SUST supervisor isn't a second marker.
        db.students.clear();
        add_students(&mut db, SheetKind::Projects, vec![choice("BEng")]);
        assert!(db.students[0].fyp.as_ref().unwrap().second_marker_name.is_empty());
    }

    #[test]
    fn second_markers_and_line_managers_are_filled_in() {
        let cells = rows(&[
            &["B Number", "Name", "Supervisor Email", "Second Marker", "Second Marker Email"],
            &["B00111111", "Ana Silva", "me@ulster.ac.uk", "Dr Murphy", "murphy@ulster.ac.uk"],
        ]);
        let mut db = Database::default();
        let student = students(&cells).unwrap().students.remove(0);
        add_students(&mut db, SheetKind::Projects, vec![Choice { student, programme: "BEng".into() }]);
        let fyp = db.students[0].fyp.as_ref().unwrap();
        assert_eq!((fyp.second_marker_name.as_str(), fyp.second_marker_email.as_str()), ("Dr Murphy", "murphy@ulster.ac.uk"));

        let cells = rows(&[
            &["Registration", "Name", "Placement", "Line Manager", "Line Manager Email", "Professional Email"],
            &["B00222222", "Sean Murphy", "Acme", "Jo Bloggs", "jo@acme.com", "sean@acme.com"],
        ]);
        let mut db = Database::default();
        let student = students(&cells).unwrap().students.remove(0);
        add_students(&mut db, SheetKind::Placement, vec![Choice { student, programme: String::new() }]);
        let p = db.students[0].placement.as_ref().unwrap();
        assert_eq!(
            (p.line_manager_name.as_str(), p.line_manager_email.as_str(), p.professional_email.as_str()),
            ("Jo Bloggs", "jo@acme.com", "sean@acme.com")
        );
    }

    #[test]
    fn a_sheet_without_student_headings_is_refused() {
        let err = students(&rows(&[&["Foo", "Bar"], &["1", "2"]])).unwrap_err();
        assert!(err.contains("B Number"));
        assert!(!students(&rows(&[&["B Number", "Name"]])).unwrap().has_staff_email);
    }

    #[test]
    fn adding_creates_students_or_gives_the_role_to_those_already_there() {
        let mut db = Database::default();
        // Already an advisee (archived, last year), and already a project student.
        db.students.push(Student {
            b_number: "b00111111".into(),
            name: "Aisha Khan".into(),
            archived: true,
            advisee: Some(Default::default()),
            ..Default::default()
        });
        db.students.push(Student {
            b_number: "B00333333".into(),
            fyp: Some(FypRecord { project_title: "Old".into(), ..Default::default() }),
            ..Default::default()
        });
        let sheet = students(&allocations()).unwrap();
        let kind = SheetKind::Projects;
        let found: Vec<Option<(bool, bool)>> =
            sheet.students.iter().map(|s| existing(&db, s, kind).map(|e| (e.has_role, e.archived))).collect();
        assert_eq!(found, [Some((false, true)), None, Some((true, false))]);

        let choices = sheet
            .students
            .into_iter()
            .map(|student| Choice { student, programme: "MEng".into() })
            .collect();
        assert_eq!(add_students(&mut db, kind, choices), Added { new: 1, existing: 1 });
        assert_eq!(db.students.len(), 3);
        let aisha = &db.students[0];
        assert!(!aisha.archived && aisha.advisee.is_some());
        let fyp = aisha.fyp.as_ref().unwrap();
        assert_eq!((fyp.programme.as_str(), fyp.project_title.as_str()), ("MEng", "Drone arm"));
        assert_eq!(db.students[1].fyp.as_ref().unwrap().project_title, "Old", "left alone");
        let sean = &db.students[2];
        assert_eq!((sean.name.as_str(), sean.b_number.as_str()), ("Sean Murphy", "B00222222"));
    }

    #[test]
    fn placement_students_get_their_company_email_and_course() {
        let mut db = Database::default();
        db.students.push(Student { email: "S1@ulster.ac.uk".into(), ..Default::default() });
        let sheet = students(&visits()).unwrap();
        let choices = sheet.students.into_iter().map(|student| Choice { student, programme: String::new() }).collect();
        assert_eq!(add_students(&mut db, SheetKind::Placement, choices), Added { new: 1, existing: 1 });
        let matched = &db.students[0];
        assert_eq!(matched.b_number, "B00444444", "found by email, and the empty number filled in");
        assert_eq!(matched.placement.as_ref().unwrap().company, "Acme Ltd");
        let new = &db.students[1];
        assert_eq!((new.course.as_str(), new.email.as_str()), ("MEng Mech", "s2@ulster.ac.uk"));
        assert_eq!(new.placement.as_ref().unwrap().company, "Widgets plc");
    }

    /// A minimal .xlsx with one sheet per entry of `sheets`, each a column
    /// of inline strings.
    fn xlsx(sheets: &[&[&str]]) -> Vec<u8> {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let mut file = |name: &str, body: String| {
            zip.start_file(name, opts).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        };
        let n = sheets.len();
        let overrides: String = (1..=n)
            .map(|i| format!(r#"<Override PartName="/xl/worksheets/sheet{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#))
            .collect();
        file("[Content_Types].xml", format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>{overrides}</Types>"#
        ));
        file("_rels/.rels", r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#.into());
        let entries: String = (1..=n).map(|i| format!(r#"<sheet name="Sheet{i}" sheetId="{i}" r:id="rId{i}"/>"#)).collect();
        file("xl/workbook.xml", format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>{entries}</sheets></workbook>"#
        ));
        let rels: String = (1..=n)
            .map(|i| format!(r#"<Relationship Id="rId{i}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{i}.xml"/>"#))
            .collect();
        file("xl/_rels/workbook.xml.rels", format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{rels}</Relationships>"#
        ));
        for (i, cells) in sheets.iter().enumerate() {
            let rows: String = cells
                .iter()
                .enumerate()
                .map(|(r, text)| format!(r#"<row r="{0}"><c r="A{0}" t="inlineStr"><is><t>{text}</t></is></c></row>"#, r + 1))
                .collect();
            file(&format!("xl/worksheets/sheet{}.xml", i + 1), format!(
                r#"<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>{rows}</sheetData></worksheet>"#
            ));
        }
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn only_the_first_sheet_is_read() {
        let bytes = xlsx(&[&["Name", "Ana Silva"], &["Name", "Sean Murphy"]]);
        let book = calamine::open_workbook_auto_from_rs(std::io::Cursor::new(bytes)).unwrap();
        let cells = first_sheet(book).unwrap();
        assert_eq!(cells, rows(&[&["Name"], &["Ana Silva"]]));

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("list.xlsx");
        std::fs::write(&path, xlsx(&[&["B Number", "B00111111"]])).unwrap();
        assert_eq!(students(&read_first_sheet(&path).unwrap()).unwrap().students[0].b_number, "B00111111");
        std::fs::write(&path, b"not a spreadsheet").unwrap();
        assert!(read_first_sheet(&path).unwrap_err().contains("can't be read as a spreadsheet"));
    }
}
