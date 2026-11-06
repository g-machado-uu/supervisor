//! Reading students from a spreadsheet (Excel or LibreOffice): the
//! department's project allocations or its list of placement visits. The
//! students are on the first sheet; other sheets may list staff by name and
//! email, which fills in the emails the first sheet leaves out. Columns are
//! found by their headings, so their order does not matter, and the rows
//! that are yours are those with your email, or your name, in the
//! supervisor (or academic visitor) column.

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
    /// The row has your email, or your name, as the supervisor or academic
    /// visitor (a cell may hold several).
    pub fn is_mine(&self, my_email: &str, my_name: &str) -> bool {
        let mine = my_email.trim();
        let by_email = !mine.is_empty()
            && self
                .staff_email
                .split(|c: char| c == ';' || c == ',' || c == '/' || c.is_whitespace())
                .any(|e| e.eq_ignore_ascii_case(mine));
        by_email || staff_names(&self.staff_name).any(|n| same_person(n, my_name))
    }

    /// The supervisors (or visitors) of the row, by name, or by email when
    /// the sheet has no names.
    pub fn staff(&self) -> Vec<String> {
        let names: Vec<String> = staff_names(&self.staff_name).map(str::to_string).collect();
        if names.is_empty() {
            self.staff_email
                .split(|c: char| c == ';' || c == ',' || c == '/' || c.is_whitespace())
                .filter(|e| !e.is_empty())
                .map(str::to_string)
                .collect()
        } else {
            names
        }
    }

    /// Fills in the emails left empty from the people listed on other
    /// sheets. Returns how many were filled in.
    fn fill_emails(&mut self, directory: &[(String, String)]) -> usize {
        let mut filled = 0;
        for (name, email) in [
            (&self.staff_name, &mut self.staff_email),
            (&self.sust_supervisor_name, &mut self.sust_supervisor_email),
            (&self.second_marker_name, &mut self.second_marker_email),
            (&self.line_manager_name, &mut self.line_manager_email),
        ] {
            if !email.is_empty() {
                continue;
            }
            let found: Vec<&str> = staff_names(name).filter_map(|n| email_of(directory, n, &self.course)).collect();
            if !found.is_empty() {
                *email = found.join("; ");
                filled += 1;
            }
        }
        filled
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

/// Several names in one cell ("A / B", "A; B", "A and B"). A comma doesn't
/// part names: "Ralph, Dr Calvin" is one.
fn staff_names(cell: &str) -> impl Iterator<Item = &str> {
    static PARTS: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)\s*(?:[;/&\n]|\band\b)\s*").expect("valid pattern"));
    PARTS.split(cell).map(str::trim).filter(|n| !n.is_empty())
}

/// Anything in brackets, such as a department after a name.
static BRACKETS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\([^)]*\)|\[[^\]]*\]").expect("valid pattern"));

/// The words of a person's name as written, in lower case, without titles
/// or anything in brackets.
fn written_words(name: &str) -> Vec<String> {
    static TITLE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^(dr|prof|professor|mr|mrs|ms|miss|mx|sir|dame|rev)$").expect("valid pattern"));
    heading(&BRACKETS.replace_all(name, " "))
        .split(' ')
        .filter(|w| !w.is_empty() && !TITLE.is_match(w))
        .map(str::to_string)
        .collect()
}

/// The words of a person's name in any order (sorted), so that "Machado,
/// Dr Gabriel" and "Gabriel Machado (Mechanical Eng)" are the same.
fn name_words(name: &str) -> Vec<String> {
    let mut words = written_words(name);
    words.sort();
    words.dedup();
    words
}

/// The same person, whatever the order of the names, titles, capitals and
/// punctuation. A name with a middle name left out still matches, as long
/// as two names are the same, and so does one written without spaces
/// ("HuaXin" for "Hua Xin").
pub fn same_person(a: &str, b: &str) -> bool {
    let (wa, wb) = (name_words(a), name_words(b));
    let (short, long) = if wa.len() <= wb.len() { (&wa, &wb) } else { (&wb, &wa) };
    if short.is_empty() {
        return false;
    }
    if short == long || (short.len() >= 2 && short.iter().all(|w| long.contains(w))) {
        return true;
    }
    let (a, mut b) = (written_words(a), written_words(b));
    let joined = a.concat();
    joined.chars().count() >= 4 && (joined == b.concat() || (b.reverse(), joined == b.concat()).1)
}

/// A key for grouping the rows of one person.
pub fn name_key(name: &str) -> String {
    let words = name_words(name);
    if words.is_empty() { name.trim().to_lowercase() } else { words.join(" ") }
}

