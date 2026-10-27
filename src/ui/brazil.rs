//! Brazilian programmes, for external research students: the level, the
//! grant (bolsa) from a federal agency or state foundation, the extension of
//! the deadlines, and the standard stages with their suggested dates.

use egui::{RichText, Ui};

use crate::model::{
    AGENCIES, AgencyKind, BrLevel, BrRule, ExtensionReason, Grant, PhdRecord, agency_by_id,
};
use crate::ui::phd::combo;
use crate::ui::{icons, theme, widgets};

/// The level, with its Portuguese name on hover. Changing it also moves the
/// student to the matching MSc or PhD list.
pub fn level_combo(ui: &mut Ui, p: &mut PhdRecord) {
    let mut level = p.brazil.level;
    egui::ComboBox::from_id_salt("br-level")
        .selected_text(level.label())
        .width(200.0)
        .show_ui(ui, |ui| {
            for option in BrLevel::ALL {
                ui.selectable_value(&mut level, option, option.label())
                    .on_hover_text(option.portuguese());
            }
        })
        .response
        .on_hover_text(level.portuguese());
    if level != p.brazil.level {
        p.set_br_level(level);
    }
}

fn small_wrapped(ui: &mut Ui, text: &str) {
    ui.add(
        egui::Label::new(RichText::new(text).small().color(theme::muted(ui)))
            .wrap_mode(egui::TextWrapMode::Wrap),
    );
}

/// "Deadline extension" row of the project dates grid.
pub fn extension_row(ui: &mut Ui, p: &mut PhdRecord) {
    ui.label("Deadline extension");
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.add(
                egui::DragValue::new(&mut p.brazil.extension_months)
                    .range(0..=24)
                    .suffix(" months"),
            )
            .on_hover_text("Months added to the course deadlines (prorrogação)");
            combo(ui, "br-ext-reason", &mut p.brazil.extension_reason, &ExtensionReason::ALL, |r| r.label());
        });
        let hint = p.brazil.extension_reason.hint();
        if !hint.is_empty() {
            small_wrapped(ui, hint);
        }
    });
    ui.end_row();
}

/// The "standard stages" box, suggested deadlines and the defence deadline
/// an extension leads to.
pub fn stages_header(ui: &mut Ui, p: &mut PhdRecord, rule: &BrRule) {
    let mut standard = !p.skip_standard;
    ui.horizontal_wrapped(|ui| {
        if ui
            .checkbox(&mut standard, "Standard stages of the Brazilian programme")
            .on_hover_text(
                "Course credits, language proficiency, qualifying exam, annual reports, \
                 sending the work to the committee and the defence",
            )
            .changed()
        {
            p.skip_standard = !standard;
        }
        if !p.skip_standard
            && ui
                .add_enabled(
                    p.start.is_some(),
                    egui::Button::new(format!("{} Fill in suggested deadlines", icons::CALENDAR_CHECK)).small(),
                )
                .on_hover_text(
                    "Works out the empty deadlines from the start date, with the rules of the level in \
                     Settings › Brazilian programmes. Dates already set are kept.",
                )
                .on_disabled_hover_text("Set the project start (enrolment) first")
                .clicked()
        {
            p.suggest_br(rule);
        }
    });
    small_wrapped(
        ui,
        if p.skip_standard {
            "Add the deadlines from the student's programme and choose when to be reminded."
        } else {
            "Suggested deadlines follow the rules of the level; check the programme's regulations \
             (regimento) and adjust as needed."
        },
    );

    // The deadline with the extension, when it differs from the one set.
    if let Some(with_ext) = p.br_rule_deadline(rule).filter(|_| p.brazil.extension_months > 0)
        && p.brazil.defence_deadline.deadline != Some(with_ext)
        && !p.skip_standard
    {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(
                theme::fg(ui, theme::AMBER),
                format!(
                    "{} With the extension, the defence deadline becomes {}",
                    icons::HOURGLASS,
                    widgets::fmt_date(Some(with_ext))
                ),
            );
            if ui.small_button("Use this date").clicked() {
                p.brazil.defence_deadline.deadline = Some(with_ext);
            }
        });
    }
}

fn agency_text(id: &str) -> String {
    match agency_by_id(id) {
        None if id.is_empty() => "No grant".into(),
        None => id.into(),
        Some(a) if a.kind == AgencyKind::State => format!("{} ({})", a.id, a.uf),
        Some(a) => a.id.into(),
    }
}

/// The grant: agency (federal, the 27 state foundations or other),
/// modality, process number, start, length and any extension.
pub fn grant_editor(ui: &mut Ui, g: &mut Grant, level: BrLevel) {
    let typical = level.typical_grant_months();
    widgets::form_grid(ui, "br-grant", |ui| {
        ui.label("Funder");
        ui.vertical(|ui| {
            let before = g.agency.clone();
            egui::ComboBox::from_id_salt("br-agency")
                .selected_text(agency_text(&g.agency))
                .width(200.0)
                .height(360.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut g.agency, String::new(), "No grant");
                    for (kind, title) in [
                        (AgencyKind::Federal, "FEDERAL"),
                        (AgencyKind::State, "STATE FOUNDATIONS (FAPs)"),
                        (AgencyKind::Other, "OTHER"),
                    ] {
                        ui.add_space(4.0);
                        ui.label(theme::semibold(title).size(10.5).color(theme::muted(ui)));
                        for a in AGENCIES.iter().filter(|a| a.kind == kind) {
                            ui.selectable_value(&mut g.agency, a.id.to_string(), agency_text(a.id))
                                .on_hover_text(a.name);
                        }
                    }
                });
            if g.agency != before && g.active() && g.months == 0 {
                g.months = typical;
            }
            if let Some(a) = agency_by_id(&g.agency) {
                small_wrapped(ui, a.name);
            }
        });
        ui.end_row();
        if !g.active() {
            return;
        }
        if g.agency == "Other" {
            ui.label("Source");
            widgets::text_field(ui, &mut g.agency_other, "Name of the funder");
            ui.end_row();
        }
        ui.label("Modality");
        widgets::text_field(ui, &mut g.modality, "e.g. Demanda Social, PROEX, GM, GD");
        ui.end_row();
        ui.label("Process number");
        widgets::text_field(ui, &mut g.process, "Optional");
        ui.end_row();
        ui.label("Grant start");
        widgets::opt_date(ui, "br-grant-start", &mut g.start);
        ui.end_row();
        ui.label("Length");
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut g.months).range(0..=72).suffix(" months"));
            if g.months != typical
                && ui
                    .small_button(format!("Usual: {typical} months"))
                    .on_hover_text(
                        "Master's: up to 24 months; doctorate: up to 48 \
                         (CAPES Portaria 76/2010, CNPq and most FAPs).",
                    )
                    .clicked()
            {
                g.months = typical;
            }
        });
        ui.end_row();
        ui.label("Extension");
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut g.extension_months).range(0..=24).suffix(" months"));
                combo(ui, "br-grant-reason", &mut g.extension_reason, &ExtensionReason::ALL, |r| r.label());
            });
            let hint = g.extension_reason.hint();
            if !hint.is_empty() {
                small_wrapped(ui, hint);
            }
        });
        ui.end_row();
        ui.label("Grant end");
        match g.end() {
            Some(end) => ui.label(theme::semibold(widgets::fmt_date(Some(end)))),
            None => ui.label(RichText::new("set the start and length").color(theme::muted(ui))),
        };
        ui.end_row();
    });
}
