//! The canforge command-line tool.

use canforge::{codegen_c, codegen_py, decode, diff, json, layout, lint, names, parse, Database, VERSION};
use std::path::Path;
use std::process::exit;

const USAGE: &str = "canforge: check, visualize, decode, diff and generate code from CAN databases (DBC files)

Usage:
  canforge lint <file.dbc> [--format text|json|sarif]
  canforge info <file.dbc>
  canforge layout <file.dbc> [--message NAME]
  canforge decode <file.dbc> <frame-id> <hex-bytes> [--format text|json]
  canforge gen <c|python> <file.dbc> [-o DIR] [--prefix NAME]
  canforge diff <old.dbc> <new.dbc> [--format text|json]

Exit status: lint exits 1 when it finds errors, diff exits 1 when it finds
breaking changes, and 2 means the command line was wrong.

Examples:
  canforge lint bus.dbc
  canforge layout bus.dbc --message VehicleStatus
  canforge decode bus.dbc 0x100 \"e8 03 03 5a 18 fc 00 05\"
  canforge gen c bus.dbc -o generated/
  canforge diff main.dbc feature.dbc
";

struct Args {
    positional: Vec<String>,
    options: Vec<(String, String)>,
}

impl Args {
    fn get(&self, names: &[&str]) -> Option<String> {
        for (k, v) in self.options.iter() {
            if names.contains(&k.as_str()) {
                return Some(v.clone());
            }
        }
        None
    }
}

fn parse_args(args: &[String], value_options: &[&str]) -> Result<Args, String> {
    let mut out = Args {
        positional: Vec::new(),
        options: Vec::new(),
    };
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if value_options.contains(&a.as_str()) {
            if i + 1 >= args.len() {
                return Err(format!("{} needs a value", a));
            }
            out.options.push((a.clone(), args[i + 1].clone()));
            i += 2;
        } else if a.starts_with('-') && a.len() > 1 {
            return Err(format!("unknown option '{}'", a));
        } else {
            out.positional.push(a.clone());
            i += 1;
        }
    }
    Ok(out)
}

fn usage_error(message: &str) -> i32 {
    eprintln!("canforge: {}\n", message);
    eprint!("{}", USAGE);
    2
}

fn load(path: &str) -> Result<Database, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {}: {}", path, e))?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    parse(&text).map_err(|e| format!("{}:{}: error: {}", path, e.line, e.message))
}

fn file_name(path: &str) -> String {
    match Path::new(path).file_name() {
        Some(n) => n.to_string_lossy().into_owned(),
        None => path.to_string(),
    }
}

fn file_stem(path: &str) -> String {
    match Path::new(path).file_stem() {
        Some(n) => n.to_string_lossy().into_owned(),
        None => "canbus".to_string(),
    }
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {}", word)
    } else {
        format!("{} {}s", n, word)
    }
}

fn cmd_lint(args: &[String]) -> i32 {
    let a = match parse_args(args, &["--format"]) {
        Ok(a) => a,
        Err(e) => return usage_error(&e),
    };
    if a.positional.len() != 1 {
        return usage_error("lint takes one DBC file");
    }
    let path = &a.positional[0];
    let format = a.get(&["--format"]).unwrap_or_else(|| "text".to_string());
    if format != "text" && format != "json" && format != "sarif" {
        return usage_error(&format!("unknown format '{}'", format));
    }
    let db = match load(path) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("{}", e);
            return 1;
        }
    };
    let diags = lint::lint(&db);
    match format.as_str() {
        "json" => println!("{}", json::lint_json(&diags)),
        "sarif" => println!("{}", json::sarif(&diags, path)),
        _ => {
            for d in diags.iter() {
                println!("{}:{}: {} {}: {}", path, d.line, d.severity, d.rule, d.message);
            }
            let (errors, warnings, infos) = lint::counts(&diags);
            if diags.is_empty() {
                println!("{}: no problems found", path);
            } else {
                println!(
                    "{}: {}, {}, {}",
                    path,
                    plural(errors, "error"),
                    plural(warnings, "warning"),
                    plural(infos, "note")
                );
            }
        }
    }
    if lint::has_errors(&diags) {
        1
    } else {
        0
    }
}

