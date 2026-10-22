//! Home screen: counts, upcoming items and things needing attention.

use egui::{RichText, Ui};
use uuid::Uuid;

use crate::model::{Database, EventKind, Severity};
use crate::ui::student_list::Category;
use crate::ui::{theme, widgets};

pub enum DashAction {
    None,
    Open(Uuid),
    Goto(Category),
    ExportIcs,
}

pub fn show(ui: &mut Ui, db: &Database) -> DashAction {
    let mut action = DashAction::None;
    let today = widgets::today();

    ui.horizontal(|ui| {
        ui.heading("Dashboard");
        ui.label(
            RichText::new(format!("{} · {}", today.format("%A %d %B %Y"), db.settings.academic_year))
                .color(theme::GREY),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .button("📅 Export calendar (.ics)…")
                .on_hover_text("Meetings, visits and deadlines. Note: the exported file is not encrypted.")
                .clicked()
            {
                action = DashAction::ExportIcs;
            }
        });
    });
    ui.add_space(8.0);

    // Summary tiles.
    ui.horizontal_wrapped(|ui| {
        for cat in [
            Category::FypBeng,
            Category::FypMeng,
            Category::FypSust,
            Category::Placement,
            Category::Advisee,
        ] {
            let resp = egui::Frame::group(ui.style())
                .fill(ui.visuals().window_fill)
                .corner_radius(egui::CornerRadius::same(10))
                .inner_margin(egui::Margin::symmetric(16, 10))
                .show(ui, |ui| {
                    ui.set_min_width(150.0);
                    ui.label(RichText::new(cat.count(db).to_string()).size(28.0).strong().color(theme::ACCENT));
                    ui.label(RichText::new(cat.title().replace("Final Year Project", "FYP")).color(theme::GREY));
                })
                .response
                .interact(egui::Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if resp.clicked() {
                action = DashAction::Goto(cat);
            }
        }
    });
    ui.add_space(10.0);

    let days = db.settings.upcoming_days;
    let upcoming = db.upcoming(today, days);
    let attention = db.attention(today);

    ui.columns(2, |cols| {
        egui::ScrollArea::vertical().id_salt("upcoming").show(&mut cols[0], |ui| {
            theme::card(ui, &format!("Upcoming – next {days} days"), |ui| {
                if upcoming.is_empty() {
                    ui.label(RichText::new("Nothing scheduled.").color(theme::GREY));
                }
                for e in &upcoming {
                    let (icon, color) = match e.kind {
                        EventKind::FypMeeting => ("👥", theme::ACCENT),
                        EventKind::Visit => ("🏢", theme::GREEN),
                        EventKind::AdviseeMeeting => ("💬", theme::AMBER),
                        EventKind::Deadline => ("📌", theme::RED),
                    };
                    ui.horizontal_wrapped(|ui| {
                        let when = match e.time {
                            Some(t) => format!("{} {}", e.date.format("%a %d %b"), t.format("%H:%M")),
                            None => e.date.format("%a %d %b").to_string(),
                        };
                        ui.label(RichText::new(when).monospace().color(color));
                        if ui
                            .add(egui::Label::new(format!("{icon} {}", e.title)).sense(egui::Sense::click()))
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            action = DashAction::Open(e.student_id);
                        }
                    });
                }
            });
        });
        egui::ScrollArea::vertical().id_salt("attention").show(&mut cols[1], |ui| {
            theme::card(ui, "Needs attention", |ui| {
                if attention.is_empty() {
                    ui.label(RichText::new("All clear ✔").color(theme::GREEN));
                }
                for a in &attention {
                    let color = match a.severity {
                        Severity::Overdue => theme::RED,
                        Severity::Pending => theme::AMBER,
                    };
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("•").size(20.0).color(color));
                        if ui
                            .add(egui::Label::new(&a.message).sense(egui::Sense::click()))
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            action = DashAction::Open(a.student_id);
                        }
                    });
                }
            });
        });
    });
    action
}
