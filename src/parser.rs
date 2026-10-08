//! Parser for the parts of the DBC format that describe frames and signals.
//!
//! Statements canforge does not interpret (attributes, environment
//! variables, signal groups and so on) are skipped up to their semicolon.

use crate::lexer::{lex, Tok, TokKind};
use crate::model::{
    is_keyword, Database, DbcError, Message, Mux, Node, Signal, ValueType, INDEPENDENT_MSG,
};

/// Start bits beyond this are rejected. Real frames have at most 512 bits;
/// the cap guarantees bit arithmetic can never overflow.
pub const MAX_START_BIT: u64 = 65535;

struct Parser {
    t: Vec<Tok>,
    i: usize,
}

fn err(line: usize, message: String) -> DbcError {
    DbcError::new(line, message)
}

fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

impl Parser {
    fn has_more(&self) -> bool {
        self.i < self.t.len()
    }

    fn last_line(&self) -> usize {
        match self.t.last() {
            Some(t) => t.line,
            None => 1,
        }
    }

    fn peek_line(&self) -> usize {
        if self.has_more() {
            self.t[self.i].line
        } else {
            self.last_line()
        }
    }

    fn peek_is(&self, kind: TokKind, text: &str) -> bool {
        self.has_more() && self.t[self.i].kind == kind && self.t[self.i].text == text
    }

    fn peek_kind_is(&self, kind: TokKind) -> bool {
        self.has_more() && self.t[self.i].kind == kind
    }

    fn at_keyword(&self) -> bool {
        self.has_more() && self.t[self.i].kind == TokKind::Ident && is_keyword(&self.t[self.i].text)
    }

    fn at_plain_ident(&self) -> bool {
        self.has_more() && self.t[self.i].kind == TokKind::Ident && !is_keyword(&self.t[self.i].text)
    }

    fn next(&mut self, what: &str) -> Result<Tok, DbcError> {
        if self.has_more() {
            let tok = self.t[self.i].clone();
            self.i += 1;
            Ok(tok)
        } else {
            Err(err(
                self.last_line(),
                format!("unexpected end of file, expected {}", what),
            ))
        }
    }

    fn expect_punct(&mut self, ch: &str) -> Result<Tok, DbcError> {
        let what = format!("'{}'", ch);
        let tok = self.next(&what)?;
        if tok.kind != TokKind::Punct || tok.text != ch {
            return Err(err(
                tok.line,
                format!("expected '{}', found '{}'", ch, tok.text),
            ));
        }
        Ok(tok)
    }

    fn expect_ident(&mut self, what: &str) -> Result<Tok, DbcError> {
        let tok = self.next(what)?;
        if tok.kind != TokKind::Ident {
            return Err(err(tok.line, format!("expected {}, found '{}'", what, tok.text)));
        }
        Ok(tok)
    }

    fn expect_str(&mut self, what: &str) -> Result<Tok, DbcError> {
        let tok = self.next(what)?;
        if tok.kind != TokKind::Str {
            return Err(err(
                tok.line,
                format!("expected {} (a quoted string), found '{}'", what, tok.text),
            ));
        }
        Ok(tok)
    }

    fn parse_uint(&mut self, what: &str) -> Result<u64, DbcError> {
        let tok = self.next(what)?;
        if tok.kind != TokKind::Num || !is_digits(&tok.text) {
            return Err(err(
                tok.line,
                format!("expected {} (a non-negative integer), found '{}'", what, tok.text),
            ));
        }
        match tok.text.parse::<u64>() {
            Ok(v) => Ok(v),
            Err(_) => Err(err(tok.line, format!("{} {} is too large", what, tok.text))),
        }
    }

    fn take_sign(&mut self, what: &str) -> Result<bool, DbcError> {
        if self.peek_is(TokKind::Punct, "-") {
            self.next(what)?;
            return Ok(true);
        }
        if self.peek_is(TokKind::Punct, "+") {
            self.next(what)?;
        }
        Ok(false)
    }

    fn parse_int(&mut self, what: &str) -> Result<i64, DbcError> {
        let negative = self.take_sign(what)?;
        let tok = self.next(what)?;
        if tok.kind != TokKind::Num || !is_digits(&tok.text) {
            return Err(err(
                tok.line,
                format!("expected {} (an integer), found '{}'", what, tok.text),
            ));
        }
        let value = match tok.text.parse::<i64>() {
            Ok(v) => v,
            Err(_) => return Err(err(tok.line, format!("{} {} is too large", what, tok.text))),
        };
        Ok(if negative { -value } else { value })
    }

