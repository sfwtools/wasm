// Copyright (C) 2026, Alex Morales
// Copyright (C) 2026, sfw.tools sfwtools.com
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

//! datetime - date arithmetic and date/time differences through the raw ABI.
//! Calendar units are intentionally distinct from elapsed units: one month is
//! the corresponding calendar month, with invalid month-end dates clamped.

use abi::option_pairs;
use chrono::{
    DateTime, Datelike, Duration, FixedOffset, Months, NaiveDate, NaiveDateTime, TimeZone,
};

const MANIFEST: &str = r#"{
  "exports": {
    "difference": {
      "summary": "Calculate calendar and elapsed differences between two dates or timestamps.",
      "options": {
        "start": {"type":"string","default":"2026-03-09"},
        "end": {"type":"string","default":"2026-04-09"}
      }
    },
    "add": {
      "summary": "Add a calendar or elapsed amount to a date or timestamp.",
      "options": {
        "date": {"type":"string","default":"2026-03-09"},
        "unit": {"type":"string","default":"months"},
        "amount": {"type":"number","default":1}
      }
    },
    "subtract": {
      "summary": "Subtract a calendar or elapsed amount from a date or timestamp.",
      "options": {
        "date": {"type":"string","default":"2026-03-09"},
        "unit": {"type":"string","default":"months"},
        "amount": {"type":"number","default":1}
      }
    },
    "calendar_info": {
      "summary": "Return calendar information for a date or timestamp.",
      "options": {
        "date": {"type":"string","default":"2026-03-09"}
      }
    }
  }
}"#;

#[derive(Debug, PartialEq)]
struct Options {
    first: String,
    second: String,
    unit: String,
    amount: i64,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            first: "2026-03-09".to_string(),
            second: "2026-04-09".to_string(),
            unit: "months".to_string(),
            amount: 1,
        }
    }
}

fn resolve_options(blob: &[u8]) -> Option<Options> {
    let mut options = Options::default();

    for (key, value) in option_pairs(blob)? {
        let value = std::str::from_utf8(value).ok()?;

        match key {
            b"start" | b"date" => options.first = value.to_string(),
            b"end" => options.second = value.to_string(),
            b"unit" => options.unit = value.to_string(),
            b"amount" => options.amount = value.parse().ok()?,
            _ => {}
        }
    }

    Some(options)
}

#[derive(Debug, PartialEq)]
enum ParsedDate {
    Date(NaiveDate),
    DateTime(DateTime<FixedOffset>),
}

fn parse_date(value: &str) -> Option<ParsedDate> {
    if let Ok(date) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        return Some(ParsedDate::Date(date));
    }

    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(ParsedDate::DateTime)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let next = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1).unwrap()
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1).unwrap()
    };

    (next - Duration::days(1)).day()
}

fn add_calendar_months(date: NaiveDate, months: i64) -> Option<NaiveDate> {
    if months >= 0 {
        return date.checked_add_months(Months::new(u32::try_from(months).ok()?));
    }

    date.checked_sub_months(Months::new(u32::try_from(months.unsigned_abs()).ok()?))
}

fn add_calendar_years(date: NaiveDate, years: i64) -> Option<NaiveDate> {
    let year = date.year().checked_add(i32::try_from(years).ok()?)?;
    let day = date.day().min(days_in_month(year, date.month()));

    NaiveDate::from_ymd_opt(year, date.month(), day)
}

fn calendar_shift(date: NaiveDate, unit: &str, amount: i64) -> Option<NaiveDate> {
    match unit {
        "years" => add_calendar_years(date, amount),
        "months" => add_calendar_months(date, amount),
        "weeks" => date.checked_add_signed(Duration::weeks(amount)),
        "days" => date.checked_add_signed(Duration::days(amount)),
        _ => None,
    }
}

