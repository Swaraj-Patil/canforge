//! Identifiers for generated code.

use crate::model::Database;
use std::collections::HashSet;

/// C keywords and stdbool macros that cannot be used as field names.
pub const C_KEYWORDS: &[&str] = &[
    "auto", "break", "case", "char", "const", "continue", "default", "do", "double", "else",
    "enum", "extern", "float", "for", "goto", "if", "inline", "int", "long", "register",
    "restrict", "return", "short", "signed", "sizeof", "static", "struct", "switch", "typedef",
    "union", "unsigned", "void", "volatile", "while", "bool", "true", "false",
];

/// `EngineSpeed` -> `engine_speed`, `ABSStatus` -> `abs_status`.
pub fn snake(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let n = chars.len();
    let mut out = String::new();
    for i in 0..n {
        let c = chars[i];
        if c.is_ascii_uppercase() {
            if i > 0 {
                let prev = chars[i - 1];
                let next_lower = i + 1 < n && chars[i + 1].is_ascii_lowercase();
                if prev.is_ascii_lowercase()
                    || prev.is_ascii_digit()
                    || (prev.is_ascii_uppercase() && next_lower)
                {
                    out.push('_');
                }
            }
            out.push(c.to_ascii_lowercase());
        } else if c.is_ascii_alphanumeric() {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    let mut collapsed = String::new();
    let mut prev_underscore = false;
    for c in out.chars() {
        if c == '_' {
            if !prev_underscore {
                collapsed.push('_');
            }
            prev_underscore = true;
        } else {
            collapsed.push(c);
            prev_underscore = false;
        }
    }
    let trimmed = collapsed.trim_matches('_');
    if trimmed.is_empty() {
        return "x".to_string();
    }
    if trimmed.starts_with(|c: char| c.is_ascii_digit()) {
        return format!("x_{}", trimmed);
    }
    trimmed.to_string()
}

/// A snake_case name that is also a valid, non-reserved C identifier.
pub fn c_ident(name: &str) -> String {
    let mut s = snake(name);
    if C_KEYWORDS.contains(&s.as_str()) {
        s.push('_');
    }
    s
}

/// Make names unique by appending `_2`, `_3`, ... to repeats.
pub fn unique_idents(names: &[String]) -> Vec<String> {
    let mut used: HashSet<String> = HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for n in names.iter() {
        let mut cand = n.clone();
        let mut k: usize = 2;
        while used.contains(&cand) {
            cand = format!("{}_{}", n, k);
            k += 1;
        }
        used.insert(cand.clone());
        out.push(cand);
    }
    out
}

/// Stable, collision-free identifiers for every message, signal and value
/// description in a database, indexed the same way as the database.
pub struct Names {
    pub msg: Vec<String>,
    pub sig: Vec<Vec<String>>,
    pub choice: Vec<Vec<Vec<String>>>,
}

impl Names {
    pub fn new(db: &Database) -> Names {
        let msg_raw: Vec<String> = db.messages.iter().map(|m| c_ident(&m.name)).collect();
        let msg = unique_idents(&msg_raw);
        let mut sig: Vec<Vec<String>> = Vec::new();
        let mut choice: Vec<Vec<Vec<String>>> = Vec::new();
        for m in db.messages.iter() {
            let sig_raw: Vec<String> = m.signals.iter().map(|s| c_ident(&s.name)).collect();
            sig.push(unique_idents(&sig_raw));
            let mut per_signal: Vec<Vec<String>> = Vec::new();
            for s in m.signals.iter() {
                let labels: Vec<String> = s
                    .choices
                    .iter()
                    .map(|pair| snake(&pair.1).to_uppercase())
                    .collect();
                per_signal.push(unique_idents(&labels));
            }
            choice.push(per_signal);
        }
        Names { msg, sig, choice }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_case_matches_the_reference() {
        let cases: [(&str, &str); 12] = [
            ("EngineData", "engine_data"),
            ("ABSStatus", "abs_status"),
            ("RPM2Value", "rpm2_value"),
            ("Speed_Rear_Left", "speed_rear_left"),
            ("VehicleSpeed_kph", "vehicle_speed_kph"),
            ("DC link and faults", "dc_link_and_faults"),
            ("N/A", "n_a"),
            ("50%", "x_50"),
            ("", "x"),
            ("__", "x"),
            ("WheelSpeedFL", "wheel_speed_fl"),
            ("IgbtTemp", "igbt_temp"),
        ];
        for (name, want) in cases.iter() {
            assert_eq!(snake(name), *want, "snake({:?})", name);
        }
    }

    #[test]
    fn avoids_keywords_and_repeats() {
        assert_eq!(c_ident("Static"), "static_");
        assert_eq!(c_ident("Default"), "default_");
        let names: Vec<String> = ["a", "a", "a_2", "a"].iter().map(|s| s.to_string()).collect();
        assert_eq!(unique_idents(&names), vec!["a", "a_2", "a_2_2", "a_3"]);
    }
}
