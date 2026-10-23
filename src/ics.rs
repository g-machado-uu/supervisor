//! Minimal iCalendar (RFC 5545) files: the calendar export and invite files.
//! Repeating meetings become one event with a weekly rule, cancelled weeks
//! and rescheduled weeks.

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, Timelike, Utc, Weekday};
use jiff::tz::TimeZone;

use crate::model::{CalItem, Event, EventKind, Settings};

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

/// A parameter value (e.g. `CN`), quoted when it holds `,` `;` or `:`.
fn param(text: &str) -> String {
    let clean: String = text
        .chars()
        .filter(|c| *c != '"' && !c.is_control())
        .collect();
    if clean.contains([',', ';', ':']) {
        format!("\"{clean}\"")
    } else {
        clean
    }
}

/// Loose check that keeps anything odd out of a `mailto:` value.
pub fn valid_email(email: &str) -> bool {
    let email = email.trim();
    match email.split_once('@') {
        Some((user, domain)) => {
            !user.is_empty()
                && domain.contains('.')
                && !email
                    .chars()
                    .any(|c| c.is_whitespace() || c.is_control() || "\"<>;:,()[]\\".contains(c))
        }
        None => false,
    }
}

fn to_jiff(dt: NaiveDateTime) -> Option<jiff::civil::DateTime> {
    jiff::civil::DateTime::new(
        i16::try_from(dt.year()).ok()?,
        dt.month() as i8,
        dt.day() as i8,
        dt.hour() as i8,
        dt.minute() as i8,
        dt.second() as i8,
        0,
    )
    .ok()
}

fn from_jiff(dt: jiff::civil::DateTime) -> Option<NaiveDateTime> {
    NaiveDate::from_ymd_opt(dt.year().into(), dt.month() as u32, dt.day() as u32)?.and_hms_opt(
        dt.hour() as u32,
        dt.minute() as u32,
        dt.second() as u32,
    )
}

fn offset_text(seconds: i32) -> String {
    let sign = if seconds < 0 { '-' } else { '+' };
    let m = seconds.unsigned_abs() / 60;
    format!("{sign}{:02}{:02}", m / 60, m % 60)
}

/// One STANDARD or DAYLIGHT part of a VTIMEZONE.
struct Observance {
    daylight: bool,
    name: String,
    from: i32,
    to: i32,
    /// Local wall time of the change, and its yearly rule.
    start: NaiveDateTime,
    rule: Option<String>,
}

/// The computer's time zone, written as a VTIMEZONE so that weekly meetings
/// keep their wall-clock time across the clock changes. Zones whose changes
/// can't be described by a "n-th weekday of the month" rule fall back to UTC.
struct Zone {
    tz: TimeZone,
    id: String,
    observances: Option<Vec<Observance>>,
}

impl Zone {
    fn new(tz: TimeZone, year: i32) -> Self {
        let id = tz.iana_name().unwrap_or("Local").to_string();
        let observances = Self::observances(&tz, year);
        Self {
            tz,
            id,
            observances,
        }
    }

    fn observances(tz: &TimeZone, year: i32) -> Option<Vec<Observance>> {
        let start = jiff::civil::date(i16::try_from(year).ok()?, 1, 1)
            .at(0, 0, 0, 0)
            .to_zoned(TimeZone::UTC)
            .ok()?
            .timestamp();
        let year_end = start + jiff::SignedDuration::from_hours(24 * 366);
        let changes: Vec<_> = tz
            .following(start)
            .take_while(|t| t.timestamp() < year_end)
            .take(3)
            .collect();
        if changes.is_empty() {
            let offset = tz.to_offset(start).seconds();
            return Some(vec![Observance {
                daylight: false,
                name: tz.to_offset_info(start).abbreviation().to_string(),
                from: offset,
                to: offset,
                start: NaiveDate::from_ymd_opt(1970, 1, 1)?.and_hms_opt(0, 0, 0)?,
                rule: None,
            }]);
        }
        if changes.len() != 2 || changes[0].dst().is_dst() == changes[1].dst().is_dst() {
            return None;
        }
        changes
            .iter()
            .map(|t| {
                let before = tz.to_offset(t.timestamp() - jiff::SignedDuration::from_secs(1));
                let local = from_jiff(before.to_datetime(t.timestamp()))?;
                let date = local.date();
                let nth = if (date + Duration::days(7)).month() != date.month() {
                    -1
                } else {
                    (date.day() as i32 - 1) / 7 + 1
                };
                let day = match date.weekday() {
                    Weekday::Mon => "MO",
                    Weekday::Tue => "TU",
                    Weekday::Wed => "WE",
                    Weekday::Thu => "TH",
                    Weekday::Fri => "FR",
                    Weekday::Sat => "SA",
                    Weekday::Sun => "SU",
                };
                Some(Observance {
                    daylight: t.dst().is_dst(),
                    name: t.abbreviation().to_string(),
                    from: before.seconds(),
                    to: t.offset().seconds(),
                    start: local,
                    rule: Some(format!(
                        "FREQ=YEARLY;BYMONTH={};BYDAY={nth}{day}",
                        date.month()
                    )),
                })
            })
            .collect()
    }