fn shift(parsed: ParsedDate, unit: &str, amount: i64) -> Option<ParsedDate> {
    match parsed {
        ParsedDate::Date(date) => calendar_shift(date, unit, amount).map(ParsedDate::Date),
        ParsedDate::DateTime(date_time) => {
            if matches!(unit, "years" | "months") {
                let date = calendar_shift(date_time.date_naive(), unit, amount)?;
                let local = NaiveDateTime::new(date, date_time.time());

                return date_time
                    .offset()
                    .from_local_datetime(&local)
                    .single()
                    .map(ParsedDate::DateTime);
            }

            let duration = match unit {
                "weeks" => Duration::weeks(amount),
                "days" => Duration::days(amount),
                "hours" => Duration::hours(amount),
                "minutes" => Duration::minutes(amount),
                "seconds" => Duration::seconds(amount),
                _ => return None,
            };

            date_time
                .checked_add_signed(duration)
                .map(ParsedDate::DateTime)
        }
    }
}

fn format_date(parsed: &ParsedDate) -> String {
    match parsed {
        ParsedDate::Date(date) => date.format("%Y-%m-%d").to_string(),
        ParsedDate::DateTime(date_time) => date_time.to_rfc3339(),
    }
}

fn calendar_difference(start: NaiveDate, end: NaiveDate) -> (i64, i64, i64) {
    if start > end {
        let (years, months, days) = calendar_difference(end, start);
        return (-years, -months, -days);
    }

    let mut years = i64::from(end.year() - start.year());
    let mut cursor = add_calendar_years(start, years).unwrap();

    if cursor > end {
        years -= 1;
        cursor = add_calendar_years(start, years).unwrap();
    }

    let mut months = i64::from(end.month()) - i64::from(cursor.month());
    months += i64::from(end.year() - cursor.year()) * 12;
    let candidate = add_calendar_months(cursor, months).unwrap();

    if candidate > end {
        months -= 1;
        cursor = add_calendar_months(cursor, months).unwrap();
    } else {
        cursor = candidate;
    }

    (years, months, (end - cursor).num_days())
}

fn json_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn difference_json(options: Options) -> Option<String> {
    let start = parse_date(&options.first)?;
    let end = parse_date(&options.second)?;

    match (start, end) {
        (ParsedDate::Date(start), ParsedDate::Date(end)) => {
            let duration = end.signed_duration_since(start);
            let (years, months, days) = calendar_difference(start, end);
            Some(format!(
                "{{\"calendar_years\":{},\"calendar_months\":{},\"calendar_days\":{},\"elapsed_seconds\":{},\"elapsed_minutes\":{},\"elapsed_hours\":{},\"elapsed_days\":{},\"elapsed_weeks\":{}}}",
                years, months, days, duration.num_seconds(), duration.num_minutes(), duration.num_hours(), duration.num_days(), duration.num_weeks()
            ))
        }
        (ParsedDate::DateTime(start), ParsedDate::DateTime(end)) => {
            let duration = end.signed_duration_since(start);
            let (years, months, days) = calendar_difference(start.date_naive(), end.date_naive());
            Some(format!(
                "{{\"calendar_years\":{},\"calendar_months\":{},\"calendar_days\":{},\"elapsed_seconds\":{},\"elapsed_minutes\":{},\"elapsed_hours\":{},\"elapsed_days\":{},\"elapsed_weeks\":{}}}",
                years, months, days, duration.num_seconds(), duration.num_minutes(), duration.num_hours(), duration.num_days(), duration.num_weeks()
            ))
        }
        _ => None,
    }
}

fn info(date: ParsedDate) -> String {
    let date = match date {
        ParsedDate::Date(date) => date,
        ParsedDate::DateTime(date_time) => date_time.date_naive(),
    };
    let days = days_in_month(date.year(), date.month());
    let day_of_year = date.ordinal();
    let days_in_year = if NaiveDate::from_ymd_opt(date.year(), 2, 29).is_some() {
        366
    } else {
        365
    };

    format!(
        "{{\"year\":{},\"month\":{},\"day\":{},\"day_of_week\":\"{:?}\",\"day_of_year\":{},\"days_in_month\":{},\"days_in_year\":{},\"is_leap_year\":{}}}",
        date.year(), date.month(), date.day(), date.weekday(), day_of_year, days, days_in_year, date.leap_year()
    )
}

#[no_mangle]
pub unsafe extern "C" fn alloc(len: u32) -> u32 {
    abi::alloc_buf(len)
}

