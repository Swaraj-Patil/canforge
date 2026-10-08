//! The parsed form of a DBC file.

use std::collections::BTreeMap;
use std::fmt;

/// A syntax error with the line it occurred on.
#[derive(Debug, Clone, PartialEq)]
pub struct DbcError {
    pub line: usize,
    pub message: String,
}

impl DbcError {
    pub fn new(line: usize, message: String) -> DbcError {
        DbcError { line, message }
    }
}

impl fmt::Display for DbcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for DbcError {}

/// How a signal takes part in multiplexing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mux {
    /// Always present.
    None,
    /// The multiplexer itself (`M`): its value selects which group is present.
    Switch,
    /// Present only when the multiplexer equals this value (`mN`).
    Value(u64),
}

/// How a signal's raw bits are interpreted (`SIG_VALTYPE_`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    Integer,
    Float32,
    Float64,
}

impl ValueType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ValueType::Integer => "integer",
            ValueType::Float32 => "float32",
            ValueType::Float64 => "float64",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Signal {
    pub name: String,
    pub start: u64,
    pub length: u64,
    /// Intel (`@1`) when true, Motorola (`@0`) when false.
    pub little_endian: bool,
    pub signed: bool,
    pub factor: f64,
    pub offset: f64,
    pub minimum: f64,
    pub maximum: f64,
    pub unit: String,
    pub receivers: Vec<String>,
    pub mux: Mux,
    pub value_type: ValueType,
    pub comment: String,
    /// Value descriptions from `VAL_`, in file order.
    pub choices: Vec<(i64, String)>,
    pub line: usize,
}

impl Signal {
    pub fn new(name: String, line: usize) -> Signal {
        Signal {
            name,
            start: 0,
            length: 0,
            little_endian: true,
            signed: false,
            factor: 1.0,
            offset: 0.0,
            minimum: 0.0,
            maximum: 0.0,
            unit: String::new(),
            receivers: Vec::new(),
            mux: Mux::None,
            value_type: ValueType::Integer,
            comment: String::new(),
            choices: Vec::new(),
            line,
        }
    }

    /// Lengths outside 1..=64 are reported by lint and skipped elsewhere.
    pub fn valid_length(&self) -> bool {
        self.length >= 1 && self.length <= 64
    }

    /// The multiplexer value this signal belongs to, if it is multiplexed.
    pub fn mux_value(&self) -> Option<u64> {
        match self.mux {
            Mux::Value(v) => Some(v),
            _ => None,
        }
    }

    /// True when the raw value is a two's complement integer.
    pub fn is_signed_integer(&self) -> bool {
        self.signed && self.value_type == ValueType::Integer
    }

    /// Declared ranges of [0|0] conventionally mean "not specified".
    pub fn has_unspecified_range(&self) -> bool {
        self.minimum == 0.0 && self.maximum == 0.0
    }
}

#[derive(Debug, Clone)]
pub struct Message {
    /// The ID exactly as written, with bit 31 marking extended frames.
    pub raw_id: u64,
    pub frame_id: u64,
    pub is_extended: bool,
    pub name: String,
    pub dlc: u64,
    pub sender: String,
    pub signals: Vec<Signal>,
    pub comment: String,
    pub line: usize,
}

impl Message {
    /// "0x100" for standard frames, "0x18FF50E5" for extended ones.
    pub fn id_hex(&self) -> String {
        if self.is_extended {
            format!("0x{:08X}", self.frame_id)
        } else {
            format!("0x{:03X}", self.frame_id)
        }
    }
}

#[derive(Debug, Clone)]
pub struct Node {
    pub name: String,
    pub comment: String,
    pub line: usize,
}

#[derive(Debug, Clone, Default)]
pub struct Database {
    pub version: String,
    pub comment: String,
    pub nodes: Vec<Node>,
    pub messages: Vec<Message>,
}

impl Database {
    /// The first message with this frame ID, optionally restricted to a format.
    pub fn find_frame(&self, frame_id: u64, extended: Option<bool>) -> Option<&Message> {
        for m in self.messages.iter() {
            let format_matches = match extended {
                Some(e) => m.is_extended == e,
                None => true,
            };
            if m.frame_id == frame_id && format_matches {
                return Some(m);
            }
        }
        None
    }

    pub fn signal_count(&self) -> usize {
        let mut n = 0;
        for m in self.messages.iter() {
            n += m.signals.len();
        }
        n
    }
}

/// Finds messages by frame ID without scanning every message. Build it once
/// per database; it answers exactly as `Database::find_frame` does.
#[derive(Debug, Clone, Default)]
pub struct FrameIndex {
    /// The first message in file order for each (frame ID, extended) pair.
    first: BTreeMap<(u64, bool), usize>,
}