/// The email of `name` in the people listed on other sheets: those with
/// the same name exactly, or else those it can be. When they are several
/// people, the one whose department (in brackets after the name) is most
/// like the student's `course`; if that doesn't decide, none.
fn email_of<'a>(directory: &'a [(String, String)], name: &str, course: &str) -> Option<&'a str> {
    let words = name_words(name);
    if words.is_empty() {
        return None;
    }
    let exact: Vec<&(String, String)> = directory.iter().filter(|(n, _)| name_words(n) == words).collect();
    let found = if exact.is_empty() { directory.iter().filter(|(n, _)| same_person(n, name)).collect() } else { exact };
    let only = |found: &[&'a (String, String)]| {
        let first = &found.first()?.1;
        found.iter().all(|(_, e)| e.eq_ignore_ascii_case(first)).then_some(first.as_str())
    };
    only(&found).or_else(|| {
        let best = found.iter().map(|(n, _)| department_score(n, course)).max().filter(|&b| b > 0)?;
        only(&found.iter().filter(|(n, _)| department_score(n, course) == best).copied().collect::<Vec<_>>())
    })
}

/// How many words of the course the department in brackets after a name
/// has, a short word counting for a longer one ("Mech" for "Mechanical").
fn department_score(name: &str, course: &str) -> usize {
    let department: Vec<&str> = BRACKETS.find_iter(name).map(|m| m.as_str()).collect();
    let department = heading(&department.join(" "));
    let course = heading(course);
    course
        .split(' ')
        .filter(|w| !w.is_empty() && department.split(' ').any(|d| !d.is_empty() && (d.starts_with(w) || w.starts_with(d))))
        .count()
}

/// The people listed on the sheets, by name and email: under each heading
/// with "Name", the email in the nearest email column to its right.
/// Students' names and emails are left out.
fn directory(sheets: &[Vec<Vec<String>>]) -> Vec<(String, String)> {
    static NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bnames?\b").expect("valid pattern"));
    let mut people = Vec::new();
    for rows in sheets {
        for (at, row) in rows.iter().take(20).enumerate() {
            let hs: Vec<String> = row.iter().map(|c| heading(c)).collect();
            let student = |h: &str| h.contains("student");
            let is_email = |h: &str| EMAIL.is_match(h) && !student(h);
            let is_name = |h: &str| NAME.is_match(h) && !EMAIL.is_match(h) && !student(h);
            let pairs: Vec<(usize, usize)> = (0..hs.len())
                .filter(|&i| is_name(&hs[i]))
                .filter_map(|i| {
                    let j = (i + 1..hs.len()).find(|&j| is_email(&hs[j]) || is_name(&hs[j]))?;
                    is_email(&hs[j]).then_some((i, j))
                })
                .collect();
            if pairs.is_empty() {
                continue;
            }
            for row in &rows[at + 1..] {
                for &(i, j) in &pairs {
                    let cell = |k: usize| row.get(k).map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "));
                    if let (Some(name), Some(email)) = (cell(i), cell(j))
                        && !name.is_empty()
                        && email.contains('@')
                    {
                        people.push((name, email));
                    }
                }
            }
            break;
        }
    }
    people
}

/// The students of a sheet.
#[derive(Debug)]
pub struct Sheet {
    pub students: Vec<SheetStudent>,
    /// There is a column with the supervisor's (or visitor's) name or email.
    pub has_staff: bool,
    /// Emails filled in from the people listed on other sheets.
    pub emails_filled: usize,
}

/// The cells of every sheet of a workbook (.xlsx, .xlsm, .xls, .ods…), as
/// text. The first sheet must be readable; another that can't be read is
/// left out.
pub fn read_workbook(path: &Path) -> Result<Vec<Vec<Vec<String>>>, String> {
    let book = calamine::open_workbook_auto(path).map_err(|e| format!("This file can't be read as a spreadsheet ({e})"))?;
    all_sheets(book)
}

fn all_sheets<RS: Read + Seek>(mut book: Sheets<RS>) -> Result<Vec<Vec<Vec<String>>>, String> {
    let cells = |range: calamine::Range<Data>| range.rows().map(|row| row.iter().map(cell_text).collect()).collect();
    let first = book
        .worksheet_range_at(0)
        .ok_or("The spreadsheet has no sheets")?
        .map_err(|e| format!("The first sheet can't be read ({e})"))?;
    let mut sheets = vec![cells(first)];
    let count = book.sheet_names().len();
    sheets.extend((1..count).filter_map(|i| book.worksheet_range_at(i)?.ok()).map(cells));
    Ok(sheets)
}