fn cmd_info(args: &[String]) -> i32 {
    let a = match parse_args(args, &[]) {
        Ok(a) => a,
        Err(e) => return usage_error(&e),
    };
    if a.positional.len() != 1 {
        return usage_error("info takes one DBC file");
    }
    let db = match load(&a.positional[0]) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("{}", e);
            return 1;
        }
    };
    if !db.version.is_empty() {
        println!("Version {}", db.version);
    }
    let node_names: Vec<&str> = db.nodes.iter().map(|n| n.name.as_str()).collect();
    println!("Nodes: {}", if node_names.is_empty() { "none".to_string() } else { node_names.join(", ") });
    println!(
        "{} with {}\n",
        plural(db.messages.len(), "message"),
        plural(db.signal_count(), "signal")
    );
    let width = db.messages.iter().map(|m| m.name.len()).max().unwrap_or(0);
    for m in db.messages.iter() {
        println!(
            "  {:<w$}  {:<10}  {:>2} bytes  {:>2} signals  {}",
            m.name,
            m.id_hex(),
            m.dlc,
            m.signals.len(),
            m.sender,
            w = width
        );
    }
    0
}

fn cmd_layout(args: &[String]) -> i32 {
    let a = match parse_args(args, &["--message", "-m"]) {
        Ok(a) => a,
        Err(e) => return usage_error(&e),
    };
    if a.positional.len() != 1 {
        return usage_error("layout takes one DBC file");
    }
    let db = match load(&a.positional[0]) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("{}", e);
            return 1;
        }
    };
    let only = a.get(&["--message", "-m"]);
    match layout::render(&db, only.as_deref()) {
        Ok(text) => {
            print!("{}", text);
            0
        }
        Err(e) => {
            eprintln!("canforge: {}", e);
            1
        }
    }
}

fn cmd_decode(args: &[String]) -> i32 {
    let a = match parse_args(args, &["--format"]) {
        Ok(a) => a,
        Err(e) => return usage_error(&e),
    };
    if a.positional.len() < 3 {
        return usage_error("decode takes a DBC file, a frame ID and the frame bytes");
    }
    let path = &a.positional[0];
    let hex = a.positional[2..].join(" ");
    let format = a.get(&["--format"]).unwrap_or_else(|| "text".to_string());
    if format == "json" {
        let src = match std::fs::read(path) {
            Ok(b) => String::from_utf8_lossy(&b).into_owned(),
            Err(e) => {
                eprintln!("cannot read {}: {}", path, e);
                return 1;
            }
        };
        let out = json::decode_json(&src, &a.positional[1], &hex);
        println!("{}", out);
        return if out.starts_with("{\"ok\":true") { 0 } else { 1 };
    }
    if format != "text" {
        return usage_error(&format!("unknown format '{}'", format));
    }
    let db = match load(path) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("{}", e);
            return 1;
        }
    };
    let id = match decode::parse_frame_id(&a.positional[1]) {
        Ok(id) => id,
        Err(e) => return usage_error(&e),
    };
    let data = match decode::parse_hex(&hex) {
        Ok(d) => d,
        Err(e) => return usage_error(&e),
    };
    match decode::decode(&db, id, &data, None) {
        Ok(d) => {
            let id_text = match db.find_frame(d.frame_id, Some(d.extended)) {
                Some(m) => m.id_hex(),
                None => format!("0x{:X}", d.frame_id),
            };
            println!("{} ({})", d.message, id_text);
            let width = d.signals.iter().map(|s| s.name.len()).max().unwrap_or(0);
            for s in d.signals.iter() {
                let value = canforge::numfmt::fmt_f64(s.physical);
                let unit = if s.unit.is_empty() { String::new() } else { format!(" {}", s.unit) };
                let label = match &s.label {
                    Some(l) => format!("  \"{}\"", l),
                    None => String::new(),
                };
                println!("  {:<w$}  {}{}  (raw {}){}", s.name, value, unit, s.raw, label, w = width);
            }
            0
        }
        Err(e) => {
            eprintln!("canforge: {}", e);
            1
        }
    }
}