#[no_mangle]
pub unsafe extern "C" fn dealloc(ptr: u32, len: u32) {
    abi::free_buf(ptr, len)
}

#[no_mangle]
pub unsafe extern "C" fn difference(ptr: u32, len: u32, opts_ptr: u32, opts_len: u32) -> u64 {
    export_json(ptr, len, opts_ptr, opts_len, difference_json)
}

#[no_mangle]
pub unsafe extern "C" fn add(ptr: u32, len: u32, opts_ptr: u32, opts_len: u32) -> u64 {
    export_shift(ptr, len, opts_ptr, opts_len, 1)
}

#[no_mangle]
pub unsafe extern "C" fn subtract(ptr: u32, len: u32, opts_ptr: u32, opts_len: u32) -> u64 {
    export_shift(ptr, len, opts_ptr, opts_len, -1)
}

#[no_mangle]
pub unsafe extern "C" fn calendar_info(ptr: u32, len: u32, opts_ptr: u32, opts_len: u32) -> u64 {
    export_json(ptr, len, opts_ptr, opts_len, |options| {
        Some(info(parse_date(&options.first)?))
    })
}

unsafe fn export_json<F>(ptr: u32, len: u32, opts_ptr: u32, opts_len: u32, operation: F) -> u64
where
    F: FnOnce(Options) -> Option<String>,
{
    let input = std::slice::from_raw_parts(ptr as *const u8, len as usize);

    if !input.is_empty() {
        return 0;
    }

    let blob = std::slice::from_raw_parts(opts_ptr as *const u8, opts_len as usize);
    let options = match resolve_options(blob) {
        Some(options) => options,
        None => return 0,
    };

    match operation(options) {
        Some(output) => abi::pack(output.into_bytes()),
        None => 0,
    }
}

unsafe fn export_shift(ptr: u32, len: u32, opts_ptr: u32, opts_len: u32, direction: i64) -> u64 {
    export_json(ptr, len, opts_ptr, opts_len, |options| {
        let date = parse_date(&options.first)?;
        let shifted = shift(date, &options.unit, options.amount.checked_mul(direction)?)?;
        Some(format!(
            "{{\"date\":\"{}\"}}",
            json_string(&format_date(&shifted))
        ))
    })
}

#[no_mangle]
pub unsafe extern "C" fn manifest() -> u64 {
    abi::pack(MANIFEST.as_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_calendar_months() {
        let result = shift(
            ParsedDate::Date(NaiveDate::from_ymd_opt(2026, 3, 9).unwrap()),
            "months",
            1,
        )
        .unwrap();
        assert_eq!(format_date(&result), "2026-04-09");
    }

    #[test]
    fn clamps_month_end() {
        let result = shift(
            ParsedDate::Date(NaiveDate::from_ymd_opt(2026, 1, 31).unwrap()),
            "months",
            1,
        )
        .unwrap();
        assert_eq!(format_date(&result), "2026-02-28");
    }

    #[test]
    fn calculates_date_difference() {
        let result = difference_json(Options {
            first: "2026-03-09".to_string(),
            second: "2026-04-09".to_string(),
            ..Options::default()
        })
        .unwrap();
        assert!(result.contains("\"calendar_months\":1"));
        assert!(result.contains("\"elapsed_days\":31"));
    }

    #[test]
    fn calculates_timestamp_difference() {
        let result = difference_json(Options {
            first: "2026-03-09T09:00:00Z".to_string(),
            second: "2026-03-09T12:30:00Z".to_string(),
            ..Options::default()
        })
        .unwrap();
        assert!(result.contains("\"elapsed_hours\":3"));
        assert!(result.contains("\"elapsed_minutes\":210"));
    }

    #[test]
    fn reports_calendar_information() {
        let result = info(parse_date("2024-02-29").unwrap());
        assert!(result.contains("\"days_in_year\":366"));
        assert!(result.contains("\"is_leap_year\":true"));

        let ordinary = info(parse_date("2026-03-31").unwrap());
        assert!(ordinary.contains("\"days_in_year\":365"));
    }
}