    fn parse_number(&mut self, what: &str) -> Result<f64, DbcError> {
        let negative = self.take_sign(what)?;
        let tok = self.next(what)?;
        if tok.kind != TokKind::Num {
            return Err(err(
                tok.line,
                format!("expected {} (a number), found '{}'", what, tok.text),
            ));
        }
        let value = match tok.text.parse::<f64>() {
            Ok(v) => v,
            Err(_) => return Err(err(tok.line, format!("invalid number '{}'", tok.text))),
        };
        Ok(if negative { -value } else { value })
    }

    fn skip_statement(&mut self) {
        while self.has_more() {
            let is_semicolon = self.t[self.i].kind == TokKind::Punct && self.t[self.i].text == ";";
            self.i += 1;
            if is_semicolon {
                return;
            }
        }
    }
}

fn find_message(db: &Database, raw_id: u64) -> Option<usize> {
    for (i, m) in db.messages.iter().enumerate() {
        if m.raw_id == raw_id {
            return Some(i);
        }
    }
    None
}

fn find_signal(msg: &Message, name: &str) -> Option<usize> {
    for (i, s) in msg.signals.iter().enumerate() {
        if s.name == name {
            return Some(i);
        }
    }
    None
}

fn parse_message(p: &mut Parser, line: usize) -> Result<Message, DbcError> {
    let raw_id = p.parse_uint("frame ID")?;
    if raw_id > 0xFFFF_FFFF {
        return Err(err(line, format!("frame ID {} does not fit in 32 bits", raw_id)));
    }
    let is_extended = (raw_id & 0x8000_0000) != 0;
    let frame_id = if is_extended { raw_id & 0x7FFF_FFFF } else { raw_id };
    let name = p.expect_ident("message name")?.text;
    p.expect_punct(":")?;
    let dlc = p.parse_uint("message length")?;
    let sender = p.expect_ident("transmitter")?.text;
    Ok(Message {
        raw_id,
        frame_id,
        is_extended,
        name,
        dlc,
        sender,
        signals: Vec::new(),
        comment: String::new(),
        line,
    })
}

fn parse_signal(p: &mut Parser, line: usize) -> Result<Signal, DbcError> {
    let name = p.expect_ident("signal name")?.text;
    let mut s = Signal::new(name, line);
    if p.peek_kind_is(TokKind::Ident) {
        let mtok = p.next("multiplexer indicator")?;
        let txt = mtok.text.clone();
        // Identifiers are ASCII, so byte slicing is safe here.
        let bytes = txt.as_bytes();
        let len = bytes.len();
        if txt == "M" {
            s.mux = Mux::Switch;
        } else if len > 1 && bytes[0] == b'm' && is_digits(&txt[1..]) {
            match txt[1..].parse::<u64>() {
                Ok(v) => s.mux = Mux::Value(v),
                Err(_) => {
                    return Err(err(
                        mtok.line,
                        format!("multiplexer value in '{}' is too large", txt),
                    ))
                }
            }
        } else if len > 2 && bytes[0] == b'm' && bytes[len - 1] == b'M' && is_digits(&txt[1..len - 1]) {
            return Err(err(
                mtok.line,
                format!("extended multiplexing ('{}') is not supported", txt),
            ));
        } else {
            return Err(err(
                mtok.line,
                format!("invalid multiplexer indicator '{}'", txt),
            ));
        }
    }
    p.expect_punct(":")?;
    s.start = p.parse_uint("start bit")?;
    if s.start > MAX_START_BIT {
        return Err(err(line, format!("start bit {} is out of range", s.start)));
    }
    p.expect_punct("|")?;
    s.length = p.parse_uint("signal length")?;
    p.expect_punct("@")?;
    let order_line = p.peek_line();
    let order = p.parse_uint("byte order")?;
    if order != 0 && order != 1 {
        return Err(err(
            order_line,
            format!("byte order must be 0 (Motorola) or 1 (Intel), found {}", order),
        ));
    }
    s.little_endian = order == 1;
    let sign = p.next("value type '+' or '-'")?;
    if sign.kind != TokKind::Punct || (sign.text != "+" && sign.text != "-") {
        return Err(err(
            sign.line,
            format!("expected value type '+' or '-', found '{}'", sign.text),
        ));
    }
    s.signed = sign.text == "-";
    p.expect_punct("(")?;
    s.factor = p.parse_number("factor")?;
    p.expect_punct(",")?;
    s.offset = p.parse_number("offset")?;
    p.expect_punct(")")?;
    p.expect_punct("[")?;
    s.minimum = p.parse_number("minimum")?;
    p.expect_punct("|")?;
    s.maximum = p.parse_number("maximum")?;
    p.expect_punct("]")?;
    s.unit = p.expect_str("unit")?.text;
    if p.at_plain_ident() {
        s.receivers.push(p.next("receiver")?.text);
        loop {
            if p.peek_is(TokKind::Punct, ",") {
                p.next("receiver")?;
                s.receivers.push(p.expect_ident("receiver")?.text);
            } else if p.at_plain_ident() {
                s.receivers.push(p.next("receiver")?.text);
            } else {
                break;
            }
        }
    }
    Ok(s)
}

