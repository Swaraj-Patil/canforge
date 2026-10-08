//! A small JSON writer, and the JSON documents the CLI and the browser use.

use crate::bits::{representable_range, signal_bits};
use crate::decode::{decode_message, no_message, parse_frame_id, parse_hex, Decoded};
use crate::diff::{counts as diff_counts, diff, verdict};
use crate::lint::{counts as lint_counts, lint, Diag, RULES};
use crate::model::{Database, FrameIndex, Mux};
use crate::names::snake;
use crate::numfmt::json_num;
use crate::parser::parse;
use crate::{codegen_c, codegen_py, PROJECT_URL, VERSION};

/// A JSON string literal.
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Builds a JSON object field by field, in insertion order.
pub struct Obj {
    parts: Vec<String>,
}

impl Default for Obj {
    fn default() -> Self {
        Obj::new()
    }
}

impl Obj {
    pub fn new() -> Obj {
        Obj { parts: Vec::new() }
    }

    /// Add a value that is already serialized JSON.
    pub fn raw(mut self, key: &str, value: String) -> Obj {
        self.parts.push(format!("{}:{}", esc(key), value));
        self
    }

    pub fn text(self, key: &str, value: &str) -> Obj {
        self.raw(key, esc(value))
    }

    pub fn number(self, key: &str, value: f64) -> Obj {
        self.raw(key, json_num(value))
    }

    pub fn int(self, key: &str, value: i128) -> Obj {
        self.raw(key, value.to_string())
    }

    pub fn flag(self, key: &str, value: bool) -> Obj {
        self.raw(key, if value { "true".to_string() } else { "false".to_string() })
    }

    pub fn null(self, key: &str) -> Obj {
        self.raw(key, "null".to_string())
    }

    pub fn build(self) -> String {
        format!("{{{}}}", self.parts.join(","))
    }
}

pub fn arr(items: Vec<String>) -> String {
    format!("[{}]", items.join(","))
}

/// `{"ok":false,"error":{"message":...,"line":...}}`
pub fn error_json(message: &str, line: Option<usize>) -> String {
    let mut e = Obj::new().text("message", message);
    e = match line {
        Some(l) => e.int("line", l as i128),
        None => e.null("line"),
    };
    Obj::new().flag("ok", false).raw("error", e.build()).build()
}

pub fn diag_json(d: &Diag) -> String {
    Obj::new()
        .text("rule", d.rule)
        .text("name", d.name)
        .text("severity", d.severity)
        .text("message", &d.message)
        .int("line", d.line as i128)
        .text("message_name", &d.message_name)
        .text("signal_name", &d.signal_name)
        .build()
}

/// Lint results for `canforge lint --format json`.
pub fn lint_json(diags: &[Diag]) -> String {
    let (errors, warnings, infos) = lint_counts(diags);
    let items: Vec<String> = diags.iter().map(diag_json).collect();
    Obj::new()
        .flag("ok", true)
        .int("errors", errors as i128)
        .int("warnings", warnings as i128)
        .int("infos", infos as i128)
        .raw("diagnostics", arr(items))
        .build()
}

/// SARIF 2.1.0, which GitHub code scanning turns into inline PR annotations.
pub fn sarif(diags: &[Diag], uri: &str) -> String {
    let rules: Vec<String> = RULES
        .iter()
        .map(|r| {
            Obj::new()
                .text("id", r.id)
                .text("name", r.name)
                .raw("shortDescription", Obj::new().text("text", r.summary).build())
                .raw(
                    "defaultConfiguration",
                    Obj::new().text("level", sarif_level(r.severity)).build(),
                )
                .build()
        })
        .collect();
    let results: Vec<String> = diags
        .iter()
        .map(|d| {
            let region = Obj::new().int("startLine", std::cmp::max(d.line, 1) as i128).build();
            let physical = Obj::new()
                .raw("artifactLocation", Obj::new().text("uri", uri).build())
                .raw("region", region)
                .build();
            let location = Obj::new().raw("physicalLocation", physical).build();
            Obj::new()
                .text("ruleId", d.rule)
                .text("level", sarif_level(d.severity))
                .raw("message", Obj::new().text("text", &d.message).build())
                .raw("locations", arr(vec![location]))
                .build()
        })
        .collect();
    let driver = Obj::new()
        .text("name", "canforge")
        .text("version", VERSION)
        .text("informationUri", PROJECT_URL)
        .raw("rules", arr(rules))
        .build();
    let run = Obj::new()
        .raw("tool", Obj::new().raw("driver", driver).build())
        .raw("results", arr(results))
        .build();
    Obj::new()
        .text("$schema", "https://json.schemastore.org/sarif-2.1.0.json")
        .text("version", "2.1.0")
        .raw("runs", arr(vec![run]))
        .build()
}

