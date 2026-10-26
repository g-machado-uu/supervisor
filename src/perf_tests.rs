//! Timings for the per-frame and save paths, on a realistic dataset.
//! Run with: cargo test --release perf_ -- --ignored --nocapture

use std::time::Instant;

use chrono::{Duration, NaiveDate, NaiveTime};

use crate::model::*;
use crate::storage::{self, Vault};

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

fn notes(n: usize) -> String {
    "Discussed progress on the literature review and next steps. ".repeat(n)
}

/// A repeating meeting with `held` weeks already marked, each with notes.
fn repeating(start: NaiveDate, every: u8, until: Option<NaiveDate>, held: usize) -> Meeting {
    let exceptions = (0..held)
        .map(|w| Occurrence {
            planned: start + Duration::weeks(w as i64 * i64::from(every)),
            held: true,
            notes: notes(2),
            ..Default::default()
        })
        .collect();
    Meeting {
        date: Some(start),
        time: NaiveTime::from_hms_opt(14, 0, 0),
        location: "MG.201".into(),
        repeat: Some(Repeat { every_weeks: every, until, exceptions }),
        notes: notes(1),
        ..Default::default()
    }
}

/// One academic year: 40 project, 6 PhD, 15 advisee and 10 placement students.
fn year(y: i32, archived: bool) -> Vec<Student> {
    let (s1, s2) = (d(y, 9, 21), d(y + 1, 1, 26));
    let held = |current, past| if archived { past } else { current };
    let mut out = Vec::new();
    for i in 0..40 {
        out.push(Student {
            name: format!("Student {y}-{i}"),
            b_number: format!("B00{y}{i:03}"),
            email: format!("s{i}@example.ac.uk"),
            course: "BEng Computer Engineering".into(),
            archived,
            notes: notes(3),
            fyp: Some(FypRecord {
                programme: if i % 3 == 0 { "MSc".into() } else { "BEng".into() },
                initial_contact: true,
                meetings: vec![repeating(s1, 1, None, held(5, 12)), repeating(s2, 1, None, held(0, 12))],
                ..Default::default()
            }),
            ..Default::default()
        });
    }
    for i in 0..6 {
        let mut phd = PhdRecord::default();
        phd.meetings = vec![repeating(s1, 2, Some(d(y + 1, 8, 31)), held(3, 24))];
        out.push(Student { name: format!("PhD {y}-{i}"), archived, notes: notes(5), phd: Some(phd), ..Default::default() });
    }
    for i in 0..15 {
        out.push(Student { name: format!("Advisee {y}-{i}"), archived, advisee: Some(AdviseeRecord::default()), ..Default::default() });
    }
    for i in 0..10 {
        out.push(Student { name: format!("Placement {y}-{i}"), archived, placement: Some(PlacementRecord::default()), ..Default::default() });
    }
    out
}

fn dataset(archived_years: i32) -> Database {
    let mut db = Database::default();
    db.settings.sem1_start = Some(d(2026, 9, 21));
    db.settings.sem2_start = Some(d(2027, 1, 26));
    for y in (2026 - archived_years)..2026 {
        db.students.extend(year(y, true));
    }
    db.students.extend(year(2026, false));
    db
}

fn time<T>(label: &str, runs: u32, mut f: impl FnMut() -> T) {
    for _ in 0..3 {
        std::hint::black_box(f());
    }
    let start = Instant::now();
    for _ in 0..runs {
        std::hint::black_box(f());
    }
    let ms = start.elapsed().as_secs_f64() * 1000.0 / f64::from(runs);
    println!("  {label:<40} {ms:8.3} ms");
}

#[test]
#[ignore = "timing, not a check; run on demand"]
fn perf_timings() {
    let today = d(2026, 10, 26);
    for archived_years in [0, 5] {
        let db = dataset(archived_years);
        println!("{} students, {} archived years:", db.students.len(), archived_years);
        time("change check (fingerprint)", 100, || storage::fingerprint(&db));
        time("dashboard data", 100, || (db.events(), db.upcoming(today, 14), db.attention(today)));
        time("events()", 100, || db.events());
        let dir = tempfile::tempdir().unwrap();
        let mut vault = Vault::create(&dir.path().join("data.svt"), "correct horse battery staple", &db).unwrap();
        time("autosave (Vault::save)", 20, || vault.save(&db).unwrap());
    }
}
