use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_UTC_OFFSET_HOURS: i8 = 8;
static UTC_OFFSET_HOURS: OnceLock<Mutex<i8>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeParts {
    pub year: u16,
    pub month: u16,
    pub day: u16,
    pub hour: u16,
    pub minute: u16,
    pub second: u16,
}

pub fn configure_utc_offset_hours(offset: i8) {
    let mut state = utc_offset_state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *state = offset;
}

pub fn current_time_parts() -> TimeParts {
    let unix_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i128;
    let offset_seconds = i128::from(current_utc_offset_hours()) * 3_600;
    time_parts_from_unix_seconds(unix_seconds + offset_seconds)
}

pub fn current_timestamp_hms() -> String {
    let parts = current_time_parts();
    format!("{:02}:{:02}:{:02}", parts.hour, parts.minute, parts.second)
}

fn current_utc_offset_hours() -> i8 {
    *utc_offset_state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn utc_offset_state() -> &'static Mutex<i8> {
    UTC_OFFSET_HOURS.get_or_init(|| Mutex::new(DEFAULT_UTC_OFFSET_HOURS))
}

fn time_parts_from_unix_seconds(total_seconds: i128) -> TimeParts {
    let days = total_seconds.div_euclid(86_400);
    let seconds_of_day = total_seconds.rem_euclid(86_400);

    let (year, month, day) = civil_from_days(days);
    let hour = (seconds_of_day / 3_600) as u16;
    let minute = ((seconds_of_day % 3_600) / 60) as u16;
    let second = (seconds_of_day % 60) as u16;

    TimeParts {
        year: year as u16,
        month,
        day,
        hour,
        minute,
        second,
    }
}

fn civil_from_days(days: i128) -> (i32, u16, u16) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if month <= 2 { 1 } else { 0 };

    (year as i32, month as u16, day as u16)
}

#[cfg(test)]
mod tests {
    use super::time_parts_from_unix_seconds;

    #[test]
    fn converts_epoch_with_positive_offset() {
        let parts = time_parts_from_unix_seconds(8 * 3_600);

        assert_eq!(parts.year, 1970);
        assert_eq!(parts.month, 1);
        assert_eq!(parts.day, 1);
        assert_eq!(parts.hour, 8);
        assert_eq!(parts.minute, 0);
        assert_eq!(parts.second, 0);
    }

    #[test]
    fn converts_epoch_with_negative_offset_across_day() {
        let parts = time_parts_from_unix_seconds(-3_600);

        assert_eq!(parts.year, 1969);
        assert_eq!(parts.month, 12);
        assert_eq!(parts.day, 31);
        assert_eq!(parts.hour, 23);
        assert_eq!(parts.minute, 0);
        assert_eq!(parts.second, 0);
    }
}