fn sarif_level(severity: &str) -> &'static str {
    match severity {
        "error" => "error",
        "warning" => "warning",
        _ => "note",
    }
}

fn mux_json(m: Mux) -> String {
    match m {
        Mux::None => "null".to_string(),
        Mux::Switch => esc("switch"),
        Mux::Value(v) => v.to_string(),
    }
}

/// Everything the browser needs to show a database: messages with their
/// signals and bit positions, nodes, and lint findings.
pub fn analysis(db: &Database) -> String {
    let diags = lint(db);
    let (errors, warnings, infos) = lint_counts(&diags);
    let mut messages: Vec<String> = Vec::new();
    for m in db.messages.iter() {
        let mut signals: Vec<String> = Vec::new();
        let mut multiplexer: Option<String> = None;
        for s in m.signals.iter() {
            if s.mux == Mux::Switch && multiplexer.is_none() {
                multiplexer = Some(s.name.clone());
            }
            let bits: Vec<String> = if s.valid_length() {
                signal_bits(s)
                    .iter()
                    .map(|&(byte, bit)| format!("[{},{}]", byte, bit))
                    .collect()
            } else {
                Vec::new()
            };
            let choices: Vec<String> = s
                .choices
                .iter()
                .map(|(v, l)| Obj::new().text("value", &v.to_string()).text("label", l).build())
                .collect();
            let receivers: Vec<String> = s.receivers.iter().map(|r| esc(r)).collect();
            let o = Obj::new()
                .text("name", &s.name)
                .int("start", s.start as i128)
                .int("length", s.length as i128)
                .text("byte_order", if s.little_endian { "intel" } else { "motorola" })
                .flag("signed", s.signed)
                .text("value_type", s.value_type.as_str())
                .number("factor", s.factor)
                .number("offset", s.offset)
                .number("minimum", s.minimum)
                .number("maximum", s.maximum);
            // What the bits can carry. Raw bounds are strings, since 64-bit
            // values do not fit a JSON number exactly.
            let o = match representable_range(s) {
                Some((rmin, rmax, pmin, pmax)) => o
                    .text("raw_min", &rmin.to_string())
                    .text("raw_max", &rmax.to_string())
                    .number("physical_min", pmin)
                    .number("physical_max", pmax),
                None => o.null("raw_min").null("raw_max").null("physical_min").null("physical_max"),
            };
            signals.push(
                o.text("unit", &s.unit)
                    .raw("receivers", arr(receivers))
                    .raw("mux", mux_json(s.mux))
                    .text("comment", &s.comment)
                    .raw("choices", arr(choices))
                    .int("line", s.line as i128)
                    .flag("valid", s.valid_length())
                    .raw("bits", arr(bits))
                    .build(),
            );
        }
        let mut obj = Obj::new()
            .text("name", &m.name)
            .int("id", m.frame_id as i128)
            .text("id_hex", &m.id_hex())
            .flag("extended", m.is_extended)
            .int("dlc", m.dlc as i128)
            .text("sender", &m.sender)
            .text("comment", &m.comment)
            .int("line", m.line as i128);
        obj = match multiplexer {
            Some(name) => obj.text("multiplexer", &name),
            None => obj.null("multiplexer"),
        };
        messages.push(obj.raw("signals", arr(signals)).build());
    }
    let nodes: Vec<String> = db
        .nodes
        .iter()
        .map(|n| Obj::new().text("name", &n.name).text("comment", &n.comment).build())
        .collect();
    let summary = Obj::new()
        .int("messages", db.messages.len() as i128)
        .int("signals", db.signal_count() as i128)
        .int("nodes", db.nodes.len() as i128)
        .int("errors", errors as i128)
        .int("warnings", warnings as i128)
        .int("infos", infos as i128)
        .text("version", &db.version)
        .text("comment", &db.comment)
        .build();
    let diag_items: Vec<String> = diags.iter().map(diag_json).collect();
    Obj::new()
        .flag("ok", true)
        .text("canforge", VERSION)
        .raw("summary", summary)
        .raw("nodes", arr(nodes))
        .raw("messages", arr(messages))
        .raw("diagnostics", arr(diag_items))
        .build()
}

/// Parse and analyse DBC source text.
pub fn analyze_json(src: &str) -> String {
    match parse(src) {
        Ok(db) => analysis(&db),
        Err(e) => error_json(&e.message, Some(e.line)),
    }
}

/// Parse DBC source, then decode a frame given as text: a frame ID and hex bytes.
pub fn decode_json(src: &str, frame_id: &str, hex: &str) -> String {
    match parse(src) {
        Ok(db) => decoded_json(&db, &FrameIndex::new(&db), frame_id, hex),
        Err(e) => error_json(&e.message, Some(e.line)),
    }
}

