//! Detail panel: common details plus a collapsible section per role.

use egui::{Align, Layout, RichText, Ui};

use crate::model::{AdviseeRecord, FypRecord, PhdRecord, PlacementRecord, Settings, Student, is_cpf, is_ni_number, masked};
use crate::ui::widgets::Share;
use crate::ui::{advisee, fyp, icons, phd, placement, theme, widgets};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Fyp,
    Phd,
    Placement,
    Advisee,
}

impl Role {
    /// Icon and colour of its chip and section.
    fn style(self) -> (&'static str, egui::Color32) {
        match self {
            Role::Fyp => (icons::BOOK_OPEN_TEXT, theme::ACCENT),
            Role::Phd => (icons::GRADUATION_CAP, theme::PLUM),
            Role::Placement => (icons::BUILDINGS, theme::AMBER),
            Role::Advisee => (icons::CHAT_CIRCLE_TEXT, theme::GREEN),
        }
    }

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
    /// Send the meeting or visit: an invite file or Google Calendar.
    Share(Share),
    /// Show the hidden registration number of this student.
    Reveal,
    /// Hide it again.
    Hide,
}

/// What of a student's private details is on screen; the eye buttons
/// switch `details` and `marks`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shown {
    /// Name, number and email; off in privacy mode, or once hidden.
    pub details: bool,
    /// Coursework marks.
    pub marks: bool,
    /// The registration number of an external student, hidden by Settings › Privacy.
    pub number: bool,
}

/// Dots for a hidden value, a dash for an empty one.
pub fn hidden_value(ui: &mut Ui, text: &str) {
    if text.trim().is_empty() {
        ui.label(RichText::new("–").color(theme::muted(ui)));
    } else {
        ui.label(RichText::new("•".repeat(8)).monospace().color(theme::muted(ui)));
    }
}

