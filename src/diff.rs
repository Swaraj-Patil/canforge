//! Classifying the changes between two revisions of a DBC file.
//!
//! "breaking" changes make existing decoders misread frames (a signal moved,
//! was rescaled, or disappeared). "caution" changes keep the wire format but
//! affect consumers (renames, narrower ranges). "compatible" changes are safe.

use crate::model::{Database, Message, Mux, Signal};
use crate::numfmt::fmt_f64;

#[derive(Debug, Clone)]
pub struct Change {
    pub level: &'static str,
    pub kind: &'static str,
    pub message: String,
    pub message_name: String,
    pub signal_name: String,
}

fn change(level: &'static str, kind: &'static str, message: String, msg_name: &str, sig_name: &str) -> Change {
    Change {
        level,
        kind,
        message,
        message_name: msg_name.to_string(),
        signal_name: sig_name.to_string(),
    }
}

fn same_layout(a: &Signal, b: &Signal) -> bool {
    a.start == b.start
        && a.length == b.length
        && a.little_endian == b.little_endian
        && a.signed == b.signed
        && a.value_type == b.value_type
        && a.factor == b.factor
        && a.offset == b.offset
        && a.mux == b.mux
}

fn describe_order(s: &Signal) -> &'static str {
    if s.little_endian {
        "Intel"
    } else {
        "Motorola"
    }
}

fn signedness(signed: bool) -> &'static str {
    if signed {
        "signed"
    } else {
        "unsigned"
    }
}

fn mux_label(m: Mux) -> String {
    match m {
        Mux::None => "none".to_string(),
        Mux::Switch => "multiplexer".to_string(),
        Mux::Value(v) => format!("m{}", v),
    }
}

/// Value descriptions as an ordered map: first occurrence fixes the order,
/// later duplicates replace the label (like building a Python dict).
fn to_map(choices: &[(i64, String)]) -> Vec<(i64, String)> {
    let mut out: Vec<(i64, String)> = Vec::new();
    for (v, l) in choices.iter() {
        let mut found = false;
        for entry in out.iter_mut() {
            if entry.0 == *v {
                entry.1 = l.clone();
                found = true;
            }
        }
        if !found {
            out.push((*v, l.clone()));
        }
    }
    out
}

fn map_get(map: &[(i64, String)], key: i64) -> Option<&String> {
    for entry in map.iter() {
        if entry.0 == key {
            return Some(&entry.1);
        }
    }
    None
}

fn receivers_text(r: &[String]) -> String {
    if r.is_empty() {
        "none".to_string()
    } else {
        r.join(",")
    }
}