/// Decode a frame given as text, a frame ID and hex bytes, finding its
/// message through `frames`, the index of `db`. The lookup ignores whether
/// the frame is standard or extended and takes the first message with that
/// ID in the file, as `canforge decode` does.
pub fn decoded_json(db: &Database, frames: &FrameIndex, frame_id: &str, hex: &str) -> String {
    let id = match parse_frame_id(frame_id) {
        Ok(id) => id,
        Err(e) => return error_json(&e, None),
    };
    let data = match parse_hex(hex) {
        Ok(d) => d,
        Err(e) => return error_json(&e, None),
    };
    let m = match frames.find(id, None).and_then(|i| db.messages.get(i)) {
        Some(m) => m,
        None => return error_json(&no_message(id), None),
    };
    match decode_message(m, &data) {
        Ok(d) => decoded_frame_json(&d),
        Err(e) => error_json(&e, None),
    }
}

fn decoded_frame_json(d: &Decoded) -> String {
    let signals: Vec<String> = d
        .signals
        .iter()
        .map(|s| {
            let mut o = Obj::new()
                .text("name", &s.name)
                .text("raw", &s.raw.to_string())
                .number("physical", s.physical)
                .text("unit", &s.unit);
            o = match &s.label {
                Some(l) => o.text("label", l),
                None => o.null("label"),
            };
            o.build()
        })
        .collect();
    let mut o = Obj::new()
        .flag("ok", true)
        .text("message", &d.message)
        .int("frame_id", d.frame_id as i128)
        .flag("extended", d.extended);
    o = match d.mux {
        Some(v) => o.text("mux", &v.to_string()),
        None => o.null("mux"),
    };
    o.raw("signals", arr(signals)).build()
}

/// Parse DBC source, then generate files as `generated_json` does.
pub fn generate_json(src: &str, lang: &str, prefix: &str, source_name: &str) -> String {
    match parse(src) {
        Ok(db) => generated_json(&db, lang, prefix, source_name),
        Err(e) => error_json(&e.message, Some(e.line)),
    }
}

/// Generated files for `lang` ("c" or "python") as `{"files":[{name, content}]}`.
/// An empty prefix is derived from `source_name`.
pub fn generated_json(db: &Database, lang: &str, prefix: &str, source_name: &str) -> String {
    let prefix = if prefix.trim().is_empty() {
        snake(source_name.split('.').next().unwrap_or("canbus"))
    } else {
        snake(prefix.trim())
    };
    let file = |name: String, content: String| Obj::new().text("name", &name).text("content", &content).build();
    match lang {
        "c" => match codegen_c::generate(db, &prefix, source_name) {
            Ok((h, c)) => Obj::new()
                .flag("ok", true)
                .raw(
                    "files",
                    arr(vec![file(format!("{}.h", prefix), h), file(format!("{}.c", prefix), c)]),
                )
                .build(),
            Err(e) => error_json(&e, None),
        },
        "python" => match codegen_py::generate(db, &prefix, source_name) {
            Ok(py) => Obj::new()
                .flag("ok", true)
                .raw("files", arr(vec![file(format!("{}.py", prefix), py)]))
                .build(),
            Err(e) => error_json(&e, None),
        },
        other => error_json(&format!("unknown language '{}'; use c or python", other), None),
    }
}

/// The generated C for one signal, given its message and signal indices as
/// text, with each part as `codegen_c::signal_snippet` returns it.
pub fn signal_code_json(db: &Database, message: &str, signal: &str, prefix: &str) -> String {
    let index = |text: &str, what: &str| {
        text.trim()
            .parse::<usize>()
            .map_err(|_| format!("'{}' is not a {} index", text.trim(), what))
    };
    let (mi, si) = match (index(message, "message"), index(signal, "signal")) {
        (Ok(mi), Ok(si)) => (mi, si),
        (Err(e), _) | (_, Err(e)) => return error_json(&e, None),
    };
    let prefix = if prefix.trim().is_empty() {
        "canbus".to_string()
    } else {
        snake(prefix.trim())
    };
    match codegen_c::signal_snippet(db, mi, si, &prefix) {
        Ok(s) => Obj::new()
            .flag("ok", true)
            .text("header", &format!("{}.h", prefix))
            .text("source", &format!("{}.c", prefix))
            .text("field", &s.field)
            .text("pack_function", &s.pack_function)
            .text("pack", &s.pack)
            .text("unpack_function", &s.unpack_function)
            .text("unpack", &s.unpack)
            .text("functions", &s.functions)
            .build(),
        Err(e) => error_json(&e, None),
    }
}

