//! Minimal iCalendar (RFC 5545) export of meetings, visits and deadlines.

use chrono::{Duration, NaiveDateTime, Utc};

use crate::model::{Event, EventKind};

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            _ => out.push(c),
        }
    }
    out
}

/// Fold lines longer than 75 octets, as the spec requires.
fn push_line(out: &mut String, line: &str) {
    let mut len = 0;
    for c in line.chars() {
        if len + c.len_utf8() > 75 {
            out.push_str("\r\n ");
            len = 1;
        }
        out.push(c);
        len += c.len_utf8();
    }
    out.push_str("\r\n");
}

pub fn to_ics(events: &[Event]) -> String {
    let stamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let mut out = String::new();
    push_line(&mut out, "BEGIN:VCALENDAR");
    push_line(&mut out, "VERSION:2.0");
    push_line(&mut out, "PRODID:-//Supervisor//Student Tracker//EN");
    push_line(&mut out, "CALSCALE:GREGORIAN");
    for e in events {
        push_line(&mut out, "BEGIN:VEVENT");
        push_line(&mut out, &format!("UID:{}@supervisor.local", e.uid));
        push_line(&mut out, &format!("DTSTAMP:{stamp}"));
        match e.time {
            // Floating local time; one hour by default.
            Some(time) => {
                let start = NaiveDateTime::new(e.date, time);
                let end = start + Duration::hours(1);
                push_line(&mut out, &format!("DTSTART:{}", start.format("%Y%m%dT%H%M%S")));
                push_line(&mut out, &format!("DTEND:{}", end.format("%Y%m%dT%H%M%S")));
            }
            None => {
                let end = e.date + Duration::days(1);
                push_line(&mut out, &format!("DTSTART;VALUE=DATE:{}", e.date.format("%Y%m%d")));
                push_line(&mut out, &format!("DTEND;VALUE=DATE:{}", end.format("%Y%m%d")));
            }
        }
        push_line(&mut out, &format!("SUMMARY:{}", escape(&e.title)));
        let category = match e.kind {
            EventKind::FypMeeting => "FYP",
            EventKind::Visit => "Placement",
            EventKind::AdviseeMeeting => "Advisee",
            EventKind::Deadline => "Deadline",
        };
        push_line(&mut out, &format!("CATEGORIES:{category}"));
        if e.kind == EventKind::Deadline {
            push_line(&mut out, "TRANSP:TRANSPARENT");
        }
        push_line(&mut out, "END:VEVENT");
    }
    push_line(&mut out, "END:VCALENDAR");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, NaiveTime};
    use uuid::Uuid;

    #[test]
    fn contains_events() {
        let date = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap();
        let events = vec![
            Event {
                uid: "a".into(),
                student_id: Uuid::nil(),
                date,
                time: Some(NaiveTime::from_hms_opt(14, 30, 0).unwrap()),
                kind: EventKind::FypMeeting,
                title: "FYP meeting – Smith, J".into(),
                done: false,
            },
            Event {
                uid: "b".into(),
                student_id: Uuid::nil(),
                date,
                time: None,
                kind: EventKind::Deadline,
                title: "MEC526 Coursework 1 due".into(),
                done: false,
            },
        ];
        let ics = to_ics(&events);
        assert_eq!(ics.matches("BEGIN:VEVENT").count(), 2);
        assert!(ics.contains("DTSTART:20261109T143000"));
        assert!(ics.contains("DTSTART;VALUE=DATE:20261109"));
        assert!(ics.contains("SUMMARY:FYP meeting – Smith\\, J"));
        assert!(ics.lines().all(|l| l.len() <= 76));
    }
}
