//! A student's timetable: a week grid to click or drag over the hours they
//! are busy. Just the times, no module names.

use chrono::{NaiveDate, Timelike};
use egui::{Align2, CornerRadius, FontId, Rect, RichText, Role, Sense, Stroke, StrokeKind, Ui, WidgetInfo, pos2, vec2};
use uuid::Uuid;

use crate::model::{BusySlot, Clash, Semester, Settings, Student, TIMETABLE_DAYS, TIMETABLE_HOURS, Timetable};
use crate::ui::{icons, theme, widgets};

/// The dialog, while open. What is ticked goes straight into the student.
pub struct TimetableDialog {
    pub student: Uuid,
    semester: Semester,
    painting: Option<Painting>,
}

/// A press on the grid, and the drag that may follow it.
#[derive(Clone, Copy)]
struct Painting {
    /// Marking hours busy or free: the opposite of the hour it began on.
    busy: bool,
    last: BusySlot,
}

impl TimetableDialog {
    /// Opens on the semester being taught or, once semester 1's teaching is
    /// over, semester 2, which is the one to plan by then.
    pub fn new(student: Uuid, settings: &Settings, today: NaiveDate) -> Self {
        let first_over = settings.teaching_end(Semester::S1).is_some_and(|end| today > end);
        let semester = if first_over || settings.semester_of(today) == Semester::S2 {
            Semester::S2
        } else {
            Semester::S1
        };
        Self {
            student,
            semester,
            painting: None,
        }
    }
}

/// Draws the dialog. Returns false once it has been closed.
pub fn show(ui: &mut Ui, dlg: &mut TimetableDialog, s: &mut Student, settings: &Settings, name: &str) -> bool {
    let screen = ui.ctx().content_rect();
    let mut done = false;
    let resp = egui::Modal::new(egui::Id::unique("student-timetable")).frame(theme::modal_frame(ui)).show(ui.ctx(), |ui| {
        ui.set_width((screen.width() - 120.0).clamp(560.0, 860.0));
        ui.horizontal(|ui| {
            theme::icon_badge(ui, icons::CALENDAR_DOTS, theme::ACCENT, 40.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(theme::serif("Timetable", 22.0));
                ui.label(RichText::new(name).color(theme::muted(ui)));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut semester = dlg.semester;
                // Laid out from the right, so semester 1 comes last to read first.
                theme::segmented(ui, &mut semester, &[(Semester::S2, "Semester 2"), (Semester::S1, "Semester 1")]);
                dlg.semester = semester;
            });
        });
        ui.add_space(8.0);
        ui.label(
            RichText::new("Click the hours the student is busy, or drag over several. Choosing a meeting time then warns of a clash.")
                .color(theme::muted(ui)),
        );
        ui.add_space(8.0);
        let row_height = ((screen.height() - 340.0) / TIMETABLE_HOURS.count() as f32).clamp(24.0, 40.0);
        grid(ui, dlg, &mut s.timetable, row_height);
        ui.add_space(8.0);
        notes(ui, dlg.semester, s, settings);
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            let slots = s.timetable.slots(dlg.semester).len();
            let clicked = ui.add_enabled(slots > 0, egui::Button::new(format!("{} Clear", icons::TRASH))).clicked();
            let ask = |semester: &Semester| {
                Some(widgets::Ask {
                    title: "Clear the timetable?".into(),
                    body: format!(
                        "The {} the student is busy in {} are cleared. The other semester is left as it is.",
                        widgets::plural(slots, "hour"),
                        semester.label()
                    ),
                    button: "Clear".into(),
                })
            };
            if let Some(semester) = widgets::confirmed(ui, egui::Id::unique(("clear-timetable", s.id)), clicked.then_some(dlg.semester), ask) {
                s.timetable.slots_mut(semester).clear();
            }
            let (other, label) = match dlg.semester {
                Semester::S2 => (Semester::S1, "Copy Semester 1"),
                _ => (Semester::S2, "Copy Semester 2"),
            };
            if slots == 0 && !s.timetable.slots(other).is_empty() && ui.button(label).clicked() {
                *s.timetable.slots_mut(dlg.semester) = s.timetable.slots(other).to_vec();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(theme::primary_button("Done")).clicked() {
                    done = true;
                }
            });
        });
    });
    !done && !resp.should_close()
}