/// Changes between two revisions.
pub fn diff_json(old_src: &str, new_src: &str) -> String {
    let old = match parse(old_src) {
        Ok(db) => db,
        Err(e) => return error_json(&format!("old file, {}", e.message), Some(e.line)),
    };
    let new = match parse(new_src) {
        Ok(db) => db,
        Err(e) => return error_json(&format!("new file, {}", e.message), Some(e.line)),
    };
    changes_json(&old, &new)
}

pub fn changes_json(old: &Database, new: &Database) -> String {
    let changes = diff(old, new);
    let (breaking, caution, compatible) = diff_counts(&changes);
    let items: Vec<String> = changes
        .iter()
        .map(|c| {
            Obj::new()
                .text("level", c.level)
                .text("kind", c.kind)
                .text("message", &c.message)
                .text("message_name", &c.message_name)
                .text("signal_name", &c.signal_name)
                .build()
        })
        .collect();
    let counts = Obj::new()
        .int("breaking", breaking as i128)
        .int("caution", caution as i128)
        .int("compatible", compatible as i128)
        .build();
    Obj::new()
        .flag("ok", true)
        .text("verdict", verdict(&changes))
        .raw("counts", counts)
        .raw("changes", arr(items))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_strings() {
        assert_eq!(esc("a\"b\\c\nd"), "\"a\\\"b\\\\c\\nd\"");
        assert_eq!(esc("\u{1}"), "\"\\u0001\"");
    }

    #[test]
    fn reports_parse_errors_as_json() {
        let j = analyze_json("BO_ 1 M 8 A\n");
        assert!(j.starts_with("{\"ok\":false"));
        assert!(j.contains("\"line\":1"));
    }

    #[test]
    fn decoding_through_the_frame_index_matches_scanning_the_messages() {
        let db = parse(include_str!("../examples/powertrain.dbc")).unwrap();
        let frames = FrameIndex::new(&db);
        let data = [0x5Au8; 64];
        for m in db.messages.iter() {
            let bytes = &data[..m.dlc as usize];
            let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
            let scanned = crate::decode::decode(&db, m.frame_id, bytes, None).unwrap();
            assert_eq!(
                decoded_json(&db, &frames, &m.id_hex(), &hex),
                decoded_frame_json(&scanned),
                "{}",
                m.name
            );
        }
    }

    #[test]
    fn decoding_reports_bad_input() {
        let db = parse(include_str!("../examples/powertrain.dbc")).unwrap();
        let frames = FrameIndex::new(&db);
        let cases = [
            ("0x7AB", "00", "no message with frame ID 0x7AB"),
            ("zz", "00", "'zz' is not a frame ID"),
            ("0x100", "xyz", "'xyz' is not hexadecimal"),
            ("0x100", "e8 03", "needs 8 bytes, got 2"),
        ];
        for (id, hex, fragment) in cases.iter() {
            let j = decoded_json(&db, &frames, id, hex);
            assert!(j.starts_with("{\"ok\":false") && j.contains(fragment), "{} {}: {}", id, hex, j);
        }
    }

    #[test]
    fn signal_code_takes_indices_as_text() {
        let db = parse(include_str!("../examples/powertrain.dbc")).unwrap();
        let j = signal_code_json(&db, "0", " 0 ", "Powertrain");
        assert!(j.starts_with("{\"ok\":true"), "{}", j);
        assert!(j.contains("\"header\":\"powertrain.h\""));
        assert!(j.contains("\"pack_function\":\"powertrain_vehicle_status_pack\""));
        assert!(signal_code_json(&db, "x", "0", "p").contains("'x' is not a message index"));
        assert!(signal_code_json(&db, "0", "-1", "p").contains("'-1' is not a signal index"));
        assert!(signal_code_json(&db, "9", "0", "p").contains("no message 9"));
    }

    #[test]
    fn analysis_reports_what_each_signal_can_carry() {
        let j = analyze_json(include_str!("../examples/powertrain.dbc"));
        // VehicleSpeed, a 16-bit unsigned signal scaled by 0.01.
        assert!(j.contains("\"raw_min\":\"0\",\"raw_max\":\"65535\",\"physical_min\":0.0,\"physical_max\":655.35"));
        // SerialNumber: the 64-bit bound stays exact as a string.
        assert!(j.contains("\"raw_max\":\"18446744073709551615\""));
    }

    #[test]
    fn generates_files_for_the_browser() {
        let src = include_str!("../examples/powertrain.dbc");
        let j = generate_json(src, "c", "", "powertrain.dbc");
        assert!(j.contains("\"name\":\"powertrain.h\""));
        assert!(j.contains("\"name\":\"powertrain.c\""));
        assert!(generate_json(src, "rust", "", "x.dbc").contains("unknown language"));
    }
}
