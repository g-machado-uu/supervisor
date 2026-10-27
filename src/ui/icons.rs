//! Phosphor line icons (regular weight), from the bundled `Phosphor.ttf`.
//!
//! The font sits right after the text font in every family, so these can be
//! mixed into any label, e.g. `format!("{} Add student", icons::PLUS)`.

pub const ARCHIVE: &str = "\u{E00C}";
pub const ARROW_RIGHT: &str = "\u{E06C}";
pub const BELL: &str = "\u{E0CE}";
pub const BOOK_OPEN_TEXT: &str = "\u{E8F2}";
pub const BUILDINGS: &str = "\u{E102}";
pub const CALENDAR_BLANK: &str = "\u{E10A}";
pub const CALENDAR_CHECK: &str = "\u{E712}";
pub const CALENDAR_DOTS: &str = "\u{E7B4}";
pub const CARET_LEFT: &str = "\u{E138}";
pub const CARET_RIGHT: &str = "\u{E13A}";
pub const CHAT_CIRCLE_TEXT: &str = "\u{E16E}";
pub const CHECK: &str = "\u{E182}";
pub const CHECK_CIRCLE: &str = "\u{E184}";
pub const CLOCK: &str = "\u{E19A}";
pub const DESKTOP: &str = "\u{E560}";
pub const FLOPPY_DISK: &str = "\u{E248}";
pub const EXPORT: &str = "\u{EAF0}";
pub const EYE: &str = "\u{E220}";
pub const EYE_SLASH: &str = "\u{E224}";
pub const GEAR_SIX: &str = "\u{E272}";
pub const GRADUATION_CAP: &str = "\u{E62C}";
pub const HOURGLASS: &str = "\u{E2B2}";
pub const HOUSE: &str = "\u{E2C2}";
pub const KEY: &str = "\u{E2D6}";
pub const LINK: &str = "\u{E2E2}";
pub const LIST_BULLETS: &str = "\u{E2F2}";
pub const LOCK_SIMPLE: &str = "\u{E308}";
pub const MAGNIFYING_GLASS: &str = "\u{E30C}";
pub const MAP_PIN: &str = "\u{E316}";
pub const MOON: &str = "\u{E330}";
pub const PAPER_PLANE_TILT: &str = "\u{E398}";
pub const PLUS: &str = "\u{E3D4}";
pub const PUSH_PIN: &str = "\u{E3E2}";
pub const REPEAT: &str = "\u{E3F6}";
pub const STUDENT: &str = "\u{E73E}";
pub const SUN: &str = "\u{E472}";
pub const TRASH: &str = "\u{E4A6}";
pub const USERS: &str = "\u{E4D6}";
pub const USERS_THREE: &str = "\u{E68E}";
pub const WARNING: &str = "\u{E4E0}";
pub const WARNING_CIRCLE: &str = "\u{E4E2}";
pub const X: &str = "\u{E4F6}";

#[cfg(test)]
pub const ALL: &[&str] = &[
    ARCHIVE, ARROW_RIGHT, BELL, BOOK_OPEN_TEXT, BUILDINGS, CALENDAR_BLANK, CALENDAR_CHECK,
    CALENDAR_DOTS, CARET_LEFT, CARET_RIGHT, CHAT_CIRCLE_TEXT, CHECK, CHECK_CIRCLE, CLOCK,
    DESKTOP, EXPORT, EYE, EYE_SLASH, FLOPPY_DISK, GEAR_SIX, GRADUATION_CAP, HOURGLASS, HOUSE, KEY, LINK, LIST_BULLETS,
    LOCK_SIMPLE, MAGNIFYING_GLASS, MAP_PIN, MOON, PAPER_PLANE_TILT, PLUS, PUSH_PIN, REPEAT,
    STUDENT, SUN, TRASH, USERS, USERS_THREE, WARNING, WARNING_CIRCLE, X,
];