/// The hours on a straight line between two, so a quick drag leaves no gaps.
fn line(from: BusySlot, to: BusySlot) -> impl Iterator<Item = BusySlot> {
    let (day, hour) = (i32::from(from.day), i32::from(from.hour));
    let (days, hours) = (i32::from(to.day) - day, i32::from(to.hour) - hour);
    let steps = days.abs().max(hours.abs()).max(1);
    (0..=steps).map(move |i| BusySlot {
        day: (day + days * i / steps) as u8,
        hour: (hour + hours * i / steps) as u8,
    })
}

/// The hours of the week, one row per hour. The cells touch, so a drag never
/// falls between two; each is drawn a little inside its rectangle.
fn grid(ui: &mut Ui, dlg: &mut TimetableDialog, timetable: &mut Timetable, row_height: f32) {
    let (first_hour, rows) = (*TIMETABLE_HOURS.start(), TIMETABLE_HOURS.count());
    let (heading, times) = (24.0, 56.0);
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, heading + row_height * rows as f32), Sense::hover());
    let column = (width - times) / TIMETABLE_DAYS.len() as f32;
    let top = rect.min + vec2(times, heading);
    let cell = |slot: BusySlot| {
        let min = top + vec2(column * f32::from(slot.day), row_height * f32::from(slot.hour - first_hour));
        Rect::from_min_size(min, vec2(column, row_height))
    };
    let mut slots = Vec::new();
    for day in 0..TIMETABLE_DAYS.len() as u8 {
        slots.extend(TIMETABLE_HOURS.map(|hour| BusySlot { day, hour }));
    }

    // Input first, so what is drawn is up to date.
    let id = ui.scope_id().with(("timetable-cells", dlg.semester.label()));
    let mut down = false;
    let mut clicked = None;
    let mut hovered = None;
    for &slot in &slots {
        let resp = ui.interact(cell(slot), id.with((slot.day, slot.hour)), Sense::click_and_drag());
        let busy = timetable.is_busy(dlg.semester, slot);
        let label = format!("{} {}", TIMETABLE_DAYS[usize::from(slot.day)], slot.label());
        resp.widget_info(|| WidgetInfo::selected(Role::CheckBox, true, busy, &label));
        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
        down |= resp.is_pointer_button_down_on();
        if resp.clicked() {
            clicked = Some(slot);
        }
        if resp.hovered() || resp.has_focus() {
            hovered = Some(slot);
        }
    }
    let under_pointer = ui.ctx().pointer_interact_pos().and_then(|pos| slots.iter().copied().find(|s| cell(*s).contains(pos)));
    if down {
        if let Some(to) = under_pointer {
            let painting = dlg.painting.unwrap_or(Painting {
                busy: !timetable.is_busy(dlg.semester, to),
                last: to,
            });
            for slot in line(painting.last, to) {
                timetable.set_busy(dlg.semester, slot, painting.busy);
            }
            dlg.painting = Some(Painting { last: to, ..painting });
        }
    } else if dlg.painting.take().is_none()
        && let Some(slot) = clicked
    {
        // A press and release too quick to see, or Space on the keyboard.
        let busy = timetable.is_busy(dlg.semester, slot);
        timetable.set_busy(dlg.semester, slot, !busy);
    }

    let painter = ui.painter();
    let muted = theme::muted(ui);
    let accent = theme::fg(ui, theme::ACCENT);
    for (day, name) in TIMETABLE_DAYS.iter().enumerate() {
        let x = top.x + column * (day as f32 + 0.5);
        painter.text(pos2(x, rect.top() + heading / 2.0), Align2::CENTER_CENTER, name.to_uppercase(), theme::semibold_font(11.0), muted);
    }
    for hour in TIMETABLE_HOURS {
        let y = cell(BusySlot { day: 0, hour }).center().y;
        let start = format!("{hour:02}:15");
        painter.text(pos2(top.x - 10.0, y), Align2::RIGHT_CENTER, start, FontId::proportional(12.0), muted);
    }
    let radius = CornerRadius::same(6);
    for &slot in &slots {
        if timetable.is_busy(dlg.semester, slot) {
            continue;
        }
        let inner = cell(slot).shrink(1.5);
        painter.rect_filled(inner, radius, ui.visuals().faint_bg_color);
        painter.rect_stroke(inner, radius, ui.visuals().widgets.noninteractive.bg_stroke, StrokeKind::Inside);
        if hovered == Some(slot) {
            painter.text(inner.center(), Align2::CENTER_CENTER, slot.label(), FontId::proportional(12.0), muted);
        }
    }
    // Busy hours run together, as they do on a printed timetable.
    for day in 0..TIMETABLE_DAYS.len() as u8 {
        for block in timetable.blocks(dlg.semester, day) {
            let first = BusySlot { day, hour: block.start.hour() as u8 };
            let last = BusySlot { day, hour: block.end.hour() as u8 - 1 };
            let inner = cell(first).union(cell(last)).shrink(1.5);
            painter.rect_filled(inner, radius, theme::soft(ui, theme::ACCENT));
            painter.rect_stroke(inner, radius, Stroke::new(1.0, accent), StrokeKind::Inside);
            painter.text(inner.center(), Align2::CENTER_CENTER, block.label(), theme::semibold_font(12.0), accent);
        }
    }
    if let Some(slot) = hovered {
        painter.rect_stroke(cell(slot).shrink(1.5), radius, Stroke::new(1.5, accent), StrokeKind::Inside);
    }
}

