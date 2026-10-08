//! Embedded C generation.
//!
//! For each message: a struct of raw signal values, `pack`/`unpack`
//! functions that move one byte at a time with a single shift and mask, and
//! per-signal `decode`/`encode`/`is_in_range` helpers. The output is C99,
//! allocates nothing, keeps no global state, and compiles warning-free under
//! `-Wall -Wextra -Wpedantic -Wconversion` and friends.
//!
//! The text must match `tests/golden/powertrain.{h,c}`, which the reference
//! model produced and CI compiles and tests against bit-by-bit packing.

use crate::bits::{raw_range, segments};
use crate::lint::{counts, lint};
use crate::model::{Database, Message, Mux, Signal, ValueType, NO_NODE};
use crate::names::Names;
use crate::numfmt::{f64_at_least, f64_at_most, fmt_f64};
use crate::{PROJECT_URL, VERSION};

/// Text safe to place inside a C comment or a docstring.
pub fn comment_text(s: &str) -> String {
    let t = s
        .replace("\r\n", " ")
        .replace('\n', " ")
        .replace('\r', " ")
        .replace("*/", "* /");
    t.trim().to_string()
}

/// Greedy word wrap; `prefix` is prepended to every line and counts toward `width`.
pub(crate) fn wrap(text: &str, width: usize, prefix: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    let plen = prefix.chars().count();
    for w in text.split_whitespace() {
        let wlen = w.chars().count();
        let clen = cur.chars().count();
        if !cur.is_empty() && plen + clen + 1 + wlen > width {
            lines.push(format!("{}{}", prefix, cur));
            cur = w.to_string();
        } else if !cur.is_empty() {
            cur.push(' ');
            cur.push_str(w);
        } else {
            cur = w.to_string();
        }
    }
    if !cur.is_empty() {
        lines.push(format!("{}{}", prefix, cur));
    }
    lines
}

/// Refuse to generate code for a database with lint errors.
pub(crate) fn require_clean(db: &Database) -> Result<(), String> {
    let (errors, _, _) = counts(&lint(db));
    if errors > 0 {
        return Err(format!(
            "cannot generate code: the database has {} lint error(s); run 'canforge lint' to see them",
            errors
        ));
    }
    Ok(())
}

/// Signals outside the multiplexer switch, groups of multiplexed signals
/// sorted by multiplexer value, and the multiplexer itself (as indices).
pub(crate) struct Groups {
    pub plain: Vec<usize>,
    pub groups: Vec<(u64, Vec<usize>)>,
    pub mux: Option<usize>,
}

pub(crate) fn split_groups(m: &Message) -> Groups {
    let mut plain: Vec<usize> = Vec::new();
    let mut groups: Vec<(u64, Vec<usize>)> = Vec::new();
    for (i, s) in m.signals.iter().enumerate() {
        match s.mux {
            Mux::Value(v) => {
                let mut found = false;
                for g in groups.iter_mut() {
                    if g.0 == v {
                        g.1.push(i);
                        found = true;
                        break;
                    }
                }
                if !found {
                    groups.push((v, vec![i]));
                }
            }
            _ => plain.push(i),
        }
    }
    groups.sort_by(|a, b| a.0.cmp(&b.0));
    let mut mux: Option<usize> = None;
    for (i, s) in m.signals.iter().enumerate() {
        if s.mux == Mux::Switch {
            mux = Some(i);
        }
    }
    Groups { plain, groups, mux }
}

/// The C type that holds a signal's raw value.
pub fn c_type(s: &Signal) -> String {
    match s.value_type {
        ValueType::Float32 => return "uint32_t".to_string(),
        ValueType::Float64 => return "uint64_t".to_string(),
        ValueType::Integer => {}
    }
    let bits = type_bits(s);
    if s.signed {
        format!("int{}_t", bits)
    } else {
        format!("uint{}_t", bits)
    }
}

fn type_bits(s: &Signal) -> u32 {
    match s.value_type {
        ValueType::Float32 => 32,
        ValueType::Float64 => 64,
        ValueType::Integer => {
            if s.length <= 8 {
                8
            } else if s.length <= 16 {
                16
            } else if s.length <= 32 {
                32
            } else {
                64
            }
        }
    }
}