    fn write(&self, out: &mut String) {
        let Some(observances) = &self.observances else {
            return;
        };
        push_line(out, "BEGIN:VTIMEZONE");
        push_line(out, &format!("TZID:{}", self.id));
        for o in observances {
            let kind = if o.daylight { "DAYLIGHT" } else { "STANDARD" };
            push_line(out, &format!("BEGIN:{kind}"));
            push_line(out, &format!("DTSTART:{}", o.start.format("%Y%m%dT%H%M%S")));
            if let Some(rule) = &o.rule {
                push_line(out, &format!("RRULE:{rule}"));
            }
            push_line(out, &format!("TZOFFSETFROM:{}", offset_text(o.from)));
            push_line(out, &format!("TZOFFSETTO:{}", offset_text(o.to)));
            push_line(out, &format!("TZNAME:{}", escape(&o.name)));
            push_line(out, &format!("END:{kind}"));
        }
        push_line(out, "END:VTIMEZONE");
    }

    fn utc(&self, local: NaiveDateTime) -> String {
        to_jiff(local)
            .and_then(|dt| dt.to_zoned(self.tz.clone()).ok())
            .map_or_else(
                || local.format("%Y%m%dT%H%M%S").to_string(),
                |z| z.timestamp().strftime("%Y%m%dT%H%M%SZ").to_string(),
            )
    }

    /// `NAME;TZID=…:local` or, without a usable zone, `NAME:utcZ`.
    fn property(&self, name: &str, local: NaiveDateTime) -> String {
        match self.observances {
            Some(_) => format!(
                "{name};TZID={}:{}",
                param(&self.id),
                local.format("%Y%m%dT%H%M%S")
            ),
            None => format!("{name}:{}", self.utc(local)),
        }
    }
}

/// Start/end (one hour, or all day without a time) of one event.
fn push_times(out: &mut String, zone: &Zone, date: NaiveDate, time: Option<NaiveTime>) {
    match time {
        Some(time) => {
            let start = NaiveDateTime::new(date, time);
            push_line(out, &zone.property("DTSTART", start));
            push_line(out, &zone.property("DTEND", start + Duration::hours(1)));
        }
        None => {
            let end = date + Duration::days(1);
            push_line(
                out,
                &format!("DTSTART;VALUE=DATE:{}", date.format("%Y%m%d")),
            );
            push_line(out, &format!("DTEND;VALUE=DATE:{}", end.format("%Y%m%d")));
        }
    }
}

/// Identifies one week of a series (`RECURRENCE-ID`, `EXDATE`) by its planned start.
fn week_property(zone: &Zone, name: &str, planned: NaiveDate, time: Option<NaiveTime>) -> String {
    match time {
        Some(time) => zone.property(name, NaiveDateTime::new(planned, time)),
        None => format!("{name};VALUE=DATE:{}", planned.format("%Y%m%d")),
    }
}

