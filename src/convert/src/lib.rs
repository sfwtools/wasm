// Copyright (C) 2026, Alex Morales
// Copyright (C) 2026, sfw.tools sfwtools.com
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

//! convert - common unit conversion through runtime dispatch over uom's typed
//! quantities. The caller supplies a numeric value and unit labels in the
//! options blob; each supported label is mapped to a compile-time uom unit.

use abi::option_pairs;
use uom::si::f64::{Length, Mass, ThermodynamicTemperature, Time, Velocity};
use uom::si::length::{centimeter, foot, inch, kilometer, meter, mile, millimeter, yard};
use uom::si::mass::{gram, kilogram, milligram, ounce, pound};
use uom::si::thermodynamic_temperature::{degree_celsius, degree_fahrenheit, kelvin};
use uom::si::time::{day, hour, millisecond, minute, second};
use uom::si::velocity::{kilometer_per_hour, knot, meter_per_second, mile_per_hour};

const MANIFEST: &str = r#"{
  "exports": {
    "convert": {
      "summary": "Convert a value between compatible common units.",
      "options": {
        "value": {"type":"number","default":1},
        "from": {"type":"string","default":"Meters (m)"},
        "to": {"type":"string","default":"Feet (ft)"}
      }
    }
  }
}"#;

#[derive(Debug, PartialEq)]
struct Options {
    value: f64,
    from: String,
    to: String,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            value: 1.0,
            from: "Meters (m)".to_string(),
            to: "Feet (ft)".to_string(),
        }
    }
}

fn resolve_options(blob: &[u8]) -> Option<Options> {
    let mut options = Options::default();

    for (key, value) in option_pairs(blob)? {
        match key {
            b"value" => options.value = std::str::from_utf8(value).ok()?.parse().ok()?,
            b"from" => options.from = std::str::from_utf8(value).ok()?.to_string(),
            b"to" => options.to = std::str::from_utf8(value).ok()?.to_string(),
            _ => {}
        }
    }

    if !options.value.is_finite() {
        return None;
    }

    Some(options)
}

fn length(value: f64, unit: &str) -> Option<Length> {
    Some(match unit {
        "Millimeters (mm)" => Length::new::<millimeter>(value),
        "Centimeters (cm)" => Length::new::<centimeter>(value),
        "Meters (m)" => Length::new::<meter>(value),
        "Kilometers (km)" => Length::new::<kilometer>(value),
        "Inches (in)" => Length::new::<inch>(value),
        "Feet (ft)" => Length::new::<foot>(value),
        "Yards (yd)" => Length::new::<yard>(value),
        "Miles (mi)" => Length::new::<mile>(value),
        _ => return None,
    })
}

fn mass(value: f64, unit: &str) -> Option<Mass> {
    Some(match unit {
        "Milligrams (mg)" => Mass::new::<milligram>(value),
        "Grams (g)" => Mass::new::<gram>(value),
        "Kilograms (kg)" => Mass::new::<kilogram>(value),
        "Metric tonnes (t)" => Mass::new::<kilogram>(value * 1000.0),
        "Ounces (oz)" => Mass::new::<ounce>(value),
        "Pounds (lb)" => Mass::new::<pound>(value),
        _ => return None,
    })
}

fn temperature(value: f64, unit: &str) -> Option<ThermodynamicTemperature> {
    Some(match unit {
        "Celsius (°C)" => ThermodynamicTemperature::new::<degree_celsius>(value),
        "Fahrenheit (°F)" => ThermodynamicTemperature::new::<degree_fahrenheit>(value),
        "Kelvin (K)" => ThermodynamicTemperature::new::<kelvin>(value),
        _ => return None,
    })
}

fn time(value: f64, unit: &str) -> Option<Time> {
    Some(match unit {
        "Milliseconds (ms)" => Time::new::<millisecond>(value),
        "Seconds (s)" => Time::new::<second>(value),
        "Minutes (min)" => Time::new::<minute>(value),
        "Hours (h)" => Time::new::<hour>(value),
        "Days (d)" => Time::new::<day>(value),
        "Weeks (wk)" => Time::new::<day>(value * 7.0),
        _ => return None,
    })
}

fn velocity(value: f64, unit: &str) -> Option<Velocity> {
    Some(match unit {
        "Meters per second (m/s)" => Velocity::new::<meter_per_second>(value),
        "Kilometers per hour (km/h)" => Velocity::new::<kilometer_per_hour>(value),
        "Miles per hour (mph)" => Velocity::new::<mile_per_hour>(value),
        "Knots (kn)" => Velocity::new::<knot>(value),
        _ => return None,
    })
}

fn category(unit: &str) -> Option<&'static str> {
    if length(0.0, unit).is_some() {
        return Some("length");
    }
    if mass(0.0, unit).is_some() {
        return Some("mass");
    }
    if temperature(0.0, unit).is_some() {
        return Some("temperature");
    }
    if time(0.0, unit).is_some() {
        return Some("time");
    }
    if velocity(0.0, unit).is_some() {
        return Some("speed");
    }
    None
}