fn compare_signals(m_name: &str, a: &Signal, b: &Signal, out: &mut Vec<Change>) {
    let q = format!("{}.{}", m_name, a.name);
    let mut layout: Vec<String> = Vec::new();
    if a.start != b.start {
        layout.push(format!("start bit {} -> {}", a.start, b.start));
    }
    if a.length != b.length {
        layout.push(format!("length {} -> {}", a.length, b.length));
    }
    if a.little_endian != b.little_endian {
        layout.push(format!("byte order {} -> {}", describe_order(a), describe_order(b)));
    }
    if a.signed != b.signed {
        layout.push(format!("{} -> {}", signedness(a.signed), signedness(b.signed)));
    }
    if a.value_type != b.value_type {
        layout.push(format!("type {} -> {}", a.value_type.as_str(), b.value_type.as_str()));
    }
    if !layout.is_empty() {
        out.push(change(
            "breaking",
            "signal-layout-changed",
            format!("{} layout changed: {}", q, layout.join(", ")),
            m_name,
            &a.name,
        ));
    }
    let mut scaling: Vec<String> = Vec::new();
    if a.factor != b.factor {
        scaling.push(format!("factor {} -> {}", fmt_f64(a.factor), fmt_f64(b.factor)));
    }
    if a.offset != b.offset {
        scaling.push(format!("offset {} -> {}", fmt_f64(a.offset), fmt_f64(b.offset)));
    }
    if !scaling.is_empty() {
        out.push(change(
            "breaking",
            "signal-scaling-changed",
            format!("{} scaling changed: {}", q, scaling.join(", ")),
            m_name,
            &a.name,
        ));
    }
    if a.mux != b.mux {
        out.push(change(
            "breaking",
            "signal-multiplexing-changed",
            format!("{} multiplexing changed: {} -> {}", q, mux_label(a.mux), mux_label(b.mux)),
            m_name,
            &a.name,
        ));
    }
    if a.unit != b.unit {
        out.push(change(
            "caution",
            "signal-unit-changed",
            format!("{} unit changed: \"{}\" -> \"{}\"", q, a.unit, b.unit),
            m_name,
            &a.name,
        ));
    }
    if a.minimum != b.minimum || a.maximum != b.maximum {
        let narrowed = b.minimum > a.minimum || b.maximum < a.maximum;
        let range = format!(
            "[{}|{}] -> [{}|{}]",
            fmt_f64(a.minimum),
            fmt_f64(a.maximum),
            fmt_f64(b.minimum),
            fmt_f64(b.maximum)
        );
        if narrowed {
            out.push(change(
                "caution",
                "signal-range-narrowed",
                format!("{} range narrowed: {}", q, range),
                m_name,
                &a.name,
            ));
        } else {
            out.push(change(
                "compatible",
                "signal-range-widened",
                format!("{} range widened: {}", q, range),
                m_name,
                &a.name,
            ));
        }
    }
    if a.choices != b.choices {
        let old_map = to_map(&a.choices);
        let new_map = to_map(&b.choices);
        let mut lost: Vec<String> = Vec::new();
        for (v, l) in old_map.iter() {
            let kept = match map_get(&new_map, *v) {
                Some(nl) => nl == l,
                None => false,
            };
            if !kept {
                lost.push(v.to_string());
            }
        }
        if !lost.is_empty() {
            out.push(change(
                "caution",
                "signal-choices-changed",
                format!("{} value descriptions changed or removed for {}", q, lost.join(", ")),
                m_name,
                &a.name,
            ));
        } else {
            let mut added: Vec<String> = Vec::new();
            for (v, _) in new_map.iter() {
                if map_get(&old_map, *v).is_none() {
                    added.push(v.to_string());
                }
            }
            out.push(change(
                "compatible",
                "signal-choices-added",
                format!("{} value descriptions added for {}", q, added.join(", ")),
                m_name,
                &a.name,
            ));
        }
    }
    if a.receivers != b.receivers {
        out.push(change(
            "caution",
            "signal-receivers-changed",
            format!(
                "{} receivers changed: {} -> {}",
                q,
                receivers_text(&a.receivers),
                receivers_text(&b.receivers)
            ),
            m_name,
            &a.name,
        ));
    }
    if a.comment != b.comment {
        out.push(change(
            "compatible",
            "comment-changed",
            format!("{} comment changed", q),
            m_name,
            &a.name,
        ));
    }
}