/// Everything after the times that a series and its moved weeks share.
fn push_details(out: &mut String, item: &CalItem, settings: &Settings, invite: bool) {
    let e = &item.event;
    push_line(out, &format!("SUMMARY:{}", escape(&e.title)));
    if let Some(location) = location(e) {
        push_line(out, &format!("LOCATION:{}", escape(location)));
    }
    let link = settings.teams_link.trim();
    if e.online && !link.is_empty() {
        push_line(
            out,
            &format!(
                "DESCRIPTION:{}",
                escape(&format!("Join on Microsoft Teams:\n{link}"))
            ),
        );
        push_line(out, &format!("URL:{}", escape(link)));
    }
    let category = match e.kind {
        EventKind::FypMeeting => "Project",
        EventKind::Visit => "Placement",
        EventKind::AdviseeMeeting => "Advisee",
        EventKind::Deadline => "Deadline",
    };
    push_line(out, &format!("CATEGORIES:{category}"));
    if e.kind == EventKind::Deadline {
        push_line(out, "TRANSP:TRANSPARENT");
    }
    if invite {
        let me = settings.my_email.trim();
        if valid_email(me) {
            let name = settings.my_name.trim();
            let cn = if name.is_empty() {
                String::new()
            } else {
                format!(";CN={}", param(name))
            };
            push_line(out, &format!("ORGANIZER{cn}:mailto:{me}"));
        }
        for a in item.attendees.iter().filter(|a| valid_email(&a.email)) {
            let cn = if a.name.is_empty() {
                String::new()
            } else {
                format!(";CN={}", param(&a.name))
            };
            push_line(
                out,
                &format!(
                    "ATTENDEE{cn};ROLE=REQ-PARTICIPANT;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:{}",
                    a.email.trim()
                ),
            );
        }
    }
    push_line(out, "STATUS:CONFIRMED");
}

fn location(e: &Event) -> Option<&str> {
    match (e.online, e.location.is_empty()) {
        (true, _) => Some("Microsoft Teams Meeting"),
        (false, false) => Some(&e.location),
        (false, true) => None,
    }
}