pub fn show(ui: &mut Ui, s: &mut Student, settings: &Settings, duplicate_b: bool, shown: &mut Shown) -> FormAction {
    let mut action = FormAction::None;
    let private = !shown.details;
    let hidden = private || (s.number_hidden(settings) && !shown.number);
    ui.horizontal(|ui| {
        theme::avatar(ui, if private { "" } else { &s.name }, 52.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            let name = if private {
                "•".repeat(10)
            } else if s.name.is_empty() {
                "New student".into()
            } else {
                s.name.clone()
            };
            ui.label(theme::serif(name, 24.0));
            let mut sub = vec![];
            let number = if hidden { masked(&s.b_number) } else { s.b_number.clone() };
            if !number.is_empty() {
                sub.push(number.as_str());
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
    // Clicking a role's chip opens its section and scrolls to it.
    let mut jump = None;
    ui.horizontal_wrapped(|ui| {
        ui.add_space(60.0);
        let mut role_chip = |ui: &mut Ui, role: Role, text: String| {
            let (icon, color) = role.style();
            if theme::chip_button(ui, format!("{icon} {text}"), color)
                .on_hover_text(format!("Go to {}", role.label()))
                .clicked()
            {
                jump = Some(role);
            }
        };
        if let Some(f) = &s.fyp {
            role_chip(ui, Role::Fyp, settings.programme_short(&f.programme));
        }
        if let Some(p) = &s.phd {
            let ext = match (p.external, p.brazilian_programme()) {
                (true, true) => " External · Brazil",
                (true, false) => " External",
                (false, _) => "",
            };
            role_chip(ui, Role::Phd, format!("{}{ext}", p.degree_label()));
        }
        if s.placement.is_some() {
            role_chip(ui, Role::Placement, "Placement".into());
        }
        if s.advisee.is_some() {
            role_chip(ui, Role::Advisee, "Advisee".into());
        }
        if s.archived {
            theme::chip(ui, format!("{} archived", icons::ARCHIVE), theme::GREY);
        }
    });
    ui.add_space(10.0);

    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        theme::card(ui, "", |ui| {
            let eye = |ui: &mut Ui| {
                if theme::eye_button(ui, shown.details, "the student's details") {
                    shown.details = !shown.details;
                }
            };
            theme::section_with(ui, "Details", eye, |ui| {
                widgets::form_grid(ui, "details-grid", |ui| {
                    let external = s.external();
                    ui.label(crate::model::id_label(external));
                    ui.vertical(|ui| {
                        let (hint, number) = if external {
                            ("Registration number at their university", "registration number")
                        } else {
                            ("B00123456", "B number")
                        };
                        if private {
                            hidden_value(ui, &s.b_number);
                        } else if hidden {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(masked(&s.b_number)).monospace());
                                let hover = if settings.reveal_needs_password {
                                    "Show the registration number (asks for your password)"
                                } else {
                                    "Show the registration number"
                                };
                                if ui.button(format!("{} Reveal", icons::EYE)).on_hover_text(hover).clicked() {
                                    action = FormAction::Reveal;
                                }
                            });
                        } else if s.number_hidden(settings) {
                            ui.horizontal(|ui| {
                                let width = (ui.available_width() - 90.0).max(120.0);
                                ui.add(egui::TextEdit::singleline(&mut s.b_number).hint_text(hint).desired_width(width));
                                if ui.button(format!("{} Hide", icons::EYE_SLASH)).clicked() {
                                    action = FormAction::Hide;
                                }
                            });
                        } else {
                            widgets::text_field(ui, &mut s.b_number, hint);
                        }
                        let id = if is_cpf(&s.b_number) {
                            Some("This is a CPF")
                        } else if is_ni_number(&s.b_number) {
                            Some("This looks like a National Insurance number")
                        } else {
                            None
                        };
                        if let Some(id) = id.filter(|_| external && !private) {
                            ui.label(
                                RichText::new(format!("{} {id} – avoid sharing it", icons::KEY))
                                    .small()
                                    .color(theme::muted(ui)),
                            );
                        }
                        if duplicate_b {
                            ui.colored_label(theme::fg(ui, theme::AMBER), format!("{} Another student has this {number}", icons::WARNING));
                        }
                    });
                    ui.end_row();
                    ui.label("Name");
                    if private {
                        hidden_value(ui, &s.name);
                    } else {
                        widgets::text_field(ui, &mut s.name, "Full name");
                    }
                    ui.end_row();
                    ui.label("Email");
                    if private {
                        hidden_value(ui, &s.email);
                    } else {
                        widgets::text_field(ui, &mut s.email, "University email");
                    }
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
            role_section(ui, Role::Fyp, jump, &mut action, |ui| {
                if let Some(share) = fyp::show(ui, f, settings, shown) {
                    invite = Some(share);
                }
            });
        }
        if let Some(p) = &mut s.phd {
            role_section(ui, Role::Phd, jump, &mut action, |ui| {
                if let Some(share) = phd::show(ui, p, settings, id) {
                    invite = Some(share);
                }
            });
        }
        if let Some(p) = &mut s.placement {
            role_section(ui, Role::Placement, jump, &mut action, |ui| {
                if let Some(share) = placement::show(ui, p, id) {
                    invite = Some(share);
                }
            });
        }
        if let Some(a) = &mut s.advisee {
            role_section(ui, Role::Advisee, jump, &mut action, |ui| {
                if let Some(share) = advisee::show(ui, a, settings) {
                    invite = Some(share);
                }
            });
        }
        if let Some(share) = invite {
            action = FormAction::Share(share);
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

/// `jump`: the role whose chip was clicked, to open and scroll to.
fn role_section(ui: &mut Ui, role: Role, jump: Option<Role>, action: &mut FormAction, add: impl FnOnce(&mut Ui)) {
    let (icon, color) = role.style();
    ui.add_space(10.0);
    let card = theme::card_frame(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let title = theme::semibold(format!("{icon}  {}", role.label()))
            .size(15.0)
            .color(theme::fg(ui, color));
        egui::CollapsingHeader::new(title)
            .id_salt(role.label())
            .default_open(true)
            .open((jump == Some(role)).then_some(true))
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
    if jump == Some(role) {
        ui.scroll_to_rect(card.response.rect, Some(Align::TOP));
    }
}