impl FrameIndex {
    pub fn new(db: &Database) -> FrameIndex {
        let mut first = BTreeMap::new();
        for (i, m) in db.messages.iter().enumerate() {
            first.entry((m.frame_id, m.is_extended)).or_insert(i);
        }
        FrameIndex { first }
    }

    /// The index in `db.messages` of the first message with this frame ID
    /// in exactly this format, standard or extended.
    pub fn find_exact(&self, frame_id: u64, extended: bool) -> Option<usize> {
        self.first.get(&(frame_id, extended)).copied()
    }

    /// The index in `db.messages` of the message `Database::find_frame`
    /// returns. Without a format, that is whichever of the standard and the
    /// extended message with this ID comes first in the file.
    pub fn find(&self, frame_id: u64, extended: Option<bool>) -> Option<usize> {
        match extended {
            Some(e) => self.find_exact(frame_id, e),
            None => match (self.find_exact(frame_id, false), self.find_exact(frame_id, true)) {
                (Some(standard), Some(ext)) => Some(standard.min(ext)),
                (standard, ext) => standard.or(ext),
            },
        }
    }
}

/// The placeholder Vector tools use for "no node".
pub const NO_NODE: &str = "Vector__XXX";

/// A pseudo-message Vector tools add to hold unassigned signals. Skipped.
pub const INDEPENDENT_MSG: &str = "VECTOR__INDEPENDENT_SIG_MSG";

/// Statement keywords. Ones we do not interpret are skipped to their `;`.
pub const KEYWORDS: &[&str] = &[
    "VERSION",
    "NS_",
    "BS_",
    "BU_",
    "BO_",
    "SG_",
    "CM_",
    "VAL_",
    "VAL_TABLE_",
    "BA_DEF_",
    "BA_DEF_DEF_",
    "BA_",
    "BA_DEF_REL_",
    "BA_REL_",
    "BA_DEF_DEF_REL_",
    "BU_SG_REL_",
    "BU_EV_REL_",
    "BU_BO_REL_",
    "SIG_VALTYPE_",
    "SIG_GROUP_",
    "SIG_TYPE_REF_",
    "BO_TX_BU_",
    "EV_",
    "ENVVAR_DATA_",
    "SGTYPE_",
    "SGTYPE_VAL_",
    "BA_DEF_SGTYPE_",
    "BA_SGTYPE_",
    "SG_MUL_VAL_",
    "CAT_DEF_",
    "CAT_",
    "FILTER",
    "SIGTYPE_VALTYPE_",
    "NS_DESC_",
    "EV_DATA_",
];

pub fn is_keyword(text: &str) -> bool {
    KEYWORDS.contains(&text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    /// Checks that `FrameIndex::find` picks the message `Database::find_frame`
    /// picks, for every frame ID in `ids`, with and without a format.
    fn check_agrees(src: &str, ids: &[u64]) {
        let db = parse(src).unwrap();
        let index = FrameIndex::new(&db);
        for &id in ids {
            for ext in [None, Some(false), Some(true)] {
                let want = db.find_frame(id, ext).map(|m| m.name.as_str());
                let got = index.find(id, ext).map(|i| db.messages[i].name.as_str());
                assert_eq!(got, want, "frame 0x{:X}, extended {:?}", id, ext);
            }
        }
    }

    // Bit 31 marks an extended ID: 2147483904 is extended frame 0x100.
    const EXTENDED_FIRST: &str = "BO_ 2147483904 Ext: 8 A\nBO_ 256 Std: 8 A\nBO_ 256 StdAgain: 8 A\nBO_ 512 Other: 8 A\n";
    const STANDARD_FIRST: &str = "BO_ 256 Std: 8 A\nBO_ 2147483904 Ext: 8 A\nBO_ 2147483904 ExtAgain: 8 A\n";

    #[test]
    fn frame_index_answers_like_find_frame() {
        check_agrees(EXTENDED_FIRST, &[0x100, 0x200, 0x300]);
        check_agrees(STANDARD_FIRST, &[0x100, 0x200]);
    }

    #[test]
    fn frame_index_without_a_format_picks_the_first_in_the_file() {
        for (src, first) in [(EXTENDED_FIRST, "Ext"), (STANDARD_FIRST, "Std")] {
            let db = parse(src).unwrap();
            let index = FrameIndex::new(&db);
            assert_eq!(index.find(0x100, None).map(|i| db.messages[i].name.as_str()), Some(first));
        }
    }

    #[test]
    fn frame_index_finds_an_exact_id_and_format() {
        let db = parse(EXTENDED_FIRST).unwrap();
        let index = FrameIndex::new(&db);
        let name = |i: Option<usize>| i.map(|i| db.messages[i].name.as_str());
        assert_eq!(name(index.find_exact(0x100, true)), Some("Ext"));
        // A repeated ID (lint E003) resolves to its first message.
        assert_eq!(name(index.find_exact(0x100, false)), Some("Std"));
        assert_eq!(name(index.find_exact(0x200, true)), None);
    }
}