/// An .ics file for Outlook. With `invite`, each event names you as the
/// organiser and the students as attendees, so Outlook can send it on.
/// Times are local to `tz` (the computer's zone in the app).
pub fn calendar(items: &[CalItem], settings: &Settings, invite: bool, tz: &TimeZone) -> String {
    let now = Utc::now();
    let stamp = now.format("%Y%m%dT%H%M%SZ").to_string();
    let first_year = items
        .iter()
        .map(|i| i.event.date.year())
        .min()
        .unwrap_or(now.year());
    let zone = Zone::new(tz.clone(), first_year - 1);

    let mut out = String::new();
    push_line(&mut out, "BEGIN:VCALENDAR");
    push_line(&mut out, "VERSION:2.0");
    push_line(&mut out, "PRODID:-//Supervisor//Student Tracker//EN");
    push_line(&mut out, "CALSCALE:GREGORIAN");
    push_line(&mut out, "METHOD:PUBLISH");
    if items.iter().any(|i| i.event.time.is_some()) {
        zone.write(&mut out);
    }
    for item in items {
        let e = &item.event;
        push_line(&mut out, "BEGIN:VEVENT");
        push_line(&mut out, &format!("UID:{}@supervisor.local", e.uid));
        push_line(&mut out, &format!("DTSTAMP:{stamp}"));
        push_line(&mut out, "SEQUENCE:0");
        push_times(&mut out, &zone, e.date, e.time);
        if let Some(rule) = &item.series {
            // UNTIL is inclusive, and in UTC when the start has a time zone.
            let until = match e.time {
                Some(time) => format!("UNTIL={}", zone.utc(NaiveDateTime::new(rule.until, time))),
                None => format!("UNTIL={}", rule.until.format("%Y%m%d")),
            };
            push_line(
                &mut out,
                &format!(
                    "RRULE:FREQ=WEEKLY;INTERVAL={};{until}",
                    rule.every_weeks.max(1)
                ),
            );
            for o in rule.exceptions.iter().filter(|o| o.cancelled) {
                push_line(&mut out, &week_property(&zone, "EXDATE", o.planned, e.time));
            }
        }
        push_details(&mut out, item, settings, invite);
        push_line(&mut out, "END:VEVENT");

        // Rescheduled weeks replace their planned week in the series.
        let moved = item
            .series
            .iter()
            .flat_map(|r| &r.exceptions)
            .filter(|o| o.rescheduled());
        for o in moved {
            push_line(&mut out, "BEGIN:VEVENT");
            push_line(&mut out, &format!("UID:{}@supervisor.local", e.uid));
            push_line(
                &mut out,
                &week_property(&zone, "RECURRENCE-ID", o.planned, e.time),
            );
            push_line(&mut out, &format!("DTSTAMP:{stamp}"));
            push_line(&mut out, "SEQUENCE:0");
            push_times(&mut out, &zone, o.date(), o.time.or(e.time));
            push_details(&mut out, item, settings, invite);
            push_line(&mut out, "END:VEVENT");
        }
    }
    push_line(&mut out, "END:VCALENDAR");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Attendee, Occurrence, SeriesRule};
    use uuid::Uuid;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn uk() -> TimeZone {
        TimeZone::posix("GMT0BST,M3.5.0/1,M10.5.0").unwrap()
    }

    fn event(date: NaiveDate, time: Option<u32>, kind: EventKind, online: bool) -> Event {
        Event {
            uid: "meeting-x".into(),
            student_id: Uuid::nil(),
            date,
            time: time.and_then(|h| NaiveTime::from_hms_opt(h, 0, 0)),
            kind,
            title: "BEng project meeting – Smith, J".into(),
            done: false,
            online,
            location: "MG.201".into(),
            series: None,
            cancelled: false,
        }
    }

    fn item(event: Event) -> CalItem {
        CalItem {
            event,
            series: None,
            attendees: vec![
                Attendee {
                    name: "Smith, J".into(),
                    email: "smith-j@ulster.ac.uk".into(),
                },
                Attendee {
                    name: "Bad".into(),
                    email: "x@y.com\r\nATTENDEE:mailto:evil@z.com".into(),
                },
            ],
        }
    }

    fn settings(link: &str) -> Settings {
        Settings {
            my_name: "Gabriel Machado".into(),
            my_email: "me@ulster.ac.uk".into(),
            teams_link: link.into(),
            ..Default::default()
        }
    }

    fn unfold(ics: &str) -> String {
        ics.replace("\r\n ", "")
    }

    #[test]
    fn calendar_without_invite_details() {
        let items = [
            item(event(
                d(2026, 11, 9),
                Some(14),
                EventKind::FypMeeting,
                false,
            )),
            item(event(d(2026, 11, 9), None, EventKind::Deadline, false)),
        ];
        let ics = calendar(&items, &settings(""), false, &uk());
        assert_eq!(ics.matches("BEGIN:VEVENT").count(), 2);
        assert!(ics.contains("DTSTART;TZID=Local:20261109T140000"));
        assert!(ics.contains("DTSTART;VALUE=DATE:20261109"));
        assert!(ics.contains("SUMMARY:BEng project meeting – Smith\\, J"));
        assert!(ics.contains("TRANSP:TRANSPARENT"));
        assert!(!ics.contains("ORGANIZER") && !ics.contains("ATTENDEE"));
        assert!(ics.lines().all(|l| l.len() <= 76));
    }

    #[test]
    fn time_zone_matches_uk_rules() {
        let ics = calendar(
            &[item(event(
                d(2026, 10, 30),
                Some(14),
                EventKind::FypMeeting,
                false,
            ))],
            &settings(""),
            false,
            &uk(),
        );
        // The same shape as the file that imported correctly into Outlook.
        let vtz = &ics[ics.find("BEGIN:VTIMEZONE").unwrap()..ics.find("END:VTIMEZONE").unwrap()];
        assert!(vtz.contains(
            "BEGIN:DAYLIGHT\r\nDTSTART:20250330T010000\r\nRRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=-1SU\r\nTZOFFSETFROM:+0000\r\nTZOFFSETTO:+0100\r\nTZNAME:BST"
        ));
        assert!(vtz.contains(
            "BEGIN:STANDARD\r\nDTSTART:20251026T020000\r\nRRULE:FREQ=YEARLY;BYMONTH=10;BYDAY=-1SU\r\nTZOFFSETFROM:+0100\r\nTZOFFSETTO:+0000\r\nTZNAME:GMT"
        ));
    }

    #[test]
    fn zone_without_clock_changes() {
        let fixed = TimeZone::fixed(jiff::tz::offset(8));
        let ics = calendar(
            &[item(event(
                d(2026, 10, 30),
                Some(9),
                EventKind::Visit,
                false,
            ))],
            &settings(""),
            false,
            &fixed,
        );
        assert!(ics.contains(
            "BEGIN:STANDARD\r\nDTSTART:19700101T000000\r\nTZOFFSETFROM:+0800\r\nTZOFFSETTO:+0800"
        ));
        assert!(!ics.contains("DAYLIGHT"));
    }

    #[test]
    fn invite_has_organizer_and_attendees() {
        let ics = calendar(
            &[item(event(
                d(2026, 10, 23),
                Some(14),
                EventKind::FypMeeting,
                false,
            ))],
            &settings(""),
            true,
            &uk(),
        );
        let unfolded = unfold(&ics);
        assert!(ics.contains("METHOD:PUBLISH"));
        assert!(ics.contains("ORGANIZER;CN=Gabriel Machado:mailto:me@ulster.ac.uk"));
        assert!(unfolded.contains("ATTENDEE;CN=\"Smith, J\";ROLE=REQ-PARTICIPANT"));
        assert!(unfolded.contains(":mailto:smith-j@ulster.ac.uk\r\n"));
        assert!(!ics.contains("evil"), "malformed addresses are dropped");
        assert!(ics.contains("LOCATION:MG.201"));
    }

    #[test]
    fn teams_location_and_link() {
        let teams = item(event(
            d(2026, 10, 30),
            Some(14),
            EventKind::FypMeeting,
            true,
        ));
        let without = calendar(std::slice::from_ref(&teams), &settings(""), true, &uk());
        assert!(without.contains("LOCATION:Microsoft Teams Meeting"));
        assert!(!without.contains("DESCRIPTION"));

        let link = "https://teams.microsoft.com/l/meetup-join/abc";
        let with = unfold(&calendar(&[teams], &settings(link), true, &uk()));
        assert!(with.contains("DESCRIPTION:Join on Microsoft Teams:\\n"));
        assert!(with.contains(&format!("URL:{link}")));
    }

    #[test]
    fn weekly_series_with_cancelled_and_moved_weeks() {
        let mut series = item(event(
            d(2026, 10, 19),
            Some(14),
            EventKind::FypMeeting,
            false,
        ));
        series.series = Some(SeriesRule {
            every_weeks: 1,
            until: d(2026, 12, 7),
            exceptions: vec![
                Occurrence {
                    planned: d(2026, 11, 9),
                    cancelled: true,
                    ..Default::default()
                },
                Occurrence {
                    planned: d(2026, 11, 16),
                    moved_to: Some(d(2026, 11, 17)),
                    time: NaiveTime::from_hms_opt(10, 0, 0),
                    ..Default::default()
                },
            ],
        });
        let ics = calendar(&[series], &settings(""), true, &uk());
        // Starts in BST, ends in GMT: the local time stays 14:00 thanks to the
        // zone; UNTIL is the last week's start in UTC.
        assert!(ics.contains("DTSTART;TZID=Local:20261019T140000"));
        assert!(ics.contains("RRULE:FREQ=WEEKLY;INTERVAL=1;UNTIL=20261207T140000Z"));
        assert!(ics.contains("EXDATE;TZID=Local:20261109T140000"));
        assert_eq!(ics.matches("BEGIN:VEVENT").count(), 2);
        assert_eq!(ics.matches("UID:meeting-x@supervisor.local").count(), 2);
        assert!(ics.contains("RECURRENCE-ID;TZID=Local:20261116T140000"));
        assert!(ics.contains("DTSTART;TZID=Local:20261117T100000"));
        assert!(ics.lines().all(|l| l.len() <= 76));
    }

    #[test]
    fn email_check() {
        assert!(valid_email(" g.machado@ulster.ac.uk "));
        assert!(!valid_email("no-at-sign"));
        assert!(!valid_email("a@b"));
        assert!(!valid_email("a@b.com;x"));
    }
}
