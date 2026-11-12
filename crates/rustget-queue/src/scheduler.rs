use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PowerAction {
    None,
    Shutdown,
    Sleep,
    Hibernate,
    Exit,
}

impl PowerAction {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Shutdown => "shutdown",
            Self::Sleep => "sleep",
            Self::Hibernate => "hibernate",
            Self::Exit => "exit",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Self {
        match value {
            "shutdown" => Self::Shutdown,
            "sleep" => Self::Sleep,
            "hibernate" => Self::Hibernate,
            "exit" => Self::Exit,
            _ => Self::None,
        }
    }
}

/// A weekly time window. Minutes are local wall-clock minutes after midnight.
/// `stop_min < start_min` denotes an overnight window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schedule {
    pub id: Uuid,
    pub name: String,
    pub enabled: bool,
    pub days: u8,
    pub start_min: Option<u16>,
    pub stop_min: Option<u16>,
    pub action: PowerAction,
}

impl Schedule {
    #[must_use]
    pub fn active_at(&self, day: u8, minute: u16) -> bool {
        if !self.enabled {
            return false;
        }
        let selected_today = self.days & (1 << day) != 0;
        let previous_day = (day + 6) % 7;
        let selected_yesterday = self.days & (1 << previous_day) != 0;
        match (self.start_min, self.stop_min) {
            (Some(start), Some(stop)) if start <= stop => {
                selected_today && minute >= start && minute < stop
            }
            (Some(start), Some(stop)) => {
                (selected_today && minute >= start) || (selected_yesterday && minute < stop)
            }
            (Some(start), None) => selected_today && minute >= start,
            (None, Some(stop)) => selected_today && minute < stop,
            (None, None) => false,
        }
    }

    /// Parse `HH:MM` into minutes after midnight.
    pub fn parse_time(value: &str) -> Option<u16> {
        let (hours, minutes) = value.split_once(':')?;
        let hours: u16 = hours.parse().ok()?;
        let minutes: u16 = minutes.parse().ok()?;
        if hours < 24 && minutes < 60 {
            Some(hours * 60 + minutes)
        } else {
            None
        }
    }

    #[must_use]
    pub fn format_time(minutes: u16) -> String {
        format!("{:02}:{:02}", minutes / 60, minutes % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schedule(start: u16, stop: u16) -> Schedule {
        Schedule {
            id: Uuid::new_v4(),
            name: "test".into(),
            enabled: true,
            days: 0b111_1111,
            start_min: Some(start),
            stop_min: Some(stop),
            action: PowerAction::None,
        }
    }

    #[test]
    fn daytime_window() {
        let item = schedule(9 * 60, 17 * 60);
        assert!(!item.active_at(0, 8 * 60));
        assert!(item.active_at(0, 9 * 60));
        assert!(item.active_at(0, 16 * 60 + 59));
        assert!(!item.active_at(0, 17 * 60));
    }

    #[test]
    fn overnight_window_uses_previous_day_selection() {
        let mut item = schedule(23 * 60, 6 * 60);
        item.days = 1; // Monday only
        assert!(item.active_at(0, 23 * 60 + 30)); // Monday 23:30
        assert!(item.active_at(1, 5 * 60)); // Tuesday 05:00
        assert!(!item.active_at(2, 5 * 60)); // Wednesday 05:00
    }

    #[test]
    fn time_parsing() {
        assert_eq!(Schedule::parse_time("09:30"), Some(570));
        assert_eq!(Schedule::parse_time("24:00"), None);
        assert_eq!(Schedule::format_time(570), "09:30");
    }
}
