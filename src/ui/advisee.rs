//! Studies advisee section: contact / reply / meetings for each semester.

use egui::{RichText, Ui};

use crate::model::{AdviseeRecord, ReplyStatus, SemesterContact};
use crate::ui::widgets;

pub fn show(ui: &mut Ui, a: &mut AdviseeRecord) {
    semester(ui, "Semester 1", &mut a.s1);
    ui.add_space(6.0);
    semester(ui, "Semester 2", &mut a.s2);
}

fn semester(ui: &mut Ui, title: &str, c: &mut SemesterContact) {
    ui.push_id(title, |ui| {
        egui::Frame::group(ui.style())
            .corner_radius(egui::CornerRadius::same(8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(title).strong());
                widgets::form_grid(ui, "sem-grid", |ui| {
                    ui.label("I contacted them");
                    widgets::opt_date(ui, "contacted", &mut c.contacted_on);
                    ui.end_row();

                    ui.label("Their reply");
                    widgets::reply_status(ui, &mut c.reply);
                    ui.end_row();

                    if c.reply == ReplyStatus::Replied {
                        ui.label("Replied on");
                        widgets::opt_date(ui, "replied", &mut c.replied_on);
                        ui.end_row();
                    }
                });
                widgets::meetings_editor(ui, "meetings", &mut c.meetings, Some("Semester meeting"));
            });
    });
}
