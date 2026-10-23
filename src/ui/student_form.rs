//! Detail panel: common details plus a collapsible section per role.

use egui::{Align, Layout, RichText, Ui};

use crate::model::{AdviseeRecord, FypRecord, PlacementRecord, Settings, Student};
use crate::ui::{advisee, fyp, placement, theme, widgets};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Fyp,
    Placement,
    Advisee,
}

impl Role {
    pub fn label(self) -> &'static str {
        match self {
            Role::Fyp => "Project supervision",
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
        ui.heading(if s.name.is_empty() { "New student" } else { &s.name });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button("×").on_hover_text("Close").clicked() {
                action = FormAction::Close;
            }
        });
    });
    if s.archived {
        theme::chip(ui, "archived", theme::GREY);
    }
    ui.add_space(4.0);

    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        theme::card(ui, "Details", |ui| {
            widgets::form_grid(ui, "details-grid", |ui| {
                ui.label("B Number");
                ui.vertical(|ui| {
                    widgets::text_field(ui, &mut s.b_number, "B00123456");
                    if duplicate_b {
                        ui.colored_label(theme::AMBER, "⚠ Another student has this B number");
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

        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.label("Add role:");
            if s.fyp.is_none() && ui.button("➕ Project supervision").clicked() {
                s.fyp = Some(FypRecord::default());
            }
            if s.placement.is_none() && ui.button("➕ Placement").clicked() {
                s.placement = Some(PlacementRecord::default());
            }
            if s.advisee.is_none() && ui.button("➕ Studies Advisee").clicked() {
                s.advisee = Some(AdviseeRecord::default());
            }
        });

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
        ui.separator();
        if ui
            .button(RichText::new("🗑 Delete student…").color(theme::RED))
            .clicked()
        {
            action = FormAction::Delete;
        }
    });
    action
}

fn role_section(ui: &mut Ui, role: Role, action: &mut FormAction, add: impl FnOnce(&mut Ui)) {
    ui.add_space(6.0);
    theme::card(ui, "", |ui| {
        egui::CollapsingHeader::new(RichText::new(role.label()).strong().size(16.0))
            .id_salt(role.label())
            .default_open(true)
            .show(ui, |ui| {
                add(ui);
                ui.add_space(4.0);
                if ui
                    .small_button(RichText::new(format!("Remove {} record…", role.label())).color(theme::GREY))
                    .clicked()
                {
                    *action = FormAction::RemoveRole(role);
                }
            });
    });
}
