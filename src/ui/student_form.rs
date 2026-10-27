//! Detail panel: common details plus a collapsible section per role.

use egui::{Align, Layout, RichText, Ui};

use crate::model::{AdviseeRecord, FypRecord, PhdRecord, PlacementRecord, Settings, Student};
use crate::ui::{advisee, fyp, icons, phd, placement, theme, widgets};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Fyp,
    Phd,
    Placement,
    Advisee,
}

impl Role {
    pub fn label(self) -> &'static str {
        match self {
            Role::Fyp => "Project supervision",
            Role::Phd => "Research supervision",
            Role::Placement => "Placement",
            Role::Advisee => "Studies Advisee",
        }
    }
}

pub enum FormAction {
    None,
    Close,
    Delete,
    RemoveRole(Role),
    /// Save an invite file for the meeting or visit with this UID.
    Invite(String),
}

pub fn show(ui: &mut Ui, s: &mut Student, settings: &Settings, duplicate_b: bool) -> FormAction {
    let mut action = FormAction::None;
    ui.horizontal(|ui| {
        theme::avatar(ui, &s.name, 52.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(theme::serif(if s.name.is_empty() { "New student" } else { &s.name }, 24.0));
            let mut sub = vec![];
            if !s.b_number.is_empty() {
                sub.push(s.b_number.as_str());
            }
            if !s.course.is_empty() {
                sub.push(s.course.as_str());
            }
            if !sub.is_empty() {
                ui.label(RichText::new(sub.join(" · ")).color(theme::muted(ui)));
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            if ui
                .add(egui::Button::new(RichText::new(icons::X).size(15.0)).frame(false))
                .on_hover_text("Close")
                .clicked()
            {
                action = FormAction::Close;
            }
        });
    });
    ui.horizontal_wrapped(|ui| {
        ui.add_space(60.0);
        if let Some(f) = &s.fyp {
            theme::chip(ui, format!("{} {}", icons::BOOK_OPEN_TEXT, settings.programme_short(&f.programme)), theme::ACCENT);
        }
        if let Some(p) = &s.phd {
            let ext = match (p.external, p.brazilian_programme()) {
                (true, true) => " External · Brazil",
                (true, false) => " External",
                (false, _) => "",
            };
            theme::chip(ui, format!("{} {}{ext}", icons::GRADUATION_CAP, p.degree_label()), theme::PLUM);
        }
        if s.placement.is_some() {
            theme::chip(ui, format!("{} Placement", icons::BUILDINGS), theme::AMBER);
        }
        if s.advisee.is_some() {
            theme::chip(ui, format!("{} Advisee", icons::CHAT_CIRCLE_TEXT), theme::GREEN);
        }
        if s.archived {
            theme::chip(ui, format!("{} archived", icons::ARCHIVE), theme::GREY);
        }
    });
    ui.add_space(10.0);

    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        theme::card(ui, "Details", |ui| {
            widgets::form_grid(ui, "details-grid", |ui| {
                let external = s.external();
                ui.label(crate::model::id_label(external));
                ui.vertical(|ui| {
                    let (hint, number) = if external {
                        ("Registration number at their university", "registration number")
                    } else {
                        ("B00123456", "B number")
                    };
                    widgets::text_field(ui, &mut s.b_number, hint);
                    if duplicate_b {
                        ui.colored_label(theme::fg(ui, theme::AMBER), format!("{} Another student has this {number}", icons::WARNING));
                    }
                });
                ui.end_row();
                ui.label("Name");
                widgets::text_field(ui, &mut s.name, "Full name");
                ui.end_row();
                ui.label("Email");
                widgets::text_field(ui, &mut s.email, "University email");
                ui.end_row();
                ui.label("Course");
                widgets::text_field(ui, &mut s.course, "e.g. BEng Mechanical Engineering");
                ui.end_row();
                ui.label("Archived");
                ui.checkbox(&mut s.archived, "Hide from lists and dashboard");
                ui.end_row();
            });
            widgets::notes(ui, &mut s.notes, "General notes…");
        });

        if s.fyp.is_none() || s.phd.is_none() || s.placement.is_none() || s.advisee.is_none() {
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Add role").color(theme::muted(ui)));
                if s.fyp.is_none() && ui.button(format!("{} Project supervision", icons::PLUS)).clicked() {
                    s.fyp = Some(FypRecord::default());
                }
                if s.phd.is_none() && ui.button(format!("{} Research (PhD / MSc)", icons::PLUS)).clicked() {
                    s.phd = Some(PhdRecord::default());
                }
                if s.placement.is_none() && ui.button(format!("{} Placement", icons::PLUS)).clicked() {
                    s.placement = Some(PlacementRecord::default());
                }
                if s.advisee.is_none() && ui.button(format!("{} Studies Advisee", icons::PLUS)).clicked() {
                    s.advisee = Some(AdviseeRecord::default());
                }
            });
        }

        // At most one invite button is clicked per frame.
        let mut invite = None;
        let id = s.id;
        if let Some(f) = &mut s.fyp {
            role_section(ui, Role::Fyp, &mut action, |ui| {
                if let Some(uid) = fyp::show(ui, f, settings) {
                    invite = Some(uid);
                }
            });
        }
        if let Some(p) = &mut s.phd {
            role_section(ui, Role::Phd, &mut action, |ui| {
                if let Some(uid) = phd::show(ui, p, settings, id) {
                    invite = Some(uid);
                }
            });
        }
        if let Some(p) = &mut s.placement {
            role_section(ui, Role::Placement, &mut action, |ui| {
                if let Some(uid) = placement::show(ui, p, id) {
                    invite = Some(uid);
                }
            });
        }
        if let Some(a) = &mut s.advisee {
            role_section(ui, Role::Advisee, &mut action, |ui| {
                if let Some(uid) = advisee::show(ui, a, settings) {
                    invite = Some(uid);
                }
            });
        }
        if let Some(uid) = invite {
            action = FormAction::Invite(uid);
        }

        ui.add_space(16.0);
        if ui
            .button(RichText::new(format!("{} Delete student…", icons::TRASH)).color(theme::fg(ui, theme::RED)))
            .clicked()
        {
            action = FormAction::Delete;
        }
        ui.add_space(8.0);
    });
    action
}

fn role_section(ui: &mut Ui, role: Role, action: &mut FormAction, add: impl FnOnce(&mut Ui)) {
    let (icon, color) = match role {
        Role::Fyp => (icons::BOOK_OPEN_TEXT, theme::ACCENT),
        Role::Phd => (icons::GRADUATION_CAP, theme::PLUM),
        Role::Placement => (icons::BUILDINGS, theme::AMBER),
        Role::Advisee => (icons::CHAT_CIRCLE_TEXT, theme::GREEN),
    };
    ui.add_space(10.0);
    theme::card(ui, "", |ui| {
        let title = theme::semibold(format!("{icon}  {}", role.label()))
            .size(15.0)
            .color(theme::fg(ui, color));
        egui::CollapsingHeader::new(title)
            .id_salt(role.label())
            .default_open(true)
            .show(ui, |ui| {
                add(ui);
                ui.add_space(4.0);
                if ui
                    .small_button(RichText::new(format!("Remove {} record…", role.label())).color(theme::muted(ui)))
                    .clicked()
                {
                    *action = FormAction::RemoveRole(role);
                }
            });
    });
}