fn cmd_gen(args: &[String]) -> i32 {
    let a = match parse_args(args, &["-o", "--out", "--prefix"]) {
        Ok(a) => a,
        Err(e) => return usage_error(&e),
    };
    if a.positional.len() != 2 {
        return usage_error("gen takes a language (c or python) and one DBC file");
    }
    let lang = a.positional[0].as_str();
    let path = &a.positional[1];
    let out_dir = a.get(&["-o", "--out"]).unwrap_or_else(|| ".".to_string());
    let prefix = match a.get(&["--prefix"]) {
        Some(p) => names::snake(&p),
        None => names::snake(&file_stem(path)),
    };
    let db = match load(path) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("{}", e);
            return 1;
        }
    };
    let source_name = file_name(path);
    let files: Vec<(String, String)> = match lang {
        "c" => match codegen_c::generate(&db, &prefix, &source_name) {
            Ok((h, c)) => vec![(format!("{}.h", prefix), h), (format!("{}.c", prefix), c)],
            Err(e) => {
                eprintln!("canforge: {}", e);
                return 1;
            }
        },
        "python" | "py" => match codegen_py::generate(&db, &prefix, &source_name) {
            Ok(py) => vec![(format!("{}.py", prefix), py)],
            Err(e) => {
                eprintln!("canforge: {}", e);
                return 1;
            }
        },
        other => return usage_error(&format!("unknown language '{}'; use c or python", other)),
    };
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        eprintln!("canforge: cannot create {}: {}", out_dir, e);
        return 1;
    }
    for (name, content) in files.iter() {
        let target = Path::new(&out_dir).join(name);
        if let Err(e) = std::fs::write(&target, content) {
            eprintln!("canforge: cannot write {}: {}", target.display(), e);
            return 1;
        }
        println!("wrote {}", target.display());
    }
    0
}

fn cmd_diff(args: &[String]) -> i32 {
    let a = match parse_args(args, &["--format"]) {
        Ok(a) => a,
        Err(e) => return usage_error(&e),
    };
    if a.positional.len() != 2 {
        return usage_error("diff takes two DBC files");
    }
    let format = a.get(&["--format"]).unwrap_or_else(|| "text".to_string());
    if format != "text" && format != "json" {
        return usage_error(&format!("unknown format '{}'", format));
    }
    let old = match load(&a.positional[0]) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("{}", e);
            return 1;
        }
    };
    let new = match load(&a.positional[1]) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("{}", e);
            return 1;
        }
    };
    let changes = diff::diff(&old, &new);
    let verdict = diff::verdict(&changes);
    if format == "json" {
        println!("{}", json::changes_json(&old, &new));
    } else {
        let (breaking, caution, compatible) = diff::counts(&changes);
        println!("{} -> {}", a.positional[0], a.positional[1]);
        if changes.is_empty() {
            println!("No changes.");
        } else {
            println!(
                "Verdict: {} ({} breaking, {} caution, {} compatible)",
                verdict, breaking, caution, compatible
            );
            for level in ["breaking", "caution", "compatible"] {
                let group: Vec<&diff::Change> = changes.iter().filter(|c| c.level == level).collect();
                if group.is_empty() {
                    continue;
                }
                println!("\n{}", level);
                for c in group.iter() {
                    println!("  {}", c.message);
                }
            }
        }
    }
    if verdict == "breaking" {
        1
    } else {
        0
    }
}

fn run(args: &[String]) -> i32 {
    if args.is_empty() {
        eprint!("{}", USAGE);
        return 2;
    }
    let rest = &args[1..];
    match args[0].as_str() {
        "-h" | "--help" | "help" => {
            print!("{}", USAGE);
            0
        }
        "-V" | "--version" | "version" => {
            println!("canforge {}", VERSION);
            0
        }
        "lint" => cmd_lint(rest),
        "info" => cmd_info(rest),
        "layout" => cmd_layout(rest),
        "decode" => cmd_decode(rest),
        "gen" => cmd_gen(rest),
        "diff" => cmd_diff(rest),
        other => usage_error(&format!("unknown command '{}'", other)),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    exit(run(&args));
}