fn type_range(s: &Signal) -> (i128, i128) {
    let bits = type_bits(s);
    if c_type(s).starts_with('u') {
        (0, (1i128 << bits) - 1)
    } else {
        (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1)
    }
}

/// Raw bounds implied by the declared physical range, clamped to the signal.
fn checked_raw_bounds(s: &Signal) -> (i128, i128) {
    let qa = (s.minimum - s.offset) / s.factor;
    let qb = (s.maximum - s.offset) / s.factor;
    let (lo_q, hi_q) = if s.factor > 0.0 { (qa, qb) } else { (qb, qa) };
    let lo = (lo_q - 1e-9 * f64::max(1.0, lo_q.abs())).ceil();
    let hi = (hi_q + 1e-9 * f64::max(1.0, hi_q.abs())).floor();
    let (rmin, rmax) = raw_range(s.length, s.signed);
    let raw_lo = std::cmp::max(lo as i128, rmin);
    let raw_hi = std::cmp::min(hi as i128, rmax);
    (raw_lo, raw_hi)
}

/// `base * factor + offset`, leaving out factors of 1 and offsets of 0.
pub(crate) fn scale_expr(base: &str, s: &Signal) -> String {
    let mut expr = base.to_string();
    if s.factor != 1.0 {
        expr = format!("{} * {}", expr, fmt_f64(s.factor));
    }
    if s.offset > 0.0 {
        expr = format!("{} + {}", expr, fmt_f64(s.offset));
    } else if s.offset < 0.0 {
        expr = format!("{} - {}", expr, fmt_f64(-s.offset));
    }
    expr
}

/// `(value - offset) / factor`, leaving out factors of 1 and offsets of 0.
fn unscale_expr(s: &Signal) -> String {
    let num = if s.offset > 0.0 {
        format!("(value - {})", fmt_f64(s.offset))
    } else if s.offset < 0.0 {
        format!("(value + {})", fmt_f64(-s.offset))
    } else {
        "value".to_string()
    };
    if s.factor != 1.0 {
        format!("{} / {}", num, fmt_f64(s.factor))
    } else {
        num
    }
}

fn signal_doc(s: &Signal) -> String {
    let order = if s.little_endian { "1" } else { "0" };
    let sign = if s.signed { "-" } else { "+" };
    let mut parts: Vec<String> = Vec::new();
    parts.push(format!("{}: {}|{}@{}{}", s.name, s.start, s.length, order, sign));
    if s.value_type != ValueType::Integer {
        parts.push(s.value_type.as_str().to_string());
    }
    parts.push(format!("scale {}, offset {}", fmt_f64(s.factor), fmt_f64(s.offset)));
    if !s.has_unspecified_range() {
        parts.push(format!("range {} to {}", fmt_f64(s.minimum), fmt_f64(s.maximum)));
    }
    if !s.unit.is_empty() {
        parts.push(format!("unit {}", comment_text(&s.unit)));
    }
    let mut text = format!("{}.", parts.join(", "));
    if !s.comment.is_empty() {
        text.push(' ');
        text.push_str(&comment_text(&s.comment));
    }
    text
}

fn pack_lines(s: &Signal, field: &str, indent: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("{}/* {} */", indent, s.name));
    lines.push(format!("{}v = (uint64_t)src_p->{};", indent, field));
    for seg in segments(s).iter() {
        let mask: u32 = (1u32 << seg.count) - 1;
        let val = if seg.raw_bit_lo == 0 {
            "v".to_string()
        } else {
            format!("(v >> {})", seg.raw_bit_lo)
        };
        let mut term = format!("({} & 0x{:X}u)", val, mask);
        if seg.byte_bit_lo > 0 {
            term = format!("({} << {})", term, seg.byte_bit_lo);
        }
        lines.push(format!(
            "{}dst_p[{}] = (uint8_t)(dst_p[{}] | (uint8_t){});",
            indent, seg.byte, seg.byte, term
        ));
    }
    lines
}

