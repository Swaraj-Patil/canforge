//! Turning the bytes of a frame into physical values.

use crate::bits::{fits, physical_value, raw_value};
use crate::model::{Database, Message, Mux, ValueType};

#[derive(Debug, Clone)]
pub struct DecodedSignal {
    pub name: String,
    pub raw: i128,
    pub physical: f64,
    pub unit: String,
    /// The value description for this raw value, if the DBC has one.
    pub label: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Decoded {
    pub message: String,
    pub frame_id: u64,
    pub extended: bool,
    /// The multiplexer's raw value, for multiplexed messages.
    pub mux: Option<i128>,
    pub signals: Vec<DecodedSignal>,
}

/// Decode one frame. Multiplexed signals are included only when the
/// multiplexer selects them. `extended` restricts the lookup to standard
/// (`Some(false)`) or extended (`Some(true)`) frames.
pub fn decode(db: &Database, frame_id: u64, data: &[u8], extended: Option<bool>) -> Result<Decoded, String> {
    match db.find_frame(frame_id, extended) {
        Some(m) => decode_message(m, data),
        None => Err(no_message(frame_id)),
    }
}

/// The error for a frame ID that no message in the database uses.
pub fn no_message(frame_id: u64) -> String {
    format!("no message with frame ID 0x{:X}", frame_id)
}

/// Decode one frame of a message already looked up, for instance through a
/// `FrameIndex`. Multiplexed signals are included only when the multiplexer
/// selects them.
pub fn decode_message(m: &Message, data: &[u8]) -> Result<Decoded, String> {
    if (data.len() as u64) < m.dlc {
        return Err(format!("message '{}' needs {} bytes, got {}", m.name, m.dlc, data.len()));
    }
    let mut mux_value: Option<i128> = None;
    for s in m.signals.iter() {
        if s.mux == Mux::Switch && s.valid_length() {
            if fits(s, data.len()) {
                mux_value = Some(raw_value(data, s));
            }
            break;
        }
    }
    let mut signals: Vec<DecodedSignal> = Vec::new();
    for s in m.signals.iter() {
        if let Mux::Value(v) = s.mux {
            if mux_value != Some(v as i128) {
                continue;
            }
        }
        if !s.valid_length() {
            return Err(format!("signal '{}' has an invalid length", s.name));
        }
        if !fits(s, data.len()) {
            return Err(format!("signal '{}' extends beyond the frame", s.name));
        }
        let raw = raw_value(data, s);
        let mut label: Option<String> = None;
        if s.value_type == ValueType::Integer {
            for (value, text) in s.choices.iter() {
                if (*value as i128) == raw {
                    label = Some(text.clone());
                    break;
                }
            }
        }
        signals.push(DecodedSignal {
            name: s.name.clone(),
            raw,
            physical: physical_value(s, raw),
            unit: s.unit.clone(),
            label,
        });
    }
    Ok(Decoded {
        message: m.name.clone(),
        frame_id: m.frame_id,
        extended: m.is_extended,
        mux: mux_value,
        signals,
    })
}

/// Parse "0x100", "256" or "0x18FF50E5" into a frame ID.
pub fn parse_frame_id(text: &str) -> Result<u64, String> {
    let t = text.trim();
    let lower = t.to_ascii_lowercase();
    let parsed = if let Some(hex) = lower.strip_prefix("0x") {
        u64::from_str_radix(hex, 16)
    } else {
        t.parse::<u64>()
    };
    match parsed {
        Ok(v) => Ok(v),
        Err(_) => Err(format!("'{}' is not a frame ID; use decimal or 0x-prefixed hex", t)),
    }
}

/// Parse frame bytes written as "e803035a", "e8 03 03 5a", "E8:03" or "0xE8 0x03".
pub fn parse_hex(text: &str) -> Result<Vec<u8>, String> {
    let mut out: Vec<u8> = Vec::new();
    let separators = |c: char| c.is_whitespace() || c == ':' || c == ',' || c == '-' || c == '_';
    for token in text.split(separators) {
        if token.is_empty() {
            continue;
        }
        let body = if token.starts_with("0x") || token.starts_with("0X") {
            &token[2..]
        } else {
            token
        };
        if body.is_empty() || !body.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!("'{}' is not hexadecimal", token));
        }
        let padded = if body.len() == 1 {
            format!("0{}", body)
        } else {
            body.to_string()
        };
        if padded.len() % 2 != 0 {
            return Err(format!("'{}' has an odd number of hex digits", token));
        }
        let mut i = 0;
        while i < padded.len() {
            match u8::from_str_radix(&padded[i..i + 2], 16) {
                Ok(b) => out.push(b),
                Err(_) => return Err(format!("'{}' is not hexadecimal", token)),
            }
            i += 2;
        }
    }
    if out.len() > 64 {
        return Err(format!("{} bytes is longer than any CAN frame (64 bytes)", out.len()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    const EXAMPLE: &str = include_str!("../examples/powertrain.dbc");

    #[test]
    fn decodes_a_frame_with_value_descriptions() {
        let db = parse(EXAMPLE).unwrap();
        let data = parse_hex("e8 03 03 5a 18 fc 00 05").unwrap();
        let d = decode(&db, 0x100, &data, None).unwrap();
        let speed = d.signals.iter().find(|s| s.name == "VehicleSpeed").unwrap();
        assert!((speed.physical - 10.0).abs() < 1e-9);
        let gear = d.signals.iter().find(|s| s.name == "GearPosition").unwrap();
        assert_eq!(gear.label.as_deref(), Some("Drive"));
        let steering = d.signals.iter().find(|s| s.name == "SteeringAngle").unwrap();
        assert_eq!(steering.raw, -1000);
    }

    #[test]
    fn multiplexer_selects_signals() {
        let db = parse(EXAMPLE).unwrap();
        let d = decode(&db, 0x300, &parse_hex("0128320000000002").unwrap(), None).unwrap();
        assert_eq!(d.mux, Some(1));
        let names: Vec<&str> = d.signals.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["PageIndex", "StatorTemp", "IgbtTemp", "InverterState"]);
        assert_eq!(d.signals[3].label.as_deref(), Some("Running"));
    }

    #[test]
    fn reports_bad_input() {
        let db = parse(EXAMPLE).unwrap();
        assert!(decode(&db, 0x7AB, &[0u8; 8], None).is_err());
        assert!(decode(&db, 0x100, &[0u8; 3], None).is_err());
        assert_eq!(parse_frame_id("0x18FF50E5").unwrap(), 0x18FF_50E5);
        assert_eq!(parse_frame_id("256").unwrap(), 256);
        assert!(parse_frame_id("zz").is_err());
        assert_eq!(parse_hex("0xE8 0x3").unwrap(), vec![0xE8, 0x03]);
        assert!(parse_hex("abc").is_err());
        assert!(parse_hex("xyz").is_err());
    }
}