/// What the student's meetings make of this semester's timetable, and whether
/// it can be checked at all.
fn notes(ui: &mut Ui, semester: Semester, s: &Student, settings: &Settings) {
    let warn = theme::fg(ui, theme::AMBER);
    if settings.semester_start(semester).is_none() {
        ui.colored_label(
            warn,
            format!("{} Set this semester's dates in Settings: clashes are only checked in its teaching weeks.", icons::WARNING),
        );
        return;
    }
    let today = widgets::today();
    let mut clashes: Vec<Clash> = s
        .meetings()
        .flat_map(|m| s.timetable.meeting_clashes(m, settings, today))
        .filter(|c| settings.teaching_week(c.date).is_some_and(|(sem, _)| sem == semester))
        .collect();
    clashes.sort_by_key(|c| (c.date, c.time));
    let Some(first) = clashes.first() else {
        ui.label(RichText::new("Clashes are checked in this semester's teaching weeks.").small().color(theme::muted(ui)));
        return;
    };
    let when = format!("{} at {} (busy {})", first.date.format("%a %d %b"), first.time.format("%H:%M"), first.block.label());
    ui.colored_label(
        warn,
        match clashes.len() {
            1 => format!("{} A meeting still to come clashes with this: {when}", icons::WARNING),
            n => format!("{} {n} meetings still to come clash with this, the first: {when}", icons::WARNING),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quick_drag_leaves_no_gaps() {
        let slot = |day, hour| BusySlot { day, hour };
        let hours: Vec<_> = line(slot(1, 9), slot(1, 12)).collect();
        assert_eq!(hours, [slot(1, 9), slot(1, 10), slot(1, 11), slot(1, 12)]);
        // Upwards and to the side too.
        let hours: Vec<_> = line(slot(3, 12), slot(1, 10)).collect();
        assert_eq!(hours, [slot(3, 12), slot(2, 11), slot(1, 10)]);
        assert_eq!(line(slot(2, 9), slot(2, 9)).last(), Some(slot(2, 9)));
    }

    #[test]
    fn opens_on_the_semester_to_plan() {
        let d = |y, m, day| NaiveDate::from_ymd_opt(y, m, day).unwrap();
        let settings = Settings {
            sem1_start: Some(d(2026, 9, 21)),
            sem2_start: Some(d(2027, 1, 25)),
            ..Default::default()
        };
        let opens_on = |today| TimetableDialog::new(Uuid::nil(), &settings, today).semester;
        assert_eq!(opens_on(d(2026, 10, 1)), Semester::S1);
        assert_eq!(opens_on(d(2026, 12, 20)), Semester::S2, "teaching of semester 1 is over");
        assert_eq!(opens_on(d(2027, 2, 1)), Semester::S2);
        let unset = Settings::default();
        assert_eq!(TimetableDialog::new(Uuid::nil(), &unset, d(2027, 2, 1)).semester, Semester::S1);
    }
}
