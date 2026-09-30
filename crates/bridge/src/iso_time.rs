//! ISO 8601 times in UTC, as JavaScript writes them: `2026-09-25T06:46:21.432Z`.
//! The calendar math is by Howard Hinnant: March starts the year, so February is last.

/// Seconds since 1970 of an ISO 8601 time in UTC, such as `2026-09-25T06:46:21.432Z`.
pub fn unix_time(iso: &str) -> Option<u32> {
    let number = |range: std::ops::Range<usize>| iso.get(range)?.parse::<i64>().ok();
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    // Days from civil.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let year_of_era = y - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    u32::try_from(days * 86_400 + hour * 3600 + minute * 60 + second).ok()
}

/// `2026-09-25T06:46:21.432Z` for milliseconds since 1970, as JavaScript writes it.
pub fn iso_time(millis: u128) -> String {
    let seconds = i64::try_from(millis / 1000).unwrap_or(i64::MAX);
    let (days, second_of_day) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    // Civil from days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        second_of_day / 3600,
        second_of_day % 3600 / 60,
        second_of_day % 60,
        millis % 1000
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_prints_as_javascript_prints_it() {
        assert_eq!(iso_time(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso_time(1_709_251_199_999), "2024-02-29T23:59:59.999Z");
        assert_eq!(iso_time(1_790_318_781_432), "2026-09-25T06:46:21.432Z");
    }

    #[test]
    fn an_iso_time_becomes_unix_seconds_and_a_bad_one_is_none() {
        assert_eq!(unix_time("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(unix_time("2024-02-29T23:59:59.999Z"), Some(1_709_251_199));
        assert_eq!(unix_time("2026-13-01T00:00:00Z"), None);
        assert_eq!(unix_time("yesterday"), None);
        assert_eq!(unix_time("1969-12-31T23:59:59Z"), None);
    }

    #[test]
    fn a_printed_time_reads_back_as_the_same_second() {
        let mut seconds: u32 = 0;
        while seconds < u32::MAX - 997_331 {
            let printed = iso_time(u128::from(seconds) * 1000 + 999);
            assert_eq!(unix_time(&printed), Some(seconds), "{printed}");
            seconds += 997_331;
        }
    }
}