fn compare_messages(a: &Message, b: &Message, out: &mut Vec<Change>) {
    let name = b.name.as_str();
    if b.dlc < a.dlc {
        out.push(change(
            "breaking",
            "frame-length-reduced",
            format!("{} length reduced from {} to {} bytes", name, a.dlc, b.dlc),
            name,
            "",
        ));
    } else if b.dlc > a.dlc {
        out.push(change(
            "caution",
            "frame-length-increased",
            format!("{} length increased from {} to {} bytes", name, a.dlc, b.dlc),
            name,
            "",
        ));
    }
    if a.sender != b.sender {
        out.push(change(
            "caution",
            "transmitter-changed",
            format!("{} transmitter changed: {} -> {}", name, a.sender, b.sender),
            name,
            "",
        ));
    }
    let old_names: Vec<&str> = a.signals.iter().map(|s| s.name.as_str()).collect();
    let new_names: Vec<&str> = b.signals.iter().map(|s| s.name.as_str()).collect();
    let removed: Vec<&Signal> = a.signals.iter().filter(|s| !new_names.contains(&s.name.as_str())).collect();
    let added: Vec<&Signal> = b.signals.iter().filter(|s| !old_names.contains(&s.name.as_str())).collect();
    let mut renamed_new: Vec<String> = Vec::new();

    for s in a.signals.iter() {
        if let Some(t) = b.signals.iter().find(|t| t.name == s.name) {
            compare_signals(name, s, t, out);
        }
    }
    for s in removed.iter() {
        let mut target: Option<&Signal> = None;
        for t in added.iter() {
            if !renamed_new.contains(&t.name) && same_layout(s, t) {
                target = Some(*t);
                break;
            }
        }
        match target {
            Some(t) => {
                renamed_new.push(t.name.clone());
                out.push(change(
                    "caution",
                    "signal-renamed",
                    format!("{}.{} renamed to {}", name, s.name, t.name),
                    name,
                    &s.name,
                ));
            }
            None => out.push(change(
                "breaking",
                "signal-removed",
                format!("{}.{} removed", name, s.name),
                name,
                &s.name,
            )),
        }
    }
    for t in added.iter() {
        if !renamed_new.contains(&t.name) {
            out.push(change(
                "compatible",
                "signal-added",
                format!(
                    "{}.{} added at {}|{}@{}{}",
                    name,
                    t.name,
                    t.start,
                    t.length,
                    if t.little_endian { "1" } else { "0" },
                    if t.signed { "-" } else { "+" }
                ),
                name,
                &t.name,
            ));
        }
    }
    if a.comment != b.comment {
        out.push(change(
            "compatible",
            "comment-changed",
            format!("{} comment changed", name),
            name,
            "",
        ));
    }
}

/// Every change from `old` to `new`, in a stable order.
pub fn diff(old: &Database, new: &Database) -> Vec<Change> {
    let mut out: Vec<Change> = Vec::new();
    let old_keys: Vec<(u64, bool)> = old.messages.iter().map(|m| (m.frame_id, m.is_extended)).collect();
    let mut matched: Vec<(u64, bool)> = Vec::new();
    for om in old.messages.iter() {
        let key = (om.frame_id, om.is_extended);
        match new.messages.iter().find(|m| (m.frame_id, m.is_extended) == key) {
            Some(nm) => {
                matched.push(key);
                if nm.name != om.name {
                    out.push(change(
                        "caution",
                        "message-renamed",
                        format!("{} renamed to {}", om.name, nm.name),
                        &nm.name,
                        "",
                    ));
                }
                compare_messages(om, nm, &mut out);
            }
            None => {
                let moved = new
                    .messages
                    .iter()
                    .find(|m| m.name == om.name && !old_keys.contains(&(m.frame_id, m.is_extended)));
                match moved {
                    Some(mv) => {
                        matched.push((mv.frame_id, mv.is_extended));
                        out.push(change(
                            "breaking",
                            "frame-id-changed",
                            format!("{} frame ID changed from {} to {}", om.name, om.id_hex(), mv.id_hex()),
                            &om.name,
                            "",
                        ));
                        compare_messages(om, mv, &mut out);
                    }
                    None => out.push(change(
                        "breaking",
                        "message-removed",
                        format!("{} ({}) removed", om.name, om.id_hex()),
                        &om.name,
                        "",
                    )),
                }
            }
        }
    }
    for nm in new.messages.iter() {
        if !matched.contains(&(nm.frame_id, nm.is_extended)) {
            out.push(change(
                "compatible",
                "message-added",
                format!("{} ({}) added", nm.name, nm.id_hex()),
                &nm.name,
                "",
            ));
        }
    }
    out
}

/// "breaking", "caution", "compatible", or "identical".
pub fn verdict(changes: &[Change]) -> &'static str {
    if changes.iter().any(|c| c.level == "breaking") {
        return "breaking";
    }
    if changes.iter().any(|c| c.level == "caution") {
        return "caution";
    }
    if !changes.is_empty() {
        return "compatible";
    }
    "identical"
}

/// (breaking, caution, compatible)
pub fn counts(changes: &[Change]) -> (usize, usize, usize) {
    let mut c = (0, 0, 0);
    for ch in changes.iter() {
        match ch.level {
            "breaking" => c.0 += 1,
            "caution" => c.1 += 1,
            _ => c.2 += 1,
        }
    }
    c
}
