//! Text diagrams of where each signal sits in a frame, for the CLI.

use crate::bits::signal_bits;
use crate::codegen_c::split_groups;
use crate::model::{Database, Message, Signal};
use crate::numfmt::fmt_f64;

const LABELS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

fn label(i: usize) -> char {
    if i < LABELS.len() {
        LABELS[i] as char
    } else {
        '?'
    }
}

fn spec(s: &Signal) -> String {
    format!(
        "{}|{}@{}{}",
        s.start,
        s.length,
        if s.little_endian { "1" } else { "0" },
        if s.signed { "-" } else { "+" }
    )
}

/// One grid: rows are bytes, columns are bits 7..0, cells name the signal.
fn grid(m: &Message, shown: &[usize]) -> Vec<String> {
    let rows = m.dlc as usize;
    let mut cells: Vec<char> = vec!['.'; rows * 8];
    for &si in shown.iter() {
        let s = &m.signals[si];
        if !s.valid_length() {
            continue;
        }
        for &(byte, bit) in signal_bits(s).iter() {
            if byte < m.dlc {
                let idx = byte as usize * 8 + bit as usize;
                cells[idx] = if cells[idx] == '.' { label(si) } else { '#' };
            }
        }
    }
    let mut out: Vec<String> = Vec::new();
    out.push("         7  6  5  4  3  2  1  0".to_string());
    for r in 0..rows {
        let mut line = format!("  B{:<3}  ", r);
        for bit in (0..8).rev() {
            line.push(' ');
            line.push(cells[r * 8 + bit]);
            line.push(' ');
        }
        out.push(line.trim_end().to_string());
    }
    out
}

/// A layout diagram for one message, with one grid per multiplexer page.
pub fn render_message(m: &Message) -> String {
    let mut out: Vec<String> = Vec::new();
    let sender = if m.sender == crate::model::NO_NODE {
        String::new()
    } else {
        format!(", sent by {}", m.sender)
    };
    out.push(format!("{}  {}  {} bytes{}", m.name, m.id_hex(), m.dlc, sender));
    let g = split_groups(m);
    if g.groups.is_empty() {
        let all: Vec<usize> = (0..m.signals.len()).collect();
        out.push(String::new());
        out.extend(grid(m, &all));
    } else {
        let mux_name = match g.mux {
            Some(i) => m.signals[i].name.clone(),
            None => "multiplexer".to_string(),
        };
        for (value, members) in g.groups.iter() {
            let mut shown: Vec<usize> = g.plain.clone();
            shown.extend(members.iter().cloned());
            out.push(String::new());
            out.push(format!("  when {} = {}", mux_name, value));
            out.extend(grid(m, &shown));
        }
    }
    out.push(String::new());
    let width = m.signals.iter().map(|s| s.name.len()).max().unwrap_or(0);
    for (i, s) in m.signals.iter().enumerate() {
        let mut line = format!("  {}  {:<w$}  {:<10}", label(i), s.name, spec(s), w = width);
        if s.factor != 1.0 || s.offset != 0.0 {
            line.push_str(&format!("  scale {} offset {}", fmt_f64(s.factor), fmt_f64(s.offset)));
        }
        if !s.unit.is_empty() {
            line.push_str(&format!("  {}", s.unit));
        }
        if let Some(v) = s.mux_value() {
            line.push_str(&format!("  (only when multiplexer = {})", v));
        } else if s.mux == crate::model::Mux::Switch {
            line.push_str("  (multiplexer)");
        }
        out.push(line.trim_end().to_string());
    }
    out.push(String::new());
    out.join("\n")
}

/// Layout diagrams for every message, or for the one named `only`.
pub fn render(db: &Database, only: Option<&str>) -> Result<String, String> {
    let mut parts: Vec<String> = Vec::new();
    for m in db.messages.iter() {
        if let Some(name) = only {
            if m.name != name {
                continue;
            }
        }
        parts.push(render_message(m));
    }
    if parts.is_empty() {
        return match only {
            Some(name) => Err(format!("no message named '{}'", name)),
            None => Ok("This database has no messages.\n".to_string()),
        };
    }
    Ok(parts.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    #[test]
    fn draws_motorola_signals_across_bytes() {
        let db = parse(include_str!("../examples/powertrain.dbc")).unwrap();
        let text = render(&db, Some("WheelSpeeds")).unwrap();
        // WheelSpeedFL (A) fills byte 0 and the top nibble of byte 1;
        // WheelSpeedFR (B) takes the bottom nibble of byte 1.
        assert!(text.contains("B0     A  A  A  A  A  A  A  A"), "{}", text);
        assert!(text.contains("B1     A  A  A  A  B  B  B  B"), "{}", text);
        assert!(render(&db, Some("Nope")).is_err());
    }

    #[test]
    fn draws_one_grid_per_multiplexer_page() {
        let db = parse(include_str!("../examples/powertrain.dbc")).unwrap();
        let text = render(&db, Some("InverterTelemetry")).unwrap();
        assert!(text.contains("when PageIndex = 0"));
        assert!(text.contains("when PageIndex = 2"));
    }
}
