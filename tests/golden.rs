//! The Rust implementation must reproduce the reference model exactly.
//!
//! Every file in tests/golden/ was produced by reference/canforge_ref.py
//! (regenerate with `python3 reference/make_golden.py`). The C golden files
//! are also compiled with strict warnings and tested against the reference
//! model's bit-by-bit packing in CI, so matching them byte for byte means the
//! Rust generator emits verified code.

use canforge::{codegen_c, codegen_py, decode, diff, lint, parse, Database};
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(rel: &str) -> String {
    let path = root().join(rel);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {}", path.display(), e));
    text.replace("\r\n", "\n")
}

fn load(rel: &str) -> Database {
    parse(&read(rel)).unwrap_or_else(|e| panic!("{}: {}", rel, e))
}

/// Line-by-line comparison that points at the first difference.
fn assert_same_text(what: &str, got: &str, want: &str) {
    if got == want {
        return;
    }
    let g: Vec<&str> = got.split('\n').collect();
    let w: Vec<&str> = want.split('\n').collect();
    let n = std::cmp::max(g.len(), w.len());
    for i in 0..n {
        let gl = g.get(i).copied().unwrap_or("<end of output>");
        let wl = w.get(i).copied().unwrap_or("<end of output>");
        if gl != wl {
            panic!(
                "{} differs from the reference model at line {}:\n  rust:      {:?}\n  reference: {:?}",
                what,
                i + 1,
                gl,
                wl
            );
        }
    }
    panic!("{} differs from the reference model", what);
}

fn clean(text: &str) -> String {
    text.replace('\t', " ").replace('\r', " ").replace('\n', " ")
}

fn data_lines(text: &str) -> Vec<String> {
    text.split('\n')
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.to_string())
        .collect()
}

#[test]
fn generated_c_matches_reference() {
    let db = load("examples/powertrain.dbc");
    let (header, source) = codegen_c::generate(&db, "powertrain", "powertrain.dbc").unwrap();
    assert_same_text("powertrain.h", &header, &read("tests/golden/powertrain.h"));
    assert_same_text("powertrain.c", &source, &read("tests/golden/powertrain.c"));
}

#[test]
fn generated_python_matches_reference() {
    let db = load("examples/powertrain.dbc");
    let py = codegen_py::generate(&db, "powertrain", "powertrain.dbc").unwrap();
    assert_same_text("powertrain.py", &py, &read("tests/golden/powertrain.py"));
}

fn hex_bytes(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

#[test]
fn decoding_matches_reference_vectors() {
    let db = load("examples/powertrain.dbc");
    let rows: Vec<Vec<String>> = data_lines(&read("tests/golden/decode_vectors.tsv"))
        .iter()
        .map(|l| l.split('\t').map(|f| f.to_string()).collect())
        .collect();
    let mut frames = 0;
    let mut values = 0;
    let mut i = 0;
    while i < rows.len() {
        let mut j = i;
        while j < rows.len() && rows[j][0] == rows[i][0] {
            j += 1;
        }
        let group = &rows[i..j];
        let first = &group[0];
        let frame_id = u64::from_str_radix(&first[1], 16).unwrap();
        let extended = first[2] == "1";
        let data = hex_bytes(&first[3]);
        let d = decode::decode(&db, frame_id, &data, Some(extended))
            .unwrap_or_else(|e| panic!("frame {} ({}): {}", first[0], first[3], e));
        let mux = match d.mux {
            Some(v) => v.to_string(),
            None => "-".to_string(),
        };
        assert_eq!(mux, first[4], "multiplexer value of frame {}", first[0]);
        let got_names: Vec<&str> = d.signals.iter().map(|s| s.name.as_str()).collect();
        let want_names: Vec<&str> = group.iter().map(|r| r[5].as_str()).collect();
        assert_eq!(got_names, want_names, "signals present in frame {} ({})", first[0], first[3]);
        for (s, row) in d.signals.iter().zip(group.iter()) {
            let context = format!("{} in frame {} ({})", s.name, row[0], row[3]);
            assert_eq!(s.raw.to_string(), row[6], "raw value of {}", context);
            assert_eq!(
                format!("{:016x}", s.physical.to_bits()),
                row[7],
                "physical value of {} (rust {:?})",
                context,
                s.physical
            );
            let label = s.label.clone().unwrap_or_else(|| "-".to_string());
            assert_eq!(clean(&label), row[8], "value description of {}", context);
            values += 1;
        }
        frames += 1;
        i = j;
    }
    assert_eq!(frames, 280);
    assert!(values > 1000);
}

#[test]
fn lint_matches_reference() {
    let mut files: Vec<String> = std::fs::read_dir(root().join("tests/fixtures/lint"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".dbc"))
        .collect();
    files.sort();
    let mut targets: Vec<(String, String)> = files
        .iter()
        .map(|f| (format!("lint/{}", f), format!("tests/fixtures/lint/{}", f)))
        .collect();
    targets.push(("examples/powertrain.dbc".to_string(), "examples/powertrain.dbc".to_string()));

    let mut got: Vec<String> = Vec::new();
    for (label, rel) in targets.iter() {
        for d in lint::lint(&load(rel)).iter() {
            got.push(format!("{}\t{}\t{}\t{}\t{}", label, d.rule, d.severity, d.line, clean(&d.message)));
        }
    }
    let want = data_lines(&read("tests/golden/lint.tsv"));
    assert_same_text("lint output", &got.join("\n"), &want.join("\n"));
}

#[test]
fn every_fixture_triggers_exactly_its_declared_rules() {
    let dir = root().join("tests/fixtures/lint");
    let mut covered: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        let first = text.lines().next().unwrap_or("");
        let declared = first.strip_prefix("// expect:").expect("fixture must start with // expect:").trim();
        let mut want: Vec<String> = if declared == "none" {
            Vec::new()
        } else {
            declared.split(',').map(|r| r.trim().to_string()).collect()
        };
        want.sort();
        let mut got: Vec<String> = lint::lint(&parse(&text).unwrap()).iter().map(|d| d.rule.to_string()).collect();
        got.sort();
        assert_eq!(got, want, "{}", path.display());
        covered.extend(want);
    }
    for rule in lint::RULES.iter() {
        assert!(covered.iter().any(|r| r == rule.id), "no fixture covers {}", rule.id);
    }
}

#[test]
fn diff_matches_reference() {
    let old = load("tests/fixtures/diff/v1.dbc");
    let new = load("tests/fixtures/diff/v2.dbc");
    let changes = diff::diff(&old, &new);
    let mut got: Vec<String> = vec![format!("# verdict\t{}", diff::verdict(&changes))];
    for c in changes.iter() {
        got.push(format!("{}\t{}\t{}", c.level, c.kind, clean(&c.message)));
    }
    let want: Vec<String> = read("tests/golden/diff.tsv")
        .split('\n')
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect();
    assert_same_text("diff output", &got.join("\n"), &want.join("\n"));
}

#[test]
fn identical_files_have_no_changes() {
    let a = load("examples/powertrain.dbc");
    let b = load("examples/powertrain.dbc");
    assert_eq!(diff::verdict(&diff::diff(&a, &b)), "identical");
}

#[test]
fn code_generation_refuses_databases_with_errors() {
    let db = load("tests/fixtures/lint/e001_signal_overlap.dbc");
    let err = codegen_c::generate(&db, "x", "x.dbc").unwrap_err();
    assert!(err.contains("lint error"), "{}", err);
    assert!(codegen_py::generate(&db, "x", "x.dbc").is_err());
}