fn parse_comment(p: &mut Parser, db: &mut Database) -> Result<(), DbcError> {
    if !p.has_more() {
        return Err(err(p.last_line(), "unexpected end of file in CM_".to_string()));
    }
    if p.peek_kind_is(TokKind::Str) {
        db.comment = p.next("comment")?.text;
    } else if p.peek_is(TokKind::Ident, "BU_") {
        p.next("BU_")?;
        let name = p.expect_ident("node name")?.text;
        let text = p.expect_str("comment")?.text;
        for node in db.nodes.iter_mut() {
            if node.name == name {
                node.comment = text.clone();
            }
        }
    } else if p.peek_is(TokKind::Ident, "BO_") {
        p.next("BO_")?;
        let raw_id = p.parse_uint("frame ID")?;
        let text = p.expect_str("comment")?.text;
        if let Some(mi) = find_message(db, raw_id) {
            db.messages[mi].comment = text;
        }
    } else if p.peek_is(TokKind::Ident, "SG_") {
        p.next("SG_")?;
        let raw_id = p.parse_uint("frame ID")?;
        let name = p.expect_ident("signal name")?.text;
        let text = p.expect_str("comment")?.text;
        if let Some(mi) = find_message(db, raw_id) {
            if let Some(si) = find_signal(&db.messages[mi], &name) {
                db.messages[mi].signals[si].comment = text;
            }
        }
    } else {
        p.skip_statement();
        return Ok(());
    }
    p.expect_punct(";")?;
    Ok(())
}

fn parse_val(p: &mut Parser, db: &mut Database) -> Result<(), DbcError> {
    if !p.peek_kind_is(TokKind::Num) {
        // Value tables for environment variables: not used by canforge.
        p.skip_statement();
        return Ok(());
    }
    let raw_id = p.parse_uint("frame ID")?;
    let name = p.expect_ident("signal name")?.text;
    let mut pairs: Vec<(i64, String)> = Vec::new();
    loop {
        if p.peek_is(TokKind::Punct, ";") {
            p.next("';'")?;
            break;
        }
        let value = p.parse_int("value")?;
        let label = p.expect_str("value description")?.text;
        pairs.push((value, label));
    }
    if let Some(mi) = find_message(db, raw_id) {
        if let Some(si) = find_signal(&db.messages[mi], &name) {
            db.messages[mi].signals[si].choices = pairs;
        }
    }
    Ok(())
}

fn parse_sig_valtype(p: &mut Parser, db: &mut Database) -> Result<(), DbcError> {
    let raw_id = p.parse_uint("frame ID")?;
    let name = p.expect_ident("signal name")?.text;
    if p.peek_is(TokKind::Punct, ":") {
        p.next("':'")?;
    }
    let vt_line = p.peek_line();
    let vt = p.parse_uint("value type")?;
    p.expect_punct(";")?;
    let kind = match vt {
        0 => ValueType::Integer,
        1 => ValueType::Float32,
        2 => ValueType::Float64,
        other => {
            return Err(err(
                vt_line,
                format!("SIG_VALTYPE_ must be 0, 1 or 2, found {}", other),
            ))
        }
    };
    if let Some(mi) = find_message(db, raw_id) {
        if let Some(si) = find_signal(&db.messages[mi], &name) {
            db.messages[mi].signals[si].value_type = kind;
        }
    }
    Ok(())
}