fn unpack_lines(s: &Signal, field: &str, indent: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("{}/* {} */", indent, s.name));
    lines.push(format!("{}v = 0u;", indent));
    for seg in segments(s).iter() {
        let mut term = format!("(uint64_t)src_p[{}]", seg.byte);
        if seg.byte_bit_lo > 0 {
            term = format!("({} >> {})", term, seg.byte_bit_lo);
        }
        if seg.count < 8 {
            term = format!("({} & 0x{:X}u)", term, (1u32 << seg.count) - 1);
        }
        if seg.raw_bit_lo > 0 {
            term = format!("({} << {})", term, seg.raw_bit_lo);
        }
        lines.push(format!("{}v |= {};", indent, term));
    }
    let t = c_type(s);
    if s.value_type == ValueType::Integer && s.signed {
        if s.length < 64 {
            let sign: u64 = 1u64 << (s.length - 1);
            let ext: u64 = !((1u64 << s.length) - 1);
            lines.push(format!("{}if ((v & 0x{:X}ull) != 0ull) {{", indent, sign));
            lines.push(format!("{}    v |= 0x{:X}ull;", indent, ext));
            lines.push(format!("{}}}", indent));
        }
        lines.push(format!("{}dst_p->{} = ({})(int64_t)v;", indent, field, t));
    } else {
        lines.push(format!("{}dst_p->{} = ({})v;", indent, field, t));
    }
    lines
}

fn signal_functions(s: &Signal, func: &str) -> Vec<String> {
    let t = c_type(s);
    let mut out: Vec<String> = Vec::new();

    out.push(String::new());
    out.push(format!("double {}_decode({} raw)", func, t));
    out.push("{".to_string());
    match s.value_type {
        ValueType::Float32 => {
            out.push("    float f;".to_string());
            out.push(String::new());
            out.push("    (void)memcpy(&f, &raw, sizeof(f));".to_string());
            out.push(format!("    return {};", scale_expr("(double)f", s)));
        }
        ValueType::Float64 => {
            out.push("    double f;".to_string());
            out.push(String::new());
            out.push("    (void)memcpy(&f, &raw, sizeof(f));".to_string());
            out.push(format!("    return {};", scale_expr("f", s)));
        }
        ValueType::Integer => {
            out.push(format!("    return {};", scale_expr("(double)raw", s)));
        }
    }
    out.push("}".to_string());

    out.push(String::new());
    out.push(format!("{} {}_encode(double value)", t, func));
    out.push("{".to_string());
    match s.value_type {
        ValueType::Float32 => {
            out.push(format!("    double r = {};", unscale_expr(s)));
            out.push("    float f;".to_string());
            out.push("    uint32_t raw;".to_string());
            out.push(String::new());
            out.push("    if (r > (double)FLT_MAX) {".to_string());
            out.push("        r = (double)FLT_MAX;".to_string());
            out.push("    }".to_string());
            out.push("    if (r < -(double)FLT_MAX) {".to_string());
            out.push("        r = -(double)FLT_MAX;".to_string());
            out.push("    }".to_string());
            out.push("    f = (float)r;".to_string());
            out.push("    (void)memcpy(&raw, &f, sizeof(raw));".to_string());
            out.push("    return raw;".to_string());
        }
        ValueType::Float64 => {
            out.push(format!("    double f = {};", unscale_expr(s)));
            out.push("    uint64_t raw;".to_string());
            out.push(String::new());
            out.push("    (void)memcpy(&raw, &f, sizeof(raw));".to_string());
            out.push("    return raw;".to_string());
        }
        ValueType::Integer => {
            let (rmin, rmax) = raw_range(s.length, s.signed);
            let lo_f = fmt_f64(f64_at_least(rmin));
            let hi_f = fmt_f64(f64_at_most(rmax));
            out.push(format!("    double r = {};", unscale_expr(s)));
            out.push(String::new());
            out.push(format!("    if (!(r >= {})) {{", lo_f));
            out.push(format!("        r = {};", lo_f));
            out.push("    }".to_string());
            out.push(format!("    if (r > {}) {{", hi_f));
            out.push(format!("        r = {};", hi_f));
            out.push("    }".to_string());
            if s.signed {
                out.push(format!("    return ({})((r >= 0.0) ? (r + 0.5) : (r - 0.5));", t));
            } else {
                out.push(format!("    return ({})(r + 0.5);", t));
            }
        }
    }
    out.push("}".to_string());

    out.push(String::new());
    out.push(format!("bool {}_is_in_range({} raw)", func, t));
    out.push("{".to_string());
    if s.has_unspecified_range() {
        out.push("    (void)raw;".to_string());
        out.push("    return true;".to_string());
    } else if s.value_type != ValueType::Integer {
        out.push(format!("    double value = {}_decode(raw);", func));
        out.push(String::new());
        out.push(format!(
            "    return (value >= {}) && (value <= {});",
            fmt_f64(s.minimum),
            fmt_f64(s.maximum)
        ));
    } else {
        let (lo, hi) = checked_raw_bounds(s);
        let (tmin, tmax) = type_range(s);
        let mut conds: Vec<String> = Vec::new();
        if s.signed {
            if lo > tmin {
                conds.push(format!("(int64_t)raw >= {}LL", lo));
            }
            if hi < tmax {
                conds.push(format!("(int64_t)raw <= {}LL", hi));
            }
        } else {
            if lo > tmin {
                conds.push(format!("(uint64_t)raw >= {}ull", lo));
            }
            if hi < tmax {
                conds.push(format!("(uint64_t)raw <= {}ull", hi));
            }
        }
        if lo > hi {
            out.push("    (void)raw;".to_string());
            out.push("    return false;".to_string());
        } else if conds.is_empty() {
            out.push("    (void)raw;".to_string());
            out.push("    return true;".to_string());
        } else if conds.len() == 1 {
            out.push(format!("    return ({});", conds[0]));
        } else {
            out.push(format!("    return ({}) && ({});", conds[0], conds[1]));
        }
    }
    out.push("}".to_string());
    out
}