/// The students of a workbook's first sheet, with the emails it leaves out
/// filled in from the people listed on the other sheets.
pub fn import(sheets: &[Vec<Vec<String>>]) -> Result<Sheet, String> {
    let (first, others) = sheets.split_first().ok_or("The spreadsheet has no sheets")?;
    let mut sheet = students(first)?;
    let people = directory(others);
    if !people.is_empty() {
        sheet.emails_filled = sheet.students.iter_mut().map(|s| s.fill_emails(&people)).sum();
    }
    Ok(sheet)
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
    Rule { field: Field::Course, email: false, all: &[r"^(course|programme|program|major)( (title|name))?$"] },
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
    let has_staff = cols.get(Field::StaffEmail).is_some() || cols.get(Field::Staff).is_some();
    Ok(Sheet { students, has_staff, emails_filled: 0 })
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
        assert!(sheet.has_staff);
        assert_eq!(sheet.students.len(), 3, "the blank row is skipped");
        let aisha = &sheet.students[0];
        assert_eq!(
            (aisha.b_number.as_str(), aisha.name.as_str(), aisha.project_title.as_str()),
            ("B00111111", "Aisha Khan", "Drone arm")
        );
        assert_eq!(sheet.students[2].name, "Ana Silva");
        let mine: Vec<&str> =
            sheet.students.iter().filter(|s| s.is_mine("me@ulster.ac.uk", "")).map(|s| s.b_number.as_str()).collect();
        assert_eq!(mine, ["B00111111", "B00333333"], "the email matches whatever its case and spaces");
        assert!(!sheet.students[0].is_mine("", ""), "with no email or name of your own, nothing is yours");
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
        assert!(s.is_mine("me@ulster.ac.uk", "") && !sheet.students[1].is_mine("me@ulster.ac.uk", ""));
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
        assert!(s.is_mine("other@ulster.ac.uk", ""), "one of several addresses");
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
        assert!(s.is_mine("me@ulster.ac.uk", ""), "the UU supervisor decides whose student it is");
        assert!(!s.is_mine("zhang@sust.edu.cn", "Dr Zhang"));
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
        assert!(!students(&rows(&[&["B Number", "Name"]])).unwrap().has_staff);
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

    /// The layout of the SUST allocations: names only on the first sheet,
    /// and the SUST and Ulster supervisors side by side on the second.
    fn sust_workbook() -> Vec<Vec<Vec<String>>> {
        vec![
            rows(&[
                &["", "Student ID", "Student names", "E-Mail", "Major", "Project Title", "Key words", "SUST Supervisor ", "UU supervisor"],
                &["1", "B01000001", "Wei Li", "li-w@ulster.ac.uk ", "Industrial design", "Solar still", "solar", "Dongxu Zhang", "Machado, Dr Gabriel"],
                &["2", "B01000002", "Fang Chen", "chen-f@ulster.ac.uk", "Industrial design", "Gearbox", "gears", "Tao Jiang", "Ralph, Dr Calvin"],
                &["3", "B01000003", "Min Wu", "wu-m@ulster.ac.uk", "Mechanical", "Drone", "uav", "Unlisted Person", "Gabriel Machado / Ralph, Dr Calvin"],
            ]),
            rows(&[
                &["SUST Supervisor", "", "", "", "", "Ulster Supervisor", ""],
                &["Name", "Email", "Key 1", "Key 2", "", "Name", "Email"],
                &["Dongxu Zhang (Mechanical Eng)", "zhangdongxu@sust.edu.cn ", "Simulation", "Design", "", "Machado, Dr Gabriel", "g.machado@ulster.ac.uk"],
                &["Jiang Tao (Design)", "jiangtao@sust.edu.cn", "", "", "", "Ralph, Dr Calvin", "c.ralph@ulster.ac.uk"],
            ]),
        ]
    }

    #[test]
    fn emails_left_out_are_found_by_name_on_other_sheets() {
        let sheet = import(&sust_workbook()).unwrap();
        assert!(sheet.has_staff);
        assert_eq!(sheet.emails_filled, 5);
        let [wei, fang, min] = &sheet.students[..] else { panic!("three students") };
        assert_eq!((wei.name.as_str(), wei.course.as_str()), ("Wei Li", "Industrial design"), "“Major” is the course");
        assert_eq!(wei.staff_email, "g.machado@ulster.ac.uk");
        assert_eq!(wei.sust_supervisor_email, "zhangdongxu@sust.edu.cn", "the department in brackets is ignored");
        assert_eq!(fang.sust_supervisor_email, "jiangtao@sust.edu.cn", "whatever the order of the names");
        assert_eq!(fang.staff_email, "c.ralph@ulster.ac.uk");
        assert_eq!(min.staff_email, "g.machado@ulster.ac.uk; c.ralph@ulster.ac.uk", "co-supervised");
        assert!(min.sust_supervisor_email.is_empty(), "not listed");
        let mine: Vec<&str> = sheet
            .students
            .iter()
            .filter(|s| s.is_mine("g.machado@ulster.ac.uk", ""))
            .map(|s| s.b_number.as_str())
            .collect();
        assert_eq!(mine, ["B01000001", "B01000003"]);
        assert_eq!(fang.staff(), ["Ralph, Dr Calvin"]);
        assert_eq!(min.staff(), ["Gabriel Machado", "Ralph, Dr Calvin"]);
    }

    #[test]
    fn your_name_finds_your_rows_when_the_sheet_has_no_emails() {
        let sheet = students(&sust_workbook()[0]).unwrap();
        assert!(sheet.has_staff && sheet.emails_filled == 0);
        let mine = |name: &str| -> Vec<&str> {
            sheet.students.iter().filter(|s| s.is_mine("g.machado@ulster.ac.uk", name)).map(|s| s.b_number.as_str()).collect()
        };
        assert_eq!(mine("Gabriel Machado"), ["B01000001", "B01000003"]);
        assert_eq!(mine("Dr. Gabriel MACHADO"), ["B01000001", "B01000003"]);
        assert_eq!(mine("Gabriel Silva Machado"), ["B01000001", "B01000003"], "a middle name left out");
        assert!(mine("Gabriel").is_empty(), "a first name alone is not enough");
        assert!(mine("").is_empty());
        assert!(mine("Dongxu Zhang").is_empty(), "the SUST supervisor is never you");
    }

    #[test]
    fn names_are_compared_without_titles_order_or_brackets() {
        assert!(same_person("Machado, Dr Gabriel", "Gabriel Machado"));
        assert!(same_person("Prof. Jo Bloggs (Mech)", "bloggs jo"));
        assert!(!same_person("Jo Bloggs", "Jo Smith"));
        assert!(!same_person("Dr", "Dr"), "a title is not a name");
        assert_eq!(name_key("Machado, Dr Gabriel"), name_key("GABRIEL MACHADO"));
        let people = vec![
            ("Jo Bloggs".to_string(), "jo@a.com".to_string()),
            ("Jo Bloggs Junior".to_string(), "junior@a.com".to_string()),
        ];
        assert_eq!(email_of(&people, "Bloggs, Jo", ""), Some("jo@a.com"), "the exact name first");
        assert_eq!(email_of(&people, "Jo Junior Bloggs", ""), Some("junior@a.com"));
        assert_eq!(email_of(&people, "Jo Smith", ""), None);
        assert!(same_person("HuaXin", "Hua Xin (Mechanical Eng)") && same_person("xinhua", "Hua Xin"));
        assert!(!same_person("Hu", "H U"), "too short to tell");

        // Two people of the same name, told apart by their department.
        let twins = vec![
            ("Meng Zhang (Mechanical Eng)".to_string(), "mech@a.com".to_string()),
            ("Zhang Meng (Industrial Design)".to_string(), "design@a.com".to_string()),
        ];
        assert_eq!(email_of(&twins, "Zhang Meng", "Mech Eng"), Some("mech@a.com"));
        assert_eq!(email_of(&twins, "Zhang Meng", "Industrial design"), Some("design@a.com"));
        assert_eq!(email_of(&twins, "Zhang Meng", "Physics"), None, "neither is guessed");
        assert_eq!(email_of(&twins, "Zhang Meng", ""), None);
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
    fn students_come_from_the_first_sheet_only() {
        let bytes = xlsx(&[&["Name", "Ana Silva"], &["Name", "Sean Murphy"]]);
        let book = calamine::open_workbook_auto_from_rs(std::io::Cursor::new(bytes)).unwrap();
        let sheets = all_sheets(book).unwrap();
        assert_eq!(sheets, [rows(&[&["Name"], &["Ana Silva"]]), rows(&[&["Name"], &["Sean Murphy"]])]);
        let names: Vec<String> = import(&sheets).unwrap().students.into_iter().map(|s| s.name).collect();
        assert_eq!(names, ["Ana Silva"]);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("list.xlsx");
        std::fs::write(&path, xlsx(&[&["B Number", "B00111111"]])).unwrap();
        assert_eq!(import(&read_workbook(&path).unwrap()).unwrap().students[0].b_number, "B00111111");
        std::fs::write(&path, b"not a spreadsheet").unwrap();
        assert!(read_workbook(&path).unwrap_err().contains("can't be read as a spreadsheet"));
    }}