pub fn convert_value(value: f64, from: &str, to: &str) -> Result<f64, &'static str> {
    let from_category = category(from).ok_or("unknown source unit")?;
    let to_category = category(to).ok_or("unknown target unit")?;

    if from_category != to_category {
        return Err("source and target units must measure the same kind of quantity");
    }

    Ok(match from_category {
        "length" => convert_length(length(value, from).unwrap(), to)?,
        "mass" => convert_mass(mass(value, from).unwrap(), to)?,
        "temperature" => convert_temperature(temperature(value, from).unwrap(), to)?,
        "time" => convert_time(time(value, from).unwrap(), to)?,
        _ => convert_velocity(velocity(value, from).unwrap(), to)?,
    })
}

fn convert_length(value: Length, to: &str) -> Result<f64, &'static str> {
    Ok(match to {
        "Millimeters (mm)" => value.get::<millimeter>(),
        "Centimeters (cm)" => value.get::<centimeter>(),
        "Meters (m)" => value.get::<meter>(),
        "Kilometers (km)" => value.get::<kilometer>(),
        "Inches (in)" => value.get::<inch>(),
        "Feet (ft)" => value.get::<foot>(),
        "Yards (yd)" => value.get::<yard>(),
        "Miles (mi)" => value.get::<mile>(),
        _ => return Err("unknown target unit"),
    })
}

fn convert_mass(value: Mass, to: &str) -> Result<f64, &'static str> {
    Ok(match to {
        "Milligrams (mg)" => value.get::<milligram>(),
        "Grams (g)" => value.get::<gram>(),
        "Kilograms (kg)" => value.get::<kilogram>(),
        "Metric tonnes (t)" => value.get::<kilogram>() / 1000.0,
        "Ounces (oz)" => value.get::<ounce>(),
        "Pounds (lb)" => value.get::<pound>(),
        _ => return Err("unknown target unit"),
    })
}

fn convert_temperature(value: ThermodynamicTemperature, to: &str) -> Result<f64, &'static str> {
    Ok(match to {
        "Celsius (°C)" => value.get::<degree_celsius>(),
        "Fahrenheit (°F)" => value.get::<degree_fahrenheit>(),
        "Kelvin (K)" => value.get::<kelvin>(),
        _ => return Err("unknown target unit"),
    })
}

fn convert_time(value: Time, to: &str) -> Result<f64, &'static str> {
    Ok(match to {
        "Milliseconds (ms)" => value.get::<millisecond>(),
        "Seconds (s)" => value.get::<second>(),
        "Minutes (min)" => value.get::<minute>(),
        "Hours (h)" => value.get::<hour>(),
        "Days (d)" => value.get::<day>(),
        "Weeks (wk)" => value.get::<day>() / 7.0,
        _ => return Err("unknown target unit"),
    })
}

fn convert_velocity(value: Velocity, to: &str) -> Result<f64, &'static str> {
    Ok(match to {
        "Meters per second (m/s)" => value.get::<meter_per_second>(),
        "Kilometers per hour (km/h)" => value.get::<kilometer_per_hour>(),
        "Miles per hour (mph)" => value.get::<mile_per_hour>(),
        "Knots (kn)" => value.get::<knot>(),
        _ => return Err("unknown target unit"),
    })
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
pub unsafe extern "C" fn convert(ptr: u32, len: u32, opts_ptr: u32, opts_len: u32) -> u64 {
    let input = std::slice::from_raw_parts(ptr as *const u8, len as usize);

    if !input.is_empty() {
        return 0;
    }

    let options = std::slice::from_raw_parts(opts_ptr as *const u8, opts_len as usize);
    let options = match resolve_options(options) {
        Some(options) => options,
        None => return 0,
    };
    let value = match convert_value(options.value, &options.from, &options.to) {
        Ok(value) => value,
        Err(_) => return 0,
    };

    abi::pack(format!("{{\"value\":{},\"unit\":\"{}\"}}", value, options.to).into_bytes())
}

#[no_mangle]
pub unsafe extern "C" fn manifest() -> u64 {
    abi::pack(MANIFEST.as_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_across_typed_categories() {
        assert!(
            (convert_value(10.0, "Meters (m)", "Feet (ft)").unwrap() - 32.80839895).abs()
                < 0.000001
        );
        assert!(
            (convert_value(0.0, "Celsius (°C)", "Fahrenheit (°F)").unwrap() - 32.0).abs()
                < 0.000001
        );
        assert!(
            (convert_value(1.0, "Kilometers per hour (km/h)", "Miles per hour (mph)").unwrap()
                - 0.621371192)
                .abs()
                < 0.000001
        );
    }

    #[test]
    fn rejects_incompatible_units() {
        assert!(convert_value(1.0, "Meters (m)", "Kilograms (kg)").is_err());
        assert!(convert_value(1.0, "unknown", "Meters (m)").is_err());
    }
}