/// Generate `(header, source)` for `prefix.h` and `prefix.c`.
pub fn generate(db: &Database, prefix: &str, source_name: &str) -> Result<(String, String), String> {
    require_clean(db)?;
    let names = Names::new(db);
    let up = prefix.to_uppercase();
    let guard = format!("{}_H", up);
    let mut any_float32 = false;
    for m in db.messages.iter() {
        for s in m.signals.iter() {
            if s.value_type == ValueType::Float32 {
                any_float32 = true;
            }
        }
    }

    let mut h: Vec<String> = Vec::new();
    h.push("/**".to_string());
    h.push(format!(" * {}.h", prefix));
    h.push(" *".to_string());
    h.push(format!(
        " * Generated by canforge {} from {}. Do not edit.",
        VERSION,
        comment_text(source_name)
    ));
    h.push(format!(" * {}", PROJECT_URL));
    h.push(" *".to_string());
    h.push(" * Each message has a struct of raw signal values and pack/unpack functions".to_string());
    h.push(" * that convert it to and from the bytes on the wire. Each signal has".to_string());
    h.push(" * decode/encode helpers for physical values and a range check.".to_string());
    h.push(" * No heap allocation, no global state, C99.".to_string());
    h.push(" */".to_string());
    h.push(String::new());
    h.push(format!("#ifndef {}", guard));
    h.push(format!("#define {}", guard));
    h.push(String::new());
    h.push("#include <stdbool.h>".to_string());
    h.push("#include <stddef.h>".to_string());
    h.push("#include <stdint.h>".to_string());
    h.push(String::new());
    h.push("#ifdef __cplusplus".to_string());
    h.push("extern \"C\" {".to_string());
    h.push("#endif".to_string());
    h.push(String::new());
    h.push("/* Frame IDs, lengths and formats. */".to_string());
    for (mi, m) in db.messages.iter().enumerate() {
        let mu = names.msg[mi].to_uppercase();
        h.push(format!("#define {}_{}_FRAME_ID (0x{:X}u)", up, mu, m.frame_id));
        h.push(format!("#define {}_{}_LENGTH ({}u)", up, mu, m.dlc));
        h.push(format!(
            "#define {}_{}_IS_EXTENDED ({})",
            up,
            mu,
            if m.is_extended { 1 } else { 0 }
        ));
    }
    let mut choice_lines: Vec<String> = Vec::new();
    for (mi, m) in db.messages.iter().enumerate() {
        let mu = names.msg[mi].to_uppercase();
        for (si, s) in m.signals.iter().enumerate() {
            if s.value_type != ValueType::Integer {
                continue;
            }
            let su = names.sig[mi][si].to_uppercase();
            for (ci, (value, _)) in s.choices.iter().enumerate() {
                let label = &names.choice[mi][si][ci];
                let lit = if *value < 0 {
                    format!("({})", value)
                } else {
                    format!("({}u)", value)
                };
                choice_lines.push(format!("#define {}_{}_{}_{}_CHOICE {}", up, mu, su, label, lit));
            }
        }
    }
    if !choice_lines.is_empty() {
        h.push(String::new());
        h.push("/* Value descriptions. */".to_string());
        h.extend(choice_lines);
    }

    for (mi, m) in db.messages.iter().enumerate() {
        let mname = &names.msg[mi];
        let typ = format!("{}_{}_t", prefix, mname);
        h.push(String::new());
        h.push("/**".to_string());
        h.push(format!(" * {}", comment_text(&m.name)));
        h.push(" *".to_string());
        let fmt = if m.is_extended { "extended" } else { "standard" };
        let sender = if m.sender == NO_NODE {
            "no transmitter".to_string()
        } else {
            format!("sent by {}", m.sender)
        };
        h.push(format!(" * Frame 0x{:X} ({}), {} bytes, {}.", m.frame_id, fmt, m.dlc, sender));
        if !m.comment.is_empty() {
            h.extend(wrap(&comment_text(&m.comment), 78, " * "));
        }
        h.push(" */".to_string());
        h.push("typedef struct {".to_string());
        if m.signals.is_empty() {
            h.push("    uint8_t unused; /* This message has no signals. */".to_string());
        }
        for (si, s) in m.signals.iter().enumerate() {
            let doc = wrap(&signal_doc(s), 74, "");
            if doc.len() == 1 {
                h.push(format!("    /* {} */", doc[0]));
            } else {
                h.push("    /*".to_string());
                for d in doc.iter() {
                    h.push(format!("     * {}", d));
                }
                h.push("     */".to_string());
            }
            h.push(format!("    {} {};", c_type(s), names.sig[mi][si]));
        }
        h.push(format!("}} {};", typ));
        h.push(String::new());
        h.push(format!("/** Set every signal of {} to raw zero. */", comment_text(&m.name)));
        h.push(format!("void {}_{}_init({} *msg_p);", prefix, mname, typ));
        h.push(String::new());
        h.push("/**".to_string());
        h.push(format!(
            " * Pack {} into dst_p, which must hold at least {}_{}_LENGTH bytes.",
            comment_text(&m.name),
            up,
            mname.to_uppercase()
        ));
        h.push(" * Returns the number of bytes written, or -1 if size is too small.".to_string());
        h.push(" */".to_string());
        h.push(format!(
            "int {}_{}_pack(uint8_t *dst_p, const {} *src_p, size_t size);",
            prefix, mname, typ
        ));
        h.push(String::new());
        h.push("/**".to_string());
        h.push(format!(
            " * Unpack {} from src_p. Returns 0 on success, or -1 if size is",
            comment_text(&m.name)
        ));
        h.push(" * too small. Multiplexed signals outside the active group are zero.".to_string());
        h.push(" */".to_string());
        h.push(format!(
            "int {}_{}_unpack({} *dst_p, const uint8_t *src_p, size_t size);",
            prefix, mname, typ
        ));
        for (si, s) in m.signals.iter().enumerate() {
            let func = format!("{}_{}_{}", prefix, mname, names.sig[mi][si]);
            let t = c_type(s);
            h.push(String::new());
            h.push(format!("double {}_decode({} raw);", func, t));
            h.push(format!("{} {}_encode(double value);", t, func));
            h.push(format!("bool {}_is_in_range({} raw);", func, t));
        }
    }

    h.push(String::new());
    h.push("#ifdef __cplusplus".to_string());
    h.push("}".to_string());
    h.push("#endif".to_string());
    h.push(String::new());
    h.push(format!("#endif /* {} */", guard));
    h.push(String::new());

    let mut c: Vec<String> = Vec::new();
    c.push("/**".to_string());
    c.push(format!(" * {}.c", prefix));
    c.push(" *".to_string());
    c.push(format!(
        " * Generated by canforge {} from {}. Do not edit.",
        VERSION,
        comment_text(source_name)
    ));
    c.push(format!(" * {}", PROJECT_URL));
    c.push(" */".to_string());
    c.push(String::new());
    if any_float32 {
        c.push("#include <float.h>".to_string());
    }
    c.push("#include <string.h>".to_string());
    c.push(String::new());
    c.push(format!("#include \"{}.h\"", prefix));

    for (mi, m) in db.messages.iter().enumerate() {
        let mname = &names.msg[mi];
        let typ = format!("{}_{}_t", prefix, mname);
        let g = split_groups(m);
        let fields = &names.sig[mi];

        c.push(String::new());
        c.push(format!("void {}_{}_init({} *msg_p)", prefix, mname, typ));
        c.push("{".to_string());
        c.push("    (void)memset(msg_p, 0, sizeof(*msg_p));".to_string());
        c.push("}".to_string());

        // pack
        c.push(String::new());
        c.push(format!(
            "int {}_{}_pack(uint8_t *dst_p, const {} *src_p, size_t size)",
            prefix, mname, typ
        ));
        c.push("{".to_string());
        if !m.signals.is_empty() {
            c.push("    uint64_t v;".to_string());
            c.push(String::new());
        }
        if m.dlc > 0 {
            c.push(format!("    if (size < {}u) {{", m.dlc));
            c.push("        return -1;".to_string());
            c.push("    }".to_string());
            c.push(String::new());
            c.push(format!("    (void)memset(dst_p, 0, {}u);", m.dlc));
        } else {
            c.push("    (void)dst_p;".to_string());
            c.push("    (void)size;".to_string());
        }
        if m.signals.is_empty() {
            c.push("    (void)src_p;".to_string());
        }
        for &si in g.plain.iter() {
            c.push(String::new());
            c.extend(pack_lines(&m.signals[si], &fields[si], "    "));
        }
        if !g.groups.is_empty() {
            if let Some(mux_index) = g.mux {
                c.push(String::new());
                c.push(format!("    switch (src_p->{}) {{", fields[mux_index]));
                for (value, members) in g.groups.iter() {
                    c.push(format!("    case {}:", value));
                    for (k, &si) in members.iter().enumerate() {
                        if k > 0 {
                            c.push(String::new());
                        }
                        c.extend(pack_lines(&m.signals[si], &fields[si], "        "));
                    }
                    c.push("        break;".to_string());
                }
                c.push("    default:".to_string());
                c.push("        break;".to_string());
                c.push("    }".to_string());
            }
        }
        c.push(String::new());
        c.push(format!("    return {};", m.dlc));
        c.push("}".to_string());

        // unpack
        c.push(String::new());
        c.push(format!(
            "int {}_{}_unpack({} *dst_p, const uint8_t *src_p, size_t size)",
            prefix, mname, typ
        ));
        c.push("{".to_string());
        if !m.signals.is_empty() {
            c.push("    uint64_t v;".to_string());
            c.push(String::new());
        }
        if m.dlc > 0 {
            c.push(format!("    if (size < {}u) {{", m.dlc));
            c.push("        return -1;".to_string());
            c.push("    }".to_string());
            c.push(String::new());
        } else {
            c.push("    (void)size;".to_string());
        }
        if m.signals.is_empty() {
            c.push("    (void)src_p;".to_string());
        }
        c.push("    (void)memset(dst_p, 0, sizeof(*dst_p));".to_string());
        for &si in g.plain.iter() {
            c.push(String::new());
            c.extend(unpack_lines(&m.signals[si], &fields[si], "    "));
        }
        if !g.groups.is_empty() {
            if let Some(mux_index) = g.mux {
                c.push(String::new());
                c.push(format!("    switch (dst_p->{}) {{", fields[mux_index]));
                for (value, members) in g.groups.iter() {
                    c.push(format!("    case {}:", value));
                    for (k, &si) in members.iter().enumerate() {
                        if k > 0 {
                            c.push(String::new());
                        }
                        c.extend(unpack_lines(&m.signals[si], &fields[si], "        "));
                    }
                    c.push("        break;".to_string());
                }
                c.push("    default:".to_string());
                c.push("        break;".to_string());
                c.push("    }".to_string());
            }
        }
        c.push(String::new());
        c.push("    return 0;".to_string());
        c.push("}".to_string());

        for (si, s) in m.signals.iter().enumerate() {
            let func = format!("{}_{}_{}", prefix, mname, fields[si]);
            c.extend(signal_functions(s, &func));
        }
    }
    c.push(String::new());

    Ok((h.join("\n"), c.join("\n")))
}
