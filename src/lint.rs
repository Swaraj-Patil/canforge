//! Checks that catch DBC mistakes before they reach a vehicle.
//!
//! Message text and the order of findings match the reference model exactly;
//! `tests/golden/lint.tsv` holds the expected output for every fixture.

use crate::bits::{raw_range, signal_bits};
use crate::model::{Database, Mux, Signal, ValueType, NO_NODE};
use crate::names::c_ident;
use crate::numfmt::fmt_f64;

/// A lint rule: stable ID, short name, severity and one-line summary.
pub struct Rule {
    pub id: &'static str,
    pub name: &'static str,
    pub severity: &'static str,
    pub summary: &'static str,
}

pub const RULES: &[Rule] = &[
    Rule { id: "E001", name: "signal-overlap", severity: "error", summary: "Two signals in the same frame use the same bits." },
    Rule { id: "E002", name: "signal-out-of-frame", severity: "error", summary: "A signal extends past the end of its frame." },
    Rule { id: "E003", name: "duplicate-frame-id", severity: "error", summary: "Two messages share a frame ID." },
    Rule { id: "E004", name: "duplicate-name", severity: "error", summary: "A message or signal name is defined twice." },
    Rule { id: "E005", name: "zero-factor", severity: "error", summary: "A signal's scaling factor is zero, so it cannot be encoded." },
    Rule { id: "E006", name: "invalid-length", severity: "error", summary: "A signal is shorter than 1 bit or longer than 64 bits." },
    Rule { id: "E007", name: "frame-id-out-of-range", severity: "error", summary: "A frame ID does not fit its 11-bit or 29-bit format." },
    Rule { id: "E008", name: "multiplexed-without-multiplexer", severity: "error", summary: "A multiplexed signal has no multiplexer to select it." },
    Rule { id: "E009", name: "multiple-multiplexers", severity: "error", summary: "A message declares more than one multiplexer." },
    Rule { id: "E010", name: "invalid-frame-length", severity: "error", summary: "A frame length is not a valid CAN or CAN FD length." },
    Rule { id: "E011", name: "float-length-mismatch", severity: "error", summary: "A float signal is not 32 or 64 bits long." },
    Rule { id: "E012", name: "multiplex-value-out-of-range", severity: "error", summary: "A multiplexer value cannot be represented by the multiplexer signal." },
    Rule { id: "W001", name: "range-not-representable", severity: "warning", summary: "A declared range extends past what the raw bits can encode." },
    Rule { id: "W002", name: "min-greater-than-max", severity: "warning", summary: "A declared minimum is greater than the maximum." },
    Rule { id: "W003", name: "unknown-node", severity: "warning", summary: "A transmitter or receiver is not declared in BU_." },
    Rule { id: "W004", name: "c-name-collision", severity: "warning", summary: "Two signal names map to the same C identifier." },
    Rule { id: "W005", name: "choice-out-of-range", severity: "warning", summary: "A value description uses a raw value the signal cannot hold." },
    Rule { id: "I001", name: "unused-node", severity: "info", summary: "A node never transmits or receives anything." },
    Rule { id: "I002", name: "can-fd-frame", severity: "info", summary: "A frame is longer than 8 bytes and needs CAN FD." },
];

/// Look up a rule by ID.
pub fn rule(id: &str) -> &'static Rule {
    for r in RULES.iter() {
        if r.id == id {
            return r;
        }
    }
    &RULES[0]
}

/// One finding.
#[derive(Debug, Clone)]
pub struct Diag {
    pub rule: &'static str,
    pub name: &'static str,
    pub severity: &'static str,
    pub message: String,
    pub line: usize,
    pub message_name: String,
    pub signal_name: String,
}

fn diag(id: &str, message: String, line: usize, msg_name: &str, sig_name: &str) -> Diag {
    let r = rule(id);
    Diag {
        rule: r.id,
        name: r.name,
        severity: r.severity,
        message,
        line,
        message_name: msg_name.to_string(),
        signal_name: sig_name.to_string(),
    }
}

const FD_LENGTHS: [u64; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 12, 16, 20, 24, 32, 48, 64];