/// Parse DBC source text.
pub fn parse(src: &str) -> Result<Database, DbcError> {
    let src = match src.strip_prefix('\u{feff}') {
        Some(rest) => rest,
        None => src,
    };
    let toks = lex(src)?;
    let mut p = Parser { t: toks, i: 0 };
    let mut db = Database::default();
    while p.has_more() {
        let tok = p.next("a statement")?;
        if tok.kind != TokKind::Ident {
            return Err(err(
                tok.line,
                format!("unexpected '{}' at the start of a statement", tok.text),
            ));
        }
        let kw = tok.text.as_str();
        if kw == "VERSION" {
            db.version = p.expect_str("version string")?.text;
        } else if kw == "NS_" {
            p.expect_punct(":")?;
            while p.has_more() {
                let stop = p.peek_is(TokKind::Ident, "BS_")
                    || p.peek_is(TokKind::Ident, "BU_")
                    || p.peek_is(TokKind::Ident, "BO_");
                if stop {
                    break;
                }
                p.i += 1;
            }
        } else if kw == "BS_" {
            p.expect_punct(":")?;
            while p.has_more() && !p.at_keyword() {
                p.i += 1;
            }
        } else if kw == "BU_" {
            p.expect_punct(":")?;
            while p.at_plain_ident() {
                let t = p.t[p.i].clone();
                p.i += 1;
                db.nodes.push(Node {
                    name: t.text,
                    comment: String::new(),
                    line: t.line,
                });
            }
        } else if kw == "BO_" {
            let mut msg = parse_message(&mut p, tok.line)?;
            while p.peek_is(TokKind::Ident, "SG_") {
                let line = p.t[p.i].line;
                p.i += 1;
                let sig = parse_signal(&mut p, line)?;
                msg.signals.push(sig);
            }
            if msg.name != INDEPENDENT_MSG {
                db.messages.push(msg);
            }
        } else if kw == "SG_" {
            return Err(err(tok.line, "SG_ must follow a BO_ message definition".to_string()));
        } else if kw == "CM_" {
            parse_comment(&mut p, &mut db)?;
        } else if kw == "VAL_" {
            parse_val(&mut p, &mut db)?;
        } else if kw == "SIG_VALTYPE_" {
            parse_sig_valtype(&mut p, &mut db)?;
        } else if is_keyword(kw) {
            p.skip_statement();
        } else {
            return Err(err(tok.line, format!("unknown keyword '{}'", kw)));
        }
    }
    Ok(db)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error_of(src: &str) -> DbcError {
        match parse(src) {
            Ok(_) => panic!("expected a parse error for {:?}", src),
            Err(e) => e,
        }
    }

    #[test]
    fn reports_errors_with_line_numbers() {
        let cases: [(&str, usize, &str); 8] = [
            ("CM_ \"never closed;\n", 1, "unterminated string"),
            ("BO_ 1 M: 8 A\n SG_ X : 0|8@2+ (1,0) [0|1] \"\" A\n", 2, "byte order"),
            ("BO_ 1 M: 8 A\n SG_ X : 0|8@1* (1,0) [0|1] \"\" A\n", 2, "unexpected character"),
            ("BO_ 1 M: 8 A\n SG_ X q7 : 0|8@1+ (1,0) [0|1] \"\" A\n", 2, "multiplexer indicator"),
            ("SG_ X : 0|8@1+ (1,0) [0|1] \"\" A\n", 1, "must follow a BO_"),
            ("WHAT_ 1;\n", 1, "unknown keyword"),
            ("BO_ 1 M 8 A\n", 1, "expected ':'"),
            ("BO_ 1 M: 8 A\n SG_ X : 99999|8@1+ (1,0) [0|0] \"\" A\n", 2, "out of range"),
        ];
        for (src, line, fragment) in cases.iter() {
            let e = error_of(src);
            assert_eq!(e.line, *line, "{}", e.message);
            assert!(e.message.contains(*fragment), "{:?} should mention {:?}", e.message, fragment);
        }
    }

    #[test]
    fn skips_statements_it_does_not_use() {
        let db = parse("BA_DEF_ BO_ \"x;y\" INT 0 1;\nBO_ 5 M: 1 A\nBA_ \"x\" BO_ 5 1;\n").unwrap();
        assert_eq!(db.messages.len(), 1);
    }

    #[test]
    fn ignores_a_byte_order_mark() {
        let db = parse("\u{feff}VERSION \"x\"\n").unwrap();
        assert_eq!(db.version, "x");
    }

    #[test]
    fn reads_extended_ids_comments_and_value_tables() {
        let src = "BU_: A\nBO_ 2566869221 J: 8 A\n SG_ X : 0|8@1+ (1,0) [0|0] \"\" A\nCM_ SG_ 2566869221 X \"hello\nthere\";\nVAL_ 2566869221 X 0 \"Off\" 1 \"On\" ;\n";
        let db = parse(src).unwrap();
        let m = &db.messages[0];
        assert!(m.is_extended);
        assert_eq!(m.frame_id, 0x18FF_50E5);
        assert_eq!(m.signals[0].comment, "hello\nthere");
        assert_eq!(m.signals[0].choices.len(), 2);
    }
}