/// Overlapping bits are fine only between alternatives of one multiplexer.
fn mux_conflict(a: &Signal, b: &Signal) -> bool {
    match (a.mux, b.mux) {
        (Mux::Value(x), Mux::Value(y)) => x == y,
        _ => true,
    }
}

fn absolute_bits(s: &Signal) -> Vec<u64> {
    let mut v: Vec<u64> = signal_bits(s)
        .iter()
        .map(|&(byte, bit)| byte * 8 + bit as u64)
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

fn fmt_bits(bits: &[u64]) -> String {
    let shown: Vec<String> = bits.iter().take(8).map(|b| b.to_string()).collect();
    let mut s = shown.join(", ");
    if bits.len() > 8 {
        s.push_str(&format!(" (+{} more)", bits.len() - 8));
    }
    s
}

/// Run every rule over the database.
pub fn lint(db: &Database) -> Vec<Diag> {
    let mut out: Vec<Diag> = Vec::new();
    let node_names: Vec<&str> = db.nodes.iter().map(|n| n.name.as_str()).collect();
    let check_nodes = !node_names.is_empty();

    for (i, m) in db.messages.iter().enumerate() {
        for o in db.messages[..i].iter() {
            if o.frame_id == m.frame_id && o.is_extended == m.is_extended {
                out.push(diag(
                    "E003",
                    format!("frame ID {} is used by both '{}' and '{}'", m.id_hex(), o.name, m.name),
                    m.line,
                    &m.name,
                    "",
                ));
                break;
            }
        }
        for o in db.messages[..i].iter() {
            if o.name == m.name {
                out.push(diag(
                    "E004",
                    format!("message name '{}' is defined more than once", m.name),
                    m.line,
                    &m.name,
                    "",
                ));
                break;
            }
        }
    }

    for m in db.messages.iter() {
        if m.is_extended {
            if m.frame_id > 0x1FFF_FFFF {
                out.push(diag(
                    "E007",
                    format!("extended frame ID 0x{:X} exceeds the 29-bit range", m.frame_id),
                    m.line,
                    &m.name,
                    "",
                ));
            }
        } else if m.frame_id > 0x7FF {
            out.push(diag(
                "E007",
                format!(
                    "frame ID 0x{:X} exceeds the 11-bit standard range; set bit 31 to mark it extended or fix the ID",
                    m.frame_id
                ),
                m.line,
                &m.name,
                "",
            ));
        }

        if !FD_LENGTHS.contains(&m.dlc) {
            out.push(diag(
                "E010",
                format!(
                    "message '{}' has length {}; CAN frames must be 0-8 bytes and CAN FD frames 12, 16, 20, 24, 32, 48 or 64",
                    m.name, m.dlc
                ),
                m.line,
                &m.name,
                "",
            ));
        } else if m.dlc > 8 {
            out.push(diag(
                "I002",
                format!("message '{}' is {} bytes long, so it needs CAN FD", m.name, m.dlc),
                m.line,
                &m.name,
                "",
            ));
        }

        if check_nodes && m.sender != NO_NODE && !node_names.contains(&m.sender.as_str()) {
            out.push(diag(
                "W003",
                format!("transmitter '{}' of message '{}' is not declared in BU_", m.sender, m.name),
                m.line,
                &m.name,
                "",
            ));
        }

        let multiplexers: Vec<&Signal> = m.signals.iter().filter(|s| s.mux == Mux::Switch).collect();
        if multiplexers.len() > 1 {
            out.push(diag(
                "E009",
                format!(
                    "message '{}' has more than one multiplexer ('{}' and '{}')",
                    m.name, multiplexers[0].name, multiplexers[1].name
                ),
                multiplexers[1].line,
                &m.name,
                &multiplexers[1].name,
            ));
        }
        let mux_sig: Option<&Signal> = multiplexers.first().copied();

        for (i, s) in m.signals.iter().enumerate() {
            for o in m.signals[..i].iter() {
                if o.name == s.name {
                    out.push(diag(
                        "E004",
                        format!("signal name '{}' is defined more than once in message '{}'", s.name, m.name),
                        s.line,
                        &m.name,
                        &s.name,
                    ));
                    break;
                }
            }

            if !s.valid_length() {
                out.push(diag(
                    "E006",
                    format!("signal '{}' has length {}; lengths must be between 1 and 64 bits", s.name, s.length),
                    s.line,
                    &m.name,
                    &s.name,
                ));
            }
            if s.factor == 0.0 {
                out.push(diag(
                    "E005",
                    format!("signal '{}' has a scaling factor of 0", s.name),
                    s.line,
                    &m.name,
                    &s.name,
                ));
            }
            if s.value_type == ValueType::Float32 && s.length != 32 {
                out.push(diag(
                    "E011",
                    format!("signal '{}' is declared float32 but is {} bits long (expected 32)", s.name, s.length),
                    s.line,
                    &m.name,
                    &s.name,
                ));
            }
            if s.value_type == ValueType::Float64 && s.length != 64 {
                out.push(diag(
                    "E011",
                    format!("signal '{}' is declared float64 but is {} bits long (expected 64)", s.name, s.length),
                    s.line,
                    &m.name,
                    &s.name,
                ));
            }

            if let Mux::Value(v) = s.mux {
                match mux_sig {
                    None => out.push(diag(
                        "E008",
                        format!(
                            "signal '{}' is multiplexed (m{}) but message '{}' has no multiplexer signal",
                            s.name, v, m.name
                        ),
                        s.line,
                        &m.name,
                        &s.name,
                    )),
                    Some(ms) => {
                        if ms.valid_length() {
                            let (lo, hi) = raw_range(ms.length, ms.signed);
                            let value = v as i128;
                            if value < lo || value > hi {
                                out.push(diag(
                                    "E012",
                                    format!(
                                        "signal '{}' uses multiplexer value {}, which multiplexer '{}' cannot represent",
                                        s.name, v, ms.name
                                    ),
                                    s.line,
                                    &m.name,
                                    &s.name,
                                ));
                            }
                        }
                    }
                }
            }

            if s.minimum > s.maximum {
                out.push(diag(
                    "W002",
                    format!(
                        "signal '{}' has minimum {} greater than maximum {}",
                        s.name,
                        fmt_f64(s.minimum),
                        fmt_f64(s.maximum)
                    ),
                    s.line,
                    &m.name,
                    &s.name,
                ));
            } else if s.valid_length()
                && s.value_type == ValueType::Integer
                && s.factor != 0.0
                && !s.has_unspecified_range()
            {
                let (rmin, rmax) = raw_range(s.length, s.signed);
                let a = (rmin as f64) * s.factor + s.offset;
                let b = (rmax as f64) * s.factor + s.offset;
                let lo_p = if a <= b { a } else { b };
                let hi_p = if a >= b { a } else { b };
                let tol = 1e-6 * f64::max(f64::max(1.0, lo_p.abs()), hi_p.abs());
                if s.minimum < lo_p - tol || s.maximum > hi_p + tol {
                    out.push(diag(
                        "W001",
                        format!(
                            "signal '{}' declares range [{}|{}] but its {}-bit raw value can only represent [{}|{}]",
                            s.name,
                            fmt_f64(s.minimum),
                            fmt_f64(s.maximum),
                            s.length,
                            fmt_f64(lo_p),
                            fmt_f64(hi_p)
                        ),
                        s.line,
                        &m.name,
                        &s.name,
                    ));
                }
            }

            if check_nodes {
                let mut seen: Vec<&str> = Vec::new();
                for r in s.receivers.iter() {
                    let r = r.as_str();
                    if r != NO_NODE && !node_names.contains(&r) && !seen.contains(&r) {
                        seen.push(r);
                        out.push(diag(
                            "W003",
                            format!("receiver '{}' of signal '{}' is not declared in BU_", r, s.name),
                            s.line,
                            &m.name,
                            &s.name,
                        ));
                    }
                }
            }

            if s.valid_length() && s.value_type == ValueType::Integer {
                let (lo, hi) = raw_range(s.length, s.signed);
                for (value, label) in s.choices.iter() {
                    let v = *value as i128;
                    if v < lo || v > hi {
                        out.push(diag(
                            "W005",
                            format!(
                                "value description {} (\"{}\") for signal '{}' is outside its raw range [{}|{}]",
                                value, label, s.name, lo, hi
                            ),
                            s.line,
                            &m.name,
                            &s.name,
                        ));
                    }
                }
            }

            if s.valid_length() {
                let beyond = signal_bits(s).iter().any(|&(byte, _)| byte >= m.dlc);
                if beyond {
                    out.push(diag(
                        "E002",
                        format!(
                            "signal '{}' extends beyond the {}-byte frame of message '{}'",
                            s.name, m.dlc, m.name
                        ),
                        s.line,
                        &m.name,
                        &s.name,
                    ));
                }
            }
        }

        let valid: Vec<&Signal> = m.signals.iter().filter(|s| s.valid_length()).collect();
        for i in 0..valid.len() {
            let a = valid[i];
            let a_bits = absolute_bits(a);
            for b in valid[i + 1..].iter() {
                if !mux_conflict(a, b) {
                    continue;
                }
                let b_bits = absolute_bits(b);
                let common: Vec<u64> = a_bits.iter().filter(|x| b_bits.contains(*x)).cloned().collect();
                if !common.is_empty() {
                    out.push(diag(
                        "E001",
                        format!(
                            "signals '{}' and '{}' in message '{}' overlap at bit {}",
                            a.name,
                            b.name,
                            m.name,
                            fmt_bits(&common)
                        ),
                        b.line,
                        &m.name,
                        &b.name,
                    ));
                }
            }
        }

        let mut idents: Vec<(String, String)> = Vec::new();
        for s in m.signals.iter() {
            let ident = c_ident(&s.name);
            let first: Option<String> = idents.iter().find(|pair| pair.0 == ident).map(|pair| pair.1.clone());
            match first {
                Some(first_name) => {
                    if first_name != s.name {
                        out.push(diag(
                            "W004",
                            format!(
                                "signals '{}' and '{}' in message '{}' both map to C identifier '{}'",
                                first_name, s.name, m.name, ident
                            ),
                            s.line,
                            &m.name,
                            &s.name,
                        ));
                    }
                }
                None => idents.push((ident, s.name.clone())),
            }
        }
    }

    if check_nodes {
        let mut used: Vec<&str> = Vec::new();
        for m in db.messages.iter() {
            used.push(m.sender.as_str());
            for s in m.signals.iter() {
                for r in s.receivers.iter() {
                    used.push(r.as_str());
                }
            }
        }
        for n in db.nodes.iter() {
            if !used.contains(&n.name.as_str()) {
                out.push(diag(
                    "I001",
                    format!("node '{}' is declared but never transmits or receives", n.name),
                    n.line,
                    "",
                    "",
                ));
            }
        }
    }
    out
}

pub fn has_errors(diags: &[Diag]) -> bool {
    diags.iter().any(|d| d.severity == "error")
}

/// (errors, warnings, infos)
pub fn counts(diags: &[Diag]) -> (usize, usize, usize) {
    let mut c = (0, 0, 0);
    for d in diags.iter() {
        match d.severity {
            "error" => c.0 += 1,
            "warning" => c.1 += 1,
            _ => c.2 += 1,
        }
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    fn rules_of(src: &str) -> Vec<&'static str> {
        lint(&parse(src).unwrap()).iter().map(|d| d.rule).collect()
    }

    #[test]
    fn overlap_respects_multiplexing() {
        let alternatives = "BU_: A\nBO_ 1 M: 8 A\n SG_ P M : 0|8@1+ (1,0) [0|0] \"\" A\n SG_ X m0 : 8|8@1+ (1,0) [0|0] \"\" A\n SG_ Y m1 : 8|8@1+ (1,0) [0|0] \"\" A\n";
        assert!(rules_of(alternatives).is_empty());
        let same_page = "BU_: A\nBO_ 1 M: 8 A\n SG_ P M : 0|8@1+ (1,0) [0|0] \"\" A\n SG_ X m0 : 8|8@1+ (1,0) [0|0] \"\" A\n SG_ Y m0 : 12|8@1+ (1,0) [0|0] \"\" A\n";
        assert_eq!(rules_of(same_page), vec!["E001"]);
    }

    #[test]
    fn every_rule_has_a_unique_id() {
        for (i, a) in RULES.iter().enumerate() {
            for b in RULES[i + 1..].iter() {
                assert_ne!(a.id, b.id);
            }
        }
    }
}
