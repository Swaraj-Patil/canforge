#!/usr/bin/env python3
"""canforge reference model.

An independent, executable specification of canforge written in plain
Python with no dependencies. It is deliberately simple: bits are packed one
at a time, every rule is spelled out, and nothing is clever.

The Rust implementation must agree with this model exactly. CI generates
golden files and decode vectors from this model and fails the build if the
Rust tool produces anything different. The C that both of them generate is
compiled and checked against this model's bit-by-bit packing on thousands
of random frames.
"""

import json
import math
import struct
import sys
from decimal import Decimal

VERSION = "0.1.0"
PROJECT_URL = "https://github.com/Swaraj-Patil/canforge"

# ---------------------------------------------------------------------------
# Character classes (ASCII only, to match Rust's is_ascii_* exactly)
# ---------------------------------------------------------------------------


def is_digit(c):
    return "0" <= c <= "9"


def is_upper(c):
    return "A" <= c <= "Z"


def is_lower(c):
    return "a" <= c <= "z"


def is_alpha(c):
    return is_upper(c) or is_lower(c)


def is_alnum(c):
    return is_alpha(c) or is_digit(c)


def is_digits(s):
    return len(s) > 0 and all(is_digit(c) for c in s)


# ---------------------------------------------------------------------------
# Number formatting shared by C, Python and JSON output.
#
# Shortest round-trip digits, never in exponent form, always with a decimal
# point. Rust's "{}" Display for f64 has the same digits and also never uses
# an exponent, so both sides print identical text.
# ---------------------------------------------------------------------------


def fmt_f64(x):
    s = format(Decimal(repr(float(x))), "f")
    if "." not in s:
        s += ".0"
    return s


def json_num(x):
    if math.isnan(x) or math.isinf(x):
        return "null"
    return fmt_f64(x)


def next_down(f):
    return math.nextafter(f, -math.inf)


def next_up(f):
    return math.nextafter(f, math.inf)


def f64_at_most(n):
    """Largest double that is <= the integer n."""
    f = float(n)
    if int(f) > n:
        f = next_down(f)
    return f


def f64_at_least(n):
    """Smallest double that is >= the integer n."""
    f = float(n)
    if int(f) < n:
        f = next_up(f)
    return f


# ---------------------------------------------------------------------------
# Errors and tokens
# ---------------------------------------------------------------------------


class DbcError(Exception):
    def __init__(self, line, message):
        super().__init__("line %d: %s" % (line, message))
        self.line = line
        self.message = message


class Tok:
    __slots__ = ("kind", "text", "line")

    def __init__(self, kind, text, line):
        self.kind = kind  # "ident" | "num" | "str" | "punct"
        self.text = text
        self.line = line


PUNCT = ":|@+-()[],;"
WHITESPACE = " \t\r\x0c\x0b"


def lex(src):
    toks = []
    i = 0
    n = len(src)
    line = 1
    while i < n:
        c = src[i]
        if c == "\n":
            line += 1
            i += 1
            continue
        if c in WHITESPACE:
            i += 1
            continue
        if c == "/" and i + 1 < n and src[i + 1] == "/":
            while i < n and src[i] != "\n":
                i += 1
            continue
        if c == '"':
            start_line = line
            i += 1
            buf = []
            closed = False
            while i < n:
                ch = src[i]
                if ch == "\\" and i + 1 < n and (src[i + 1] == '"' or src[i + 1] == "\\"):
                    buf.append(src[i + 1])
                    i += 2
                    continue
                if ch == '"':
                    closed = True
                    i += 1
                    break
                if ch == "\n":
                    line += 1
                buf.append(ch)
                i += 1
            if not closed:
                raise DbcError(start_line, "unterminated string")
            toks.append(Tok("str", "".join(buf), start_line))
            continue
        if is_alpha(c) or c == "_":
            j = i
            while j < n and (is_alnum(src[j]) or src[j] == "_"):
                j += 1
            toks.append(Tok("ident", src[i:j], line))
            i = j
            continue
        if is_digit(c) or (c == "." and i + 1 < n and is_digit(src[i + 1])):
            j = i
            while j < n and is_digit(src[j]):
                j += 1
            if j < n and src[j] == ".":
                j += 1
                while j < n and is_digit(src[j]):
                    j += 1
            if j < n and (src[j] == "e" or src[j] == "E"):
                k = j + 1
                if k < n and (src[k] == "+" or src[k] == "-"):
                    k += 1
                if k < n and is_digit(src[k]):
                    j = k
                    while j < n and is_digit(src[j]):
                        j += 1
            toks.append(Tok("num", src[i:j], line))
            i = j
            continue
        if c in PUNCT:
            toks.append(Tok("punct", c, line))
            i += 1
            continue
        raise DbcError(line, "unexpected character %r" % c)
    return toks


# ---------------------------------------------------------------------------
# Model
# ---------------------------------------------------------------------------

MUX_NONE = None
MUX_SWITCH = "M"  # the multiplexer signal itself; a multiplexed signal stores an int


class Signal:
    def __init__(self):
        self.name = ""
        self.start = 0
        self.length = 0
        self.little_endian = True
        self.signed = False
        self.factor = 1.0
        self.offset = 0.0
        self.minimum = 0.0
        self.maximum = 0.0
        self.unit = ""
        self.receivers = []
        self.mux = MUX_NONE
        self.value_type = "integer"  # "integer" | "float32" | "float64"
        self.comment = ""
        self.choices = []  # list of (int value, str label)
        self.line = 0


class Message:
    def __init__(self):
        self.raw_id = 0
        self.frame_id = 0
        self.is_extended = False
        self.name = ""
        self.dlc = 0
        self.sender = ""
        self.signals = []
        self.comment = ""
        self.line = 0


class Node:
    def __init__(self, name, line):
        self.name = name
        self.comment = ""
        self.line = line


class Database:
    def __init__(self):
        self.version = ""
        self.comment = ""
        self.nodes = []
        self.messages = []


NO_NODE = "Vector__XXX"
INDEPENDENT_MSG = "VECTOR__INDEPENDENT_SIG_MSG"

# Statements we recognise by keyword. Anything here that we do not parse is
# skipped up to its terminating semicolon.
KEYWORDS = {
    "VERSION", "NS_", "BS_", "BU_", "BO_", "SG_", "CM_", "VAL_", "VAL_TABLE_",
    "BA_DEF_", "BA_DEF_DEF_", "BA_", "BA_DEF_REL_", "BA_REL_", "BA_DEF_DEF_REL_",
    "BU_SG_REL_", "BU_EV_REL_", "BU_BO_REL_", "SIG_VALTYPE_", "SIG_GROUP_",
    "SIG_TYPE_REF_", "BO_TX_BU_", "EV_", "ENVVAR_DATA_", "SGTYPE_", "SGTYPE_VAL_",
    "BA_DEF_SGTYPE_", "BA_SGTYPE_", "SG_MUL_VAL_", "CAT_DEF_", "CAT_", "FILTER",
    "SIGTYPE_VALTYPE_", "NS_DESC_", "EV_DATA_",
}

U64_MAX = (1 << 64) - 1
# Far beyond any real frame (64 bytes = 512 bits), small enough that bit
# arithmetic can never overflow in the Rust implementation.
MAX_START_BIT = 65535
I64_MAX = (1 << 63) - 1


# ---------------------------------------------------------------------------
# Parser
# ---------------------------------------------------------------------------


class Parser:
    def __init__(self, toks):
        self.t = toks
        self.i = 0

    def peek(self):
        if self.i < len(self.t):
            return self.t[self.i]
        return None

    def last_line(self):
        if self.t:
            return self.t[-1].line
        return 1

    def next(self, what):
        tok = self.peek()
        if tok is None:
            raise DbcError(self.last_line(), "unexpected end of file, expected %s" % what)
        self.i += 1
        return tok

    def is_punct(self, ch):
        tok = self.peek()
        return tok is not None and tok.kind == "punct" and tok.text == ch

    def at_keyword(self):
        tok = self.peek()
        return tok is not None and tok.kind == "ident" and tok.text in KEYWORDS

    def expect_punct(self, ch):
        tok = self.next("'%s'" % ch)
        if tok.kind != "punct" or tok.text != ch:
            raise DbcError(tok.line, "expected '%s', found '%s'" % (ch, tok.text))
        return tok

    def expect_ident(self, what):
        tok = self.next(what)
        if tok.kind != "ident":
            raise DbcError(tok.line, "expected %s, found '%s'" % (what, tok.text))
        return tok

    def expect_str(self, what):
        tok = self.next(what)
        if tok.kind != "str":
            raise DbcError(tok.line, "expected %s (a quoted string), found '%s'" % (what, tok.text))
        return tok

    def parse_uint(self, what):
        tok = self.next(what)
        if tok.kind != "num" or not is_digits(tok.text):
            raise DbcError(tok.line, "expected %s (a non-negative integer), found '%s'" % (what, tok.text))
        value = int(tok.text)
        if value > U64_MAX:
            raise DbcError(tok.line, "%s %s is too large" % (what, tok.text))
        return value

    def parse_int(self, what):
        negative = False
        if self.is_punct("-"):
            self.next(what)
            negative = True
        elif self.is_punct("+"):
            self.next(what)
        tok = self.next(what)
        if tok.kind != "num" or not is_digits(tok.text):
            raise DbcError(tok.line, "expected %s (an integer), found '%s'" % (what, tok.text))
        value = int(tok.text)
        if value > I64_MAX:
            raise DbcError(tok.line, "%s %s is too large" % (what, tok.text))
        return -value if negative else value

    def parse_number(self, what):
        negative = False
        if self.is_punct("-"):
            self.next(what)
            negative = True
        elif self.is_punct("+"):
            self.next(what)
        tok = self.next(what)
        if tok.kind != "num":
            raise DbcError(tok.line, "expected %s (a number), found '%s'" % (what, tok.text))
        value = float(tok.text)
        return -value if negative else value

    def skip_statement(self):
        while True:
            tok = self.peek()
            if tok is None:
                return
            self.i += 1
            if tok.kind == "punct" and tok.text == ";":
                return


def find_message(db, raw_id):
    for m in db.messages:
        if m.raw_id == raw_id:
            return m
    return None


def find_signal(msg, name):
    for s in msg.signals:
        if s.name == name:
            return s
    return None


def parse_message(p, line):
    msg = Message()
    msg.line = line
    raw_id = p.parse_uint("frame ID")
    if raw_id > 0xFFFFFFFF:
        raise DbcError(line, "frame ID %d does not fit in 32 bits" % raw_id)
    msg.raw_id = raw_id
    msg.is_extended = (raw_id & 0x80000000) != 0
    msg.frame_id = (raw_id & 0x7FFFFFFF) if msg.is_extended else raw_id
    msg.name = p.expect_ident("message name").text
    p.expect_punct(":")
    msg.dlc = p.parse_uint("message length")
    msg.sender = p.expect_ident("transmitter").text
    return msg


def parse_signal(p, line):
    s = Signal()
    s.line = line
    s.name = p.expect_ident("signal name").text
    tok = p.peek()
    if tok is not None and tok.kind == "ident":
        mtok = p.next("multiplexer indicator")
        txt = mtok.text
        if txt == "M":
            s.mux = MUX_SWITCH
        elif len(txt) > 1 and txt[0] == "m" and is_digits(txt[1:]):
            value = int(txt[1:])
            if value > U64_MAX:
                raise DbcError(mtok.line, "multiplexer value in '%s' is too large" % txt)
            s.mux = value
        elif len(txt) > 2 and txt[0] == "m" and txt[-1] == "M" and is_digits(txt[1:-1]):
            raise DbcError(mtok.line, "extended multiplexing ('%s') is not supported" % txt)
        else:
            raise DbcError(mtok.line, "invalid multiplexer indicator '%s'" % txt)
    p.expect_punct(":")
    s.start = p.parse_uint("start bit")
    if s.start > MAX_START_BIT:
        raise DbcError(line, "start bit %d is out of range" % s.start)
    p.expect_punct("|")
    s.length = p.parse_uint("signal length")
    p.expect_punct("@")
    order_tok = p.peek()
    order = p.parse_uint("byte order")
    if order != 0 and order != 1:
        raise DbcError(order_tok.line, "byte order must be 0 (Motorola) or 1 (Intel), found %d" % order)
    s.little_endian = order == 1
    sign = p.next("value type '+' or '-'")
    if sign.kind != "punct" or (sign.text != "+" and sign.text != "-"):
        raise DbcError(sign.line, "expected value type '+' or '-', found '%s'" % sign.text)
    s.signed = sign.text == "-"
    p.expect_punct("(")
    s.factor = p.parse_number("factor")
    p.expect_punct(",")
    s.offset = p.parse_number("offset")
    p.expect_punct(")")
    p.expect_punct("[")
    s.minimum = p.parse_number("minimum")
    p.expect_punct("|")
    s.maximum = p.parse_number("maximum")
    p.expect_punct("]")
    s.unit = p.expect_str("unit").text
    tok = p.peek()
    if tok is not None and tok.kind == "ident" and tok.text not in KEYWORDS:
        s.receivers.append(p.next("receiver").text)
        while True:
            tok = p.peek()
            if tok is None:
                break
            if tok.kind == "punct" and tok.text == ",":
                p.next("receiver")
                s.receivers.append(p.expect_ident("receiver").text)
                continue
            if tok.kind == "ident" and tok.text not in KEYWORDS:
                s.receivers.append(p.next("receiver").text)
                continue
            break
    return s


def parse_comment(p, db):
    tok = p.peek()
    if tok is None:
        raise DbcError(p.last_line(), "unexpected end of file in CM_")
    if tok.kind == "str":
        db.comment = p.next("comment").text
    elif tok.kind == "ident" and tok.text == "BU_":
        p.next("BU_")
        name = p.expect_ident("node name").text
        text = p.expect_str("comment").text
        for node in db.nodes:
            if node.name == name:
                node.comment = text
    elif tok.kind == "ident" and tok.text == "BO_":
        p.next("BO_")
        raw_id = p.parse_uint("frame ID")
        text = p.expect_str("comment").text
        msg = find_message(db, raw_id)
        if msg is not None:
            msg.comment = text
    elif tok.kind == "ident" and tok.text == "SG_":
        p.next("SG_")
        raw_id = p.parse_uint("frame ID")
        name = p.expect_ident("signal name").text
        text = p.expect_str("comment").text
        msg = find_message(db, raw_id)
        if msg is not None:
            sig = find_signal(msg, name)
            if sig is not None:
                sig.comment = text
    else:
        p.skip_statement()
        return
    p.expect_punct(";")


def parse_val(p, db):
    tok = p.peek()
    if tok is None or tok.kind != "num":
        p.skip_statement()
        return
    raw_id = p.parse_uint("frame ID")
    name = p.expect_ident("signal name").text
    pairs = []
    while True:
        if p.is_punct(";"):
            p.next("';'")
            break
        value = p.parse_int("value")
        label = p.expect_str("value description").text
        pairs.append((value, label))
    msg = find_message(db, raw_id)
    if msg is not None:
        sig = find_signal(msg, name)
        if sig is not None:
            sig.choices = pairs


def parse_sig_valtype(p, db):
    raw_id = p.parse_uint("frame ID")
    name = p.expect_ident("signal name").text
    if p.is_punct(":"):
        p.next("':'")
    vt_tok = p.peek()
    vt = p.parse_uint("value type")
    p.expect_punct(";")
    if vt == 0:
        kind = "integer"
    elif vt == 1:
        kind = "float32"
    elif vt == 2:
        kind = "float64"
    else:
        raise DbcError(vt_tok.line, "SIG_VALTYPE_ must be 0, 1 or 2, found %d" % vt)
    msg = find_message(db, raw_id)
    if msg is not None:
        sig = find_signal(msg, name)
        if sig is not None:
            sig.value_type = kind


def parse(src):
    if src.startswith("\ufeff"):
        src = src[1:]
    p = Parser(lex(src))
    db = Database()
    while p.peek() is not None:
        tok = p.next("a statement")
        if tok.kind != "ident":
            raise DbcError(tok.line, "unexpected '%s' at the start of a statement" % tok.text)
        kw = tok.text
        if kw == "VERSION":
            db.version = p.expect_str("version string").text
        elif kw == "NS_":
            p.expect_punct(":")
            while True:
                t = p.peek()
                if t is None:
                    break
                if t.kind == "ident" and (t.text == "BS_" or t.text == "BU_" or t.text == "BO_"):
                    break
                p.i += 1
        elif kw == "BS_":
            p.expect_punct(":")
            while p.peek() is not None and not p.at_keyword():
                p.i += 1
        elif kw == "BU_":
            p.expect_punct(":")
            while True:
                t = p.peek()
                if t is None or t.kind != "ident" or t.text in KEYWORDS:
                    break
                p.i += 1
                db.nodes.append(Node(t.text, t.line))
        elif kw == "BO_":
            msg = parse_message(p, tok.line)
            while True:
                t = p.peek()
                if t is None or t.kind != "ident" or t.text != "SG_":
                    break
                p.i += 1
                msg.signals.append(parse_signal(p, t.line))
            if msg.name != INDEPENDENT_MSG:
                db.messages.append(msg)
        elif kw == "SG_":
            raise DbcError(tok.line, "SG_ must follow a BO_ message definition")
        elif kw == "CM_":
            parse_comment(p, db)
        elif kw == "VAL_":
            parse_val(p, db)
        elif kw == "SIG_VALTYPE_":
            parse_sig_valtype(p, db)
        elif kw in KEYWORDS:
            p.skip_statement()
        else:
            raise DbcError(tok.line, "unknown keyword '%s'" % kw)
    return db


def parse_file(path):
    with open(path, "rb") as f:
        return parse(f.read().decode("utf-8", errors="replace"))


# ---------------------------------------------------------------------------
# Bit layout
#
# Bits are numbered DBC-style: absolute bit b lives in byte b // 8 at bit
# position b % 8, where position 0 is the least significant bit.
#
# Intel (little endian, @1): the start bit is the signal's LSB, and the raw
# value's bit i sits at absolute bit start + i.
#
# Motorola (big endian, @0): the start bit is the signal's MSB. Walking from
# the MSB toward the LSB, the position moves down within a byte and, after
# position 0, continues at position 7 of the next byte.
# ---------------------------------------------------------------------------


def valid_length(s):
    return 1 <= s.length <= 64


def bit_positions(start, length, little_endian):
    """Return a list indexed by raw bit (0 = LSB) of (byte, bit) pairs."""
    pos = [None] * length
    if little_endian:
        for i in range(length):
            a = start + i
            pos[i] = (a // 8, a % 8)
    else:
        b = start
        for k in range(length):
            pos[length - 1 - k] = (b // 8, b % 8)
            if b % 8 == 0:
                b += 15
            else:
                b -= 1
    return pos


def signal_bits(s):
    return bit_positions(s.start, s.length, s.little_endian)


def segments(s):
    """Group a signal's bits into one contiguous run per byte.

    Returns (byte, byte_bit_lo, raw_bit_lo, count) sorted by byte. Within a
    run, byte bit p holds raw bit raw_bit_lo + (p - byte_bit_lo).
    """
    groups = {}
    for raw_bit, (byte, bit) in enumerate(signal_bits(s)):
        groups.setdefault(byte, []).append((bit, raw_bit))
    out = []
    for byte in sorted(groups):
        items = groups[byte]
        p_lo = min(b for b, _ in items)
        r_lo = None
        for b, r in items:
            if b == p_lo:
                r_lo = r
        for b, r in items:
            if r - r_lo != b - p_lo:
                raise AssertionError("non-contiguous run in byte %d of %s" % (byte, s.name))
        out.append((byte, p_lo, r_lo, len(items)))
    return out


def raw_range(length, signed):
    if signed:
        return (-(1 << (length - 1)), (1 << (length - 1)) - 1)
    return (0, (1 << length) - 1)


def extract_raw(data, s):
    """Read a signal's unsigned raw bits one bit at a time."""
    v = 0
    for raw_bit, (byte, bit) in enumerate(signal_bits(s)):
        if (data[byte] >> bit) & 1:
            v |= 1 << raw_bit
    return v


def insert_raw(data, s, raw):
    """Write a signal's raw value one bit at a time. raw may be negative."""
    u = raw & ((1 << s.length) - 1)
    for raw_bit, (byte, bit) in enumerate(signal_bits(s)):
        if (u >> raw_bit) & 1:
            data[byte] |= 1 << bit
        else:
            data[byte] &= ~(1 << bit) & 0xFF


def to_signed(v, length):
    if v & (1 << (length - 1)):
        return v - (1 << length)
    return v


def raw_value(data, s):
    v = extract_raw(data, s)
    if s.signed and s.value_type == "integer":
        v = to_signed(v, s.length)
    return v


def physical_value(s, raw):
    """raw is the value from raw_value()."""
    if s.value_type == "float32":
        f = struct.unpack("<f", (raw & 0xFFFFFFFF).to_bytes(4, "little"))[0]
        return f * s.factor + s.offset
    if s.value_type == "float64":
        f = struct.unpack("<d", (raw & U64_MAX).to_bytes(8, "little"))[0]
        return f * s.factor + s.offset
    return float(raw) * s.factor + s.offset


# ---------------------------------------------------------------------------
# Identifiers for generated code
# ---------------------------------------------------------------------------

C_KEYWORDS = {
    "auto", "break", "case", "char", "const", "continue", "default", "do",
    "double", "else", "enum", "extern", "float", "for", "goto", "if",
    "inline", "int", "long", "register", "restrict", "return", "short",
    "signed", "sizeof", "static", "struct", "switch", "typedef", "union",
    "unsigned", "void", "volatile", "while", "bool", "true", "false",
}


def snake(name):
    out = []
    n = len(name)
    for i in range(n):
        c = name[i]
        if is_upper(c):
            if i > 0:
                prev = name[i - 1]
                next_lower = i + 1 < n and is_lower(name[i + 1])
                if is_lower(prev) or is_digit(prev) or (is_upper(prev) and next_lower):
                    out.append("_")
            out.append(c.lower())
        elif is_alnum(c):
            out.append(c)
        else:
            out.append("_")
    collapsed = []
    prev_us = False
    for c in "".join(out):
        if c == "_":
            if not prev_us:
                collapsed.append("_")
            prev_us = True
        else:
            collapsed.append(c)
            prev_us = False
    s = "".join(collapsed).strip("_")
    if not s:
        return "x"
    if is_digit(s[0]):
        return "x_" + s
    return s


def c_ident(name):
    s = snake(name)
    if s in C_KEYWORDS:
        s += "_"
    return s


def unique_idents(names):
    used = set()
    out = []
    for n in names:
        cand = n
        k = 2
        while cand in used:
            cand = "%s_%d" % (n, k)
            k += 1
        used.add(cand)
        out.append(cand)
    return out


def fmt_id(msg):
    if msg.is_extended:
        return "0x%08X" % msg.frame_id
    return "0x%03X" % msg.frame_id


# ---------------------------------------------------------------------------
# Lint
# ---------------------------------------------------------------------------

RULES = [
    ("E001", "signal-overlap", "error", "Two signals in the same frame use the same bits."),
    ("E002", "signal-out-of-frame", "error", "A signal extends past the end of its frame."),
    ("E003", "duplicate-frame-id", "error", "Two messages share a frame ID."),
    ("E004", "duplicate-name", "error", "A message or signal name is defined twice."),
    ("E005", "zero-factor", "error", "A signal's scaling factor is zero, so it cannot be encoded."),
    ("E006", "invalid-length", "error", "A signal is shorter than 1 bit or longer than 64 bits."),
    ("E007", "frame-id-out-of-range", "error", "A frame ID does not fit its 11-bit or 29-bit format."),
    ("E008", "multiplexed-without-multiplexer", "error", "A multiplexed signal has no multiplexer to select it."),
    ("E009", "multiple-multiplexers", "error", "A message declares more than one multiplexer."),
    ("E010", "invalid-frame-length", "error", "A frame length is not a valid CAN or CAN FD length."),
    ("E011", "float-length-mismatch", "error", "A float signal is not 32 or 64 bits long."),
    ("E012", "multiplex-value-out-of-range", "error", "A multiplexer value cannot be represented by the multiplexer signal."),
    ("W001", "range-not-representable", "warning", "A declared range extends past what the raw bits can encode."),
    ("W002", "min-greater-than-max", "warning", "A declared minimum is greater than the maximum."),
    ("W003", "unknown-node", "warning", "A transmitter or receiver is not declared in BU_."),
    ("W004", "c-name-collision", "warning", "Two signal names map to the same C identifier."),
    ("W005", "choice-out-of-range", "warning", "A value description uses a raw value the signal cannot hold."),
    ("I001", "unused-node", "info", "A node never transmits or receives anything."),
    ("I002", "can-fd-frame", "info", "A frame is longer than 8 bytes and needs CAN FD."),
]
RULE_INFO = {r[0]: r for r in RULES}

FD_LENGTHS = [0, 1, 2, 3, 4, 5, 6, 7, 8, 12, 16, 20, 24, 32, 48, 64]


def diag(rule, message, line, msg_name="", sig_name=""):
    info = RULE_INFO[rule]
    return {
        "rule": rule,
        "name": info[1],
        "severity": info[2],
        "message": message,
        "line": line,
        "message_name": msg_name,
        "signal_name": sig_name,
    }


def mux_conflict(a, b):
    """Overlapping bits are fine only between alternatives of the same multiplexer."""
    if isinstance(a.mux, int) and isinstance(b.mux, int) and a.mux != b.mux:
        return False
    return True


def fmt_bits(bits):
    shown = ", ".join(str(b) for b in bits[:8])
    if len(bits) > 8:
        shown += " (+%d more)" % (len(bits) - 8)
    return shown


def lint(db):
    out = []
    node_names = [n.name for n in db.nodes]
    check_nodes = len(node_names) > 0

    # Frame IDs and message names across the database.
    for i, m in enumerate(db.messages):
        for j in range(i):
            o = db.messages[j]
            if o.frame_id == m.frame_id and o.is_extended == m.is_extended:
                out.append(diag("E003", "frame ID %s is used by both '%s' and '%s'" % (fmt_id(m), o.name, m.name), m.line, m.name))
                break
        for j in range(i):
            if db.messages[j].name == m.name:
                out.append(diag("E004", "message name '%s' is defined more than once" % m.name, m.line, m.name))
                break

    for m in db.messages:
        if m.is_extended:
            if m.frame_id > 0x1FFFFFFF:
                out.append(diag("E007", "extended frame ID 0x%X exceeds the 29-bit range" % m.frame_id, m.line, m.name))
        elif m.frame_id > 0x7FF:
            out.append(diag("E007", "frame ID 0x%X exceeds the 11-bit standard range; set bit 31 to mark it extended or fix the ID" % m.frame_id, m.line, m.name))

        if m.dlc not in FD_LENGTHS:
            out.append(diag("E010", "message '%s' has length %d; CAN frames must be 0-8 bytes and CAN FD frames 12, 16, 20, 24, 32, 48 or 64" % (m.name, m.dlc), m.line, m.name))
        elif m.dlc > 8:
            out.append(diag("I002", "message '%s' is %d bytes long, so it needs CAN FD" % (m.name, m.dlc), m.line, m.name))

        if check_nodes and m.sender != NO_NODE and m.sender not in node_names:
            out.append(diag("W003", "transmitter '%s' of message '%s' is not declared in BU_" % (m.sender, m.name), m.line, m.name))

        multiplexers = [s for s in m.signals if s.mux == MUX_SWITCH]
        if len(multiplexers) > 1:
            out.append(diag("E009", "message '%s' has more than one multiplexer ('%s' and '%s')" % (m.name, multiplexers[0].name, multiplexers[1].name), multiplexers[1].line, m.name, multiplexers[1].name))
        mux_sig = multiplexers[0] if multiplexers else None

        for i, s in enumerate(m.signals):
            for j in range(i):
                if m.signals[j].name == s.name:
                    out.append(diag("E004", "signal name '%s' is defined more than once in message '%s'" % (s.name, m.name), s.line, m.name, s.name))
                    break

            if not valid_length(s):
                out.append(diag("E006", "signal '%s' has length %d; lengths must be between 1 and 64 bits" % (s.name, s.length), s.line, m.name, s.name))
            if s.factor == 0.0:
                out.append(diag("E005", "signal '%s' has a scaling factor of 0" % s.name, s.line, m.name, s.name))
            if s.value_type == "float32" and s.length != 32:
                out.append(diag("E011", "signal '%s' is declared float32 but is %d bits long (expected 32)" % (s.name, s.length), s.line, m.name, s.name))
            if s.value_type == "float64" and s.length != 64:
                out.append(diag("E011", "signal '%s' is declared float64 but is %d bits long (expected 64)" % (s.name, s.length), s.line, m.name, s.name))

            if isinstance(s.mux, int):
                if mux_sig is None:
                    out.append(diag("E008", "signal '%s' is multiplexed (m%d) but message '%s' has no multiplexer signal" % (s.name, s.mux, m.name), s.line, m.name, s.name))
                elif valid_length(mux_sig):
                    lo, hi = raw_range(mux_sig.length, mux_sig.signed)
                    if s.mux < lo or s.mux > hi:
                        out.append(diag("E012", "signal '%s' uses multiplexer value %d, which multiplexer '%s' cannot represent" % (s.name, s.mux, mux_sig.name), s.line, m.name, s.name))

            if s.minimum > s.maximum:
                out.append(diag("W002", "signal '%s' has minimum %s greater than maximum %s" % (s.name, fmt_f64(s.minimum), fmt_f64(s.maximum)), s.line, m.name, s.name))
            elif valid_length(s) and s.value_type == "integer" and s.factor != 0.0 and not (s.minimum == 0.0 and s.maximum == 0.0):
                rmin, rmax = raw_range(s.length, s.signed)
                a = float(rmin) * s.factor + s.offset
                b = float(rmax) * s.factor + s.offset
                lo_p = min(a, b)
                hi_p = max(a, b)
                tol = 1e-6 * max(max(1.0, abs(lo_p)), abs(hi_p))
                if s.minimum < lo_p - tol or s.maximum > hi_p + tol:
                    out.append(diag("W001", "signal '%s' declares range [%s|%s] but its %d-bit raw value can only represent [%s|%s]" % (s.name, fmt_f64(s.minimum), fmt_f64(s.maximum), s.length, fmt_f64(lo_p), fmt_f64(hi_p)), s.line, m.name, s.name))

            if check_nodes:
                seen = []
                for r in s.receivers:
                    if r != NO_NODE and r not in node_names and r not in seen:
                        seen.append(r)
                        out.append(diag("W003", "receiver '%s' of signal '%s' is not declared in BU_" % (r, s.name), s.line, m.name, s.name))

            if valid_length(s) and s.value_type == "integer":
                lo, hi = raw_range(s.length, s.signed)
                for value, label in s.choices:
                    if value < lo or value > hi:
                        out.append(diag("W005", "value description %d (\"%s\") for signal '%s' is outside its raw range [%d|%d]" % (value, label, s.name, lo, hi), s.line, m.name, s.name))

            if valid_length(s):
                beyond = False
                for byte, _ in signal_bits(s):
                    if byte >= m.dlc:
                        beyond = True
                if beyond:
                    out.append(diag("E002", "signal '%s' extends beyond the %d-byte frame of message '%s'" % (s.name, m.dlc, m.name), s.line, m.name, s.name))

        valid = [s for s in m.signals if valid_length(s)]
        for i in range(len(valid)):
            a = valid[i]
            a_bits = set(byte * 8 + bit for byte, bit in signal_bits(a))
            for j in range(i + 1, len(valid)):
                b = valid[j]
                if not mux_conflict(a, b):
                    continue
                b_bits = set(byte * 8 + bit for byte, bit in signal_bits(b))
                common = sorted(a_bits & b_bits)
                if common:
                    out.append(diag("E001", "signals '%s' and '%s' in message '%s' overlap at bit %s" % (a.name, b.name, m.name, fmt_bits(common)), b.line, m.name, b.name))

        idents = {}
        for s in m.signals:
            ident = c_ident(s.name)
            if ident in idents and idents[ident] != s.name:
                out.append(diag("W004", "signals '%s' and '%s' in message '%s' both map to C identifier '%s'" % (idents[ident], s.name, m.name, ident), s.line, m.name, s.name))
            elif ident not in idents:
                idents[ident] = s.name

    if check_nodes:
        used = set()
        for m in db.messages:
            used.add(m.sender)
            for s in m.signals:
                for r in s.receivers:
                    used.add(r)
        for n in db.nodes:
            if n.name not in used:
                out.append(diag("I001", "node '%s' is declared but never transmits or receives" % n.name, n.line))
    return out


def has_errors(diags):
    return any(d["severity"] == "error" for d in diags)


# ---------------------------------------------------------------------------
# Decode
# ---------------------------------------------------------------------------


class DecodeError(Exception):
    pass


def find_frame(db, frame_id, extended=None):
    for m in db.messages:
        if m.frame_id == frame_id and (extended is None or m.is_extended == extended):
            return m
    return None


def decode(db, frame_id, data, extended=None):
    m = find_frame(db, frame_id, extended)
    if m is None:
        raise DecodeError("no message with frame ID 0x%X" % frame_id)
    if len(data) < m.dlc:
        raise DecodeError("message '%s' needs %d bytes, got %d" % (m.name, m.dlc, len(data)))
    mux_value = None
    for s in m.signals:
        if s.mux == MUX_SWITCH and valid_length(s):
            if all(byte < len(data) for byte, _ in signal_bits(s)):
                mux_value = raw_value(data, s)
            break
    results = []
    for s in m.signals:
        if isinstance(s.mux, int) and s.mux != mux_value:
            continue
        if not valid_length(s):
            raise DecodeError("signal '%s' has an invalid length" % s.name)
        for byte, _ in signal_bits(s):
            if byte >= len(data):
                raise DecodeError("signal '%s' extends beyond the frame" % s.name)
        raw = raw_value(data, s)
        label = None
        if s.value_type == "integer":
            for value, text in s.choices:
                if value == raw:
                    label = text
                    break
        results.append({
            "name": s.name,
            "raw": str(raw),
            "physical": physical_value(s, raw),
            "unit": s.unit,
            "label": label,
        })
    return {
        "message": m.name,
        "frame_id": m.frame_id,
        "extended": m.is_extended,
        "mux": None if mux_value is None else str(mux_value),
        "signals": results,
    }


# ---------------------------------------------------------------------------
# Shared code generation helpers
# ---------------------------------------------------------------------------


class CodegenError(Exception):
    pass


def comment_text(s):
    s = s.replace("\r\n", " ").replace("\n", " ").replace("\r", " ")
    s = s.replace("*/", "* /")
    return s.strip()


def c_type(s):
    if s.value_type == "float32":
        return "uint32_t"
    if s.value_type == "float64":
        return "uint64_t"
    if s.length <= 8:
        bits = 8
    elif s.length <= 16:
        bits = 16
    elif s.length <= 32:
        bits = 32
    else:
        bits = 64
    return ("int%d_t" if s.signed else "uint%d_t") % bits


def type_range(s):
    t = c_type(s)
    bits = int(t.replace("uint", "").replace("int", "").replace("_t", ""))
    if t.startswith("u"):
        return (0, (1 << bits) - 1)
    return (-(1 << (bits - 1)), (1 << (bits - 1)) - 1)


def is_unspecified_range(s):
    return s.minimum == 0.0 and s.maximum == 0.0


def checked_raw_bounds(s):
    """Raw bounds implied by the declared physical range, clamped to the signal."""
    qa = (s.minimum - s.offset) / s.factor
    qb = (s.maximum - s.offset) / s.factor
    if s.factor > 0.0:
        lo_q, hi_q = qa, qb
    else:
        lo_q, hi_q = qb, qa
    raw_lo = math.ceil(lo_q - 1e-9 * max(1.0, abs(lo_q)))
    raw_hi = math.floor(hi_q + 1e-9 * max(1.0, abs(hi_q)))
    rmin, rmax = raw_range(s.length, s.signed)
    raw_lo = max(raw_lo, rmin)
    raw_hi = min(raw_hi, rmax)
    return raw_lo, raw_hi


def scale_expr(base, s):
    expr = base
    if s.factor != 1.0:
        expr = "%s * %s" % (expr, fmt_f64(s.factor))
    if s.offset > 0.0:
        expr = "%s + %s" % (expr, fmt_f64(s.offset))
    elif s.offset < 0.0:
        expr = "%s - %s" % (expr, fmt_f64(-s.offset))
    return expr


def unscale_expr(s):
    if s.offset > 0.0:
        num = "(value - %s)" % fmt_f64(s.offset)
    elif s.offset < 0.0:
        num = "(value + %s)" % fmt_f64(-s.offset)
    else:
        num = "value"
    if s.factor != 1.0:
        return "%s / %s" % (num, fmt_f64(s.factor))
    return num


class Names:
    """Stable, collision-free identifiers for one database."""

    def __init__(self, db):
        self.msg = unique_idents([c_ident(m.name) for m in db.messages])
        self.sig = []
        self.choice = []
        for m in db.messages:
            sig_names = unique_idents([c_ident(s.name) for s in m.signals])
            self.sig.append(sig_names)
            per_sig = []
            for s in m.signals:
                per_sig.append(unique_idents([snake(label).upper() for _, label in s.choices]))
            self.choice.append(per_sig)


def require_clean(db):
    errors = [d for d in lint(db) if d["severity"] == "error"]
    if errors:
        raise CodegenError("cannot generate code: the database has %d lint error(s); run 'canforge lint' to see them" % len(errors))


def split_groups(m):
    """Signals outside the multiplexer switch, and (value, signals) groups."""
    plain = []
    groups = []
    for s in m.signals:
        if isinstance(s.mux, int):
            for g in groups:
                if g[0] == s.mux:
                    g[1].append(s)
                    break
            else:
                groups.append((s.mux, [s]))
        else:
            plain.append(s)
    groups.sort(key=lambda g: g[0])
    mux_sig = None
    for s in m.signals:
        if s.mux == MUX_SWITCH:
            mux_sig = s
    return plain, groups, mux_sig


# ---------------------------------------------------------------------------
# C code generation
# ---------------------------------------------------------------------------


def c_pack_lines(s, field, indent):
    lines = []
    lines.append("%s/* %s */" % (indent, s.name))
    lines.append("%sv = (uint64_t)src_p->%s;" % (indent, field))
    for byte, p_lo, r_lo, count in segments(s):
        mask = (1 << count) - 1
        val = "v" if r_lo == 0 else "(v >> %d)" % r_lo
        term = "(%s & 0x%Xu)" % (val, mask)
        if p_lo > 0:
            term = "(%s << %d)" % (term, p_lo)
        lines.append("%sdst_p[%d] = (uint8_t)(dst_p[%d] | (uint8_t)%s);" % (indent, byte, byte, term))
    return lines


def c_unpack_lines(s, field, indent):
    lines = []
    lines.append("%s/* %s */" % (indent, s.name))
    lines.append("%sv = 0u;" % indent)
    for byte, p_lo, r_lo, count in segments(s):
        term = "(uint64_t)src_p[%d]" % byte
        if p_lo > 0:
            term = "(%s >> %d)" % (term, p_lo)
        if count < 8:
            term = "(%s & 0x%Xu)" % (term, (1 << count) - 1)
        if r_lo > 0:
            term = "(%s << %d)" % (term, r_lo)
        lines.append("%sv |= %s;" % (indent, term))
    t = c_type(s)
    if s.value_type == "integer" and s.signed:
        if s.length < 64:
            sign = 1 << (s.length - 1)
            ext = U64_MAX & ~((1 << s.length) - 1)
            lines.append("%sif ((v & 0x%Xull) != 0ull) {" % (indent, sign))
            lines.append("%s    v |= 0x%Xull;" % (indent, ext))
            lines.append("%s}" % indent)
        lines.append("%sdst_p->%s = (%s)(int64_t)v;" % (indent, field, t))
    else:
        lines.append("%sdst_p->%s = (%s)v;" % (indent, field, t))
    return lines


def c_signal_doc(s):
    order = "1" if s.little_endian else "0"
    sign = "-" if s.signed else "+"
    parts = ["%s: %d|%d@%s%s" % (s.name, s.start, s.length, order, sign)]
    if s.value_type != "integer":
        parts.append(s.value_type)
    parts.append("scale %s, offset %s" % (fmt_f64(s.factor), fmt_f64(s.offset)))
    if not is_unspecified_range(s):
        parts.append("range %s to %s" % (fmt_f64(s.minimum), fmt_f64(s.maximum)))
    if s.unit:
        parts.append("unit %s" % comment_text(s.unit))
    text = ", ".join(parts) + "."
    if s.comment:
        text += " " + comment_text(s.comment)
    return text


def wrap(text, width, prefix):
    words = text.split()
    lines = []
    cur = ""
    for w in words:
        if cur and len(prefix) + len(cur) + 1 + len(w) > width:
            lines.append(prefix + cur)
            cur = w
        elif cur:
            cur = cur + " " + w
        else:
            cur = w
    if cur:
        lines.append(prefix + cur)
    return lines


def gen_c(db, prefix, source_name):
    require_clean(db)
    names = Names(db)
    P = prefix.upper()
    guard = "%s_H" % P
    any_float32 = any(s.value_type == "float32" for m in db.messages for s in m.signals)

    h = []
    h.append("/**")
    h.append(" * %s.h" % prefix)
    h.append(" *")
    h.append(" * Generated by canforge %s from %s. Do not edit." % (VERSION, comment_text(source_name)))
    h.append(" * %s" % PROJECT_URL)
    h.append(" *")
    h.append(" * Each message has a struct of raw signal values and pack/unpack functions")
    h.append(" * that convert it to and from the bytes on the wire. Each signal has")
    h.append(" * decode/encode helpers for physical values and a range check.")
    h.append(" * No heap allocation, no global state, C99.")
    h.append(" */")
    h.append("")
    h.append("#ifndef %s" % guard)
    h.append("#define %s" % guard)
    h.append("")
    h.append("#include <stdbool.h>")
    h.append("#include <stddef.h>")
    h.append("#include <stdint.h>")
    h.append("")
    h.append("#ifdef __cplusplus")
    h.append('extern "C" {')
    h.append("#endif")
    h.append("")
    h.append("/* Frame IDs, lengths and formats. */")
    for mi, m in enumerate(db.messages):
        M = names.msg[mi].upper()
        h.append("#define %s_%s_FRAME_ID (0x%Xu)" % (P, M, m.frame_id))
        h.append("#define %s_%s_LENGTH (%du)" % (P, M, m.dlc))
        h.append("#define %s_%s_IS_EXTENDED (%d)" % (P, M, 1 if m.is_extended else 0))
    choice_lines = []
    for mi, m in enumerate(db.messages):
        M = names.msg[mi].upper()
        for si, s in enumerate(m.signals):
            if s.value_type != "integer":
                continue
            S = names.sig[mi][si].upper()
            for ci, (value, label) in enumerate(s.choices):
                L = names.choice[mi][si][ci]
                if value < 0:
                    lit = "(%d)" % value
                else:
                    lit = "(%du)" % value
                choice_lines.append("#define %s_%s_%s_%s_CHOICE %s" % (P, M, S, L, lit))
    if choice_lines:
        h.append("")
        h.append("/* Value descriptions. */")
        h.extend(choice_lines)

    for mi, m in enumerate(db.messages):
        mname = names.msg[mi]
        typ = "%s_%s_t" % (prefix, mname)
        h.append("")
        h.append("/**")
        h.append(" * %s" % comment_text(m.name))
        h.append(" *")
        fmt = "extended" if m.is_extended else "standard"
        sender = "no transmitter" if m.sender == NO_NODE else "sent by %s" % m.sender
        h.append(" * Frame 0x%X (%s), %d bytes, %s." % (m.frame_id, fmt, m.dlc, sender))
        if m.comment:
            h.extend(wrap(comment_text(m.comment), 78, " * "))
        h.append(" */")
        h.append("typedef struct {")
        if not m.signals:
            h.append("    uint8_t unused; /* This message has no signals. */")
        for si, s in enumerate(m.signals):
            doc = wrap(c_signal_doc(s), 74, "")
            if len(doc) == 1:
                h.append("    /* %s */" % doc[0])
            else:
                h.append("    /*")
                for d in doc:
                    h.append("     * %s" % d)
                h.append("     */")
            h.append("    %s %s;" % (c_type(s), names.sig[mi][si]))
        h.append("} %s;" % typ)
        h.append("")
        h.append("/** Set every signal of %s to raw zero. */" % comment_text(m.name))
        h.append("void %s_%s_init(%s *msg_p);" % (prefix, mname, typ))
        h.append("")
        h.append("/**")
        h.append(" * Pack %s into dst_p, which must hold at least %s_%s_LENGTH bytes." % (comment_text(m.name), P, mname.upper()))
        h.append(" * Returns the number of bytes written, or -1 if size is too small.")
        h.append(" */")
        h.append("int %s_%s_pack(uint8_t *dst_p, const %s *src_p, size_t size);" % (prefix, mname, typ))
        h.append("")
        h.append("/**")
        h.append(" * Unpack %s from src_p. Returns 0 on success, or -1 if size is" % comment_text(m.name))
        h.append(" * too small. Multiplexed signals outside the active group are zero.")
        h.append(" */")
        h.append("int %s_%s_unpack(%s *dst_p, const uint8_t *src_p, size_t size);" % (prefix, mname, typ))
        for si, s in enumerate(m.signals):
            fn = "%s_%s_%s" % (prefix, mname, names.sig[mi][si])
            t = c_type(s)
            h.append("")
            h.append("double %s_decode(%s raw);" % (fn, t))
            h.append("%s %s_encode(double value);" % (t, fn))
            h.append("bool %s_is_in_range(%s raw);" % (fn, t))

    h.append("")
    h.append("#ifdef __cplusplus")
    h.append("}")
    h.append("#endif")
    h.append("")
    h.append("#endif /* %s */" % guard)
    h.append("")

    c = []
    c.append("/**")
    c.append(" * %s.c" % prefix)
    c.append(" *")
    c.append(" * Generated by canforge %s from %s. Do not edit." % (VERSION, comment_text(source_name)))
    c.append(" * %s" % PROJECT_URL)
    c.append(" */")
    c.append("")
    if any_float32:
        c.append("#include <float.h>")
    c.append("#include <string.h>")
    c.append("")
    c.append('#include "%s.h"' % prefix)

    for mi, m in enumerate(db.messages):
        mname = names.msg[mi]
        typ = "%s_%s_t" % (prefix, mname)
        plain, groups, mux_sig = split_groups(m)
        field_of = {}
        for si, s in enumerate(m.signals):
            field_of[id(s)] = names.sig[mi][si]

        c.append("")
        c.append("void %s_%s_init(%s *msg_p)" % (prefix, mname, typ))
        c.append("{")
        c.append("    (void)memset(msg_p, 0, sizeof(*msg_p));")
        c.append("}")

        # pack
        c.append("")
        c.append("int %s_%s_pack(uint8_t *dst_p, const %s *src_p, size_t size)" % (prefix, mname, typ))
        c.append("{")
        if m.signals:
            c.append("    uint64_t v;")
            c.append("")
        if m.dlc > 0:
            c.append("    if (size < %du) {" % m.dlc)
            c.append("        return -1;")
            c.append("    }")
            c.append("")
            c.append("    (void)memset(dst_p, 0, %du);" % m.dlc)
        else:
            c.append("    (void)dst_p;")
            c.append("    (void)size;")
        if not m.signals:
            c.append("    (void)src_p;")
        for s in plain:
            c.append("")
            c.extend(c_pack_lines(s, field_of[id(s)], "    "))
        if groups:
            c.append("")
            c.append("    switch (src_p->%s) {" % field_of[id(mux_sig)])
            for value, sigs in groups:
                c.append("    case %d:" % value)
                for k, s in enumerate(sigs):
                    if k > 0:
                        c.append("")
                    c.extend(c_pack_lines(s, field_of[id(s)], "        "))
                c.append("        break;")
            c.append("    default:")
            c.append("        break;")
            c.append("    }")
        c.append("")
        c.append("    return %d;" % m.dlc)
        c.append("}")

        # unpack
        c.append("")
        c.append("int %s_%s_unpack(%s *dst_p, const uint8_t *src_p, size_t size)" % (prefix, mname, typ))
        c.append("{")
        if m.signals:
            c.append("    uint64_t v;")
            c.append("")
        if m.dlc > 0:
            c.append("    if (size < %du) {" % m.dlc)
            c.append("        return -1;")
            c.append("    }")
            c.append("")
        else:
            c.append("    (void)size;")
        if not m.signals:
            c.append("    (void)src_p;")
        c.append("    (void)memset(dst_p, 0, sizeof(*dst_p));")
        for s in plain:
            c.append("")
            c.extend(c_unpack_lines(s, field_of[id(s)], "    "))
        if groups:
            c.append("")
            c.append("    switch (dst_p->%s) {" % field_of[id(mux_sig)])
            for value, sigs in groups:
                c.append("    case %d:" % value)
                for k, s in enumerate(sigs):
                    if k > 0:
                        c.append("")
                    c.extend(c_unpack_lines(s, field_of[id(s)], "        "))
                c.append("        break;")
            c.append("    default:")
            c.append("        break;")
            c.append("    }")
        c.append("")
        c.append("    return 0;")
        c.append("}")

        for si, s in enumerate(m.signals):
            c.extend(c_signal_functions(s, "%s_%s_%s" % (prefix, mname, names.sig[mi][si])))

    c.append("")
    return "\n".join(h), "\n".join(c)


def c_signal_functions(s, fn):
    t = c_type(s)
    out = []
    # decode
    out.append("")
    out.append("double %s_decode(%s raw)" % (fn, t))
    out.append("{")
    if s.value_type == "float32":
        out.append("    float f;")
        out.append("")
        out.append("    (void)memcpy(&f, &raw, sizeof(f));")
        out.append("    return %s;" % scale_expr("(double)f", s))
    elif s.value_type == "float64":
        out.append("    double f;")
        out.append("")
        out.append("    (void)memcpy(&f, &raw, sizeof(f));")
        out.append("    return %s;" % scale_expr("f", s))
    else:
        out.append("    return %s;" % scale_expr("(double)raw", s))
    out.append("}")

    # encode
    out.append("")
    out.append("%s %s_encode(double value)" % (t, fn))
    out.append("{")
    if s.value_type == "float32":
        out.append("    double r = %s;" % unscale_expr(s))
        out.append("    float f;")
        out.append("    uint32_t raw;")
        out.append("")
        out.append("    if (r > (double)FLT_MAX) {")
        out.append("        r = (double)FLT_MAX;")
        out.append("    }")
        out.append("    if (r < -(double)FLT_MAX) {")
        out.append("        r = -(double)FLT_MAX;")
        out.append("    }")
        out.append("    f = (float)r;")
        out.append("    (void)memcpy(&raw, &f, sizeof(raw));")
        out.append("    return raw;")
    elif s.value_type == "float64":
        out.append("    double f = %s;" % unscale_expr(s))
        out.append("    uint64_t raw;")
        out.append("")
        out.append("    (void)memcpy(&raw, &f, sizeof(raw));")
        out.append("    return raw;")
    else:
        rmin, rmax = raw_range(s.length, s.signed)
        lo_f = fmt_f64(f64_at_least(rmin))
        hi_f = fmt_f64(f64_at_most(rmax))
        out.append("    double r = %s;" % unscale_expr(s))
        out.append("")
        out.append("    if (!(r >= %s)) {" % lo_f)
        out.append("        r = %s;" % lo_f)
        out.append("    }")
        out.append("    if (r > %s) {" % hi_f)
        out.append("        r = %s;" % hi_f)
        out.append("    }")
        if s.signed:
            out.append("    return (%s)((r >= 0.0) ? (r + 0.5) : (r - 0.5));" % t)
        else:
            out.append("    return (%s)(r + 0.5);" % t)
    out.append("}")

    # range check
    out.append("")
    out.append("bool %s_is_in_range(%s raw)" % (fn, t))
    out.append("{")
    if is_unspecified_range(s):
        out.append("    (void)raw;")
        out.append("    return true;")
    elif s.value_type != "integer":
        out.append("    double value = %s_decode(raw);" % fn)
        out.append("")
        out.append("    return (value >= %s) && (value <= %s);" % (fmt_f64(s.minimum), fmt_f64(s.maximum)))
    else:
        lo, hi = checked_raw_bounds(s)
        tmin, tmax = type_range(s)
        conds = []
        if s.signed:
            if lo > tmin:
                conds.append("(int64_t)raw >= %sLL" % lo)
            if hi < tmax:
                conds.append("(int64_t)raw <= %sLL" % hi)
        else:
            if lo > tmin:
                conds.append("(uint64_t)raw >= %dull" % lo)
            if hi < tmax:
                conds.append("(uint64_t)raw <= %dull" % hi)
        if lo > hi:
            out.append("    (void)raw;")
            out.append("    return false;")
        elif not conds:
            out.append("    (void)raw;")
            out.append("    return true;")
        elif len(conds) == 1:
            out.append("    return (%s);" % conds[0])
        else:
            out.append("    return (%s) && (%s);" % (conds[0], conds[1]))
    out.append("}")
    return out


# ---------------------------------------------------------------------------
# Python code generation
# ---------------------------------------------------------------------------


def py_str(s):
    return json.dumps(s, ensure_ascii=True)


def py_unpack_lines(s, indent):
    lines = []
    lines.append("%sv = 0" % indent)
    for byte, p_lo, r_lo, count in segments(s):
        term = "data[%d]" % byte
        if p_lo > 0:
            term = "(%s >> %d)" % (term, p_lo)
        if count < 8:
            term = "(%s & 0x%X)" % (term, (1 << count) - 1)
        if r_lo > 0:
            term = "(%s << %d)" % (term, r_lo)
        lines.append("%sv |= %s" % (indent, term))
    if s.value_type == "integer" and s.signed:
        lines.append("%sif v & 0x%X:" % (indent, 1 << (s.length - 1)))
        lines.append("%s    v -= 0x%X" % (indent, 1 << s.length))
    return lines


def py_value_expr(s):
    if s.value_type == "float32":
        base = 'struct.unpack("<f", v.to_bytes(4, "little"))[0]'
    elif s.value_type == "float64":
        base = 'struct.unpack("<d", v.to_bytes(8, "little"))[0]'
    elif s.factor == 1.0:
        base = "float(v)"
    else:
        base = "v"
    return scale_expr(base, s)


def gen_py(db, prefix, source_name):
    require_clean(db)
    names = Names(db)
    out = []
    out.append('"""Decoders for %s.' % comment_text(source_name).replace('"', "'"))
    out.append("")
    out.append("Generated by canforge %s. Do not edit." % VERSION)
    out.append(PROJECT_URL)
    out.append("")
    out.append("Each decode_* function takes the frame payload as bytes and returns a")
    out.append("dict of physical values keyed by signal name. Multiplexed signals are")
    out.append("only present when their multiplexer selects them.")
    out.append('"""')
    out.append("")
    out.append("import struct")
    out.append("")
    out.append("")
    out.append("class DecodeError(ValueError):")
    out.append('    """Raised when a frame cannot be decoded."""')

    entries = []
    for mi, m in enumerate(db.messages):
        fname = "decode_%s" % names.msg[mi]
        entries.append((m, fname))
        plain, groups, mux_sig = split_groups(m)
        out.append("")
        out.append("")
        out.append("def %s(data):" % fname)
        fmt = "extended" if m.is_extended else "standard"
        out.append('    """Decode %s (frame 0x%X, %s, %d bytes)."""' % (comment_text(m.name), m.frame_id, fmt, m.dlc))
        if m.dlc > 0:
            out.append("    if len(data) < %d:" % m.dlc)
            out.append('        raise DecodeError("%s needs %d bytes, got %%d" %% len(data))' % (m.name, m.dlc))
        out.append("    values = {}")
        for s in plain:
            out.extend(py_unpack_lines(s, "    "))
            if s is mux_sig:
                out.append("    mux = v")
            out.append("    values[%s] = %s" % (py_str(s.name), py_value_expr(s)))
        for gi, (value, sigs) in enumerate(groups):
            out.append("    %s mux == %d:" % ("if" if gi == 0 else "elif", value))
            for s in sigs:
                out.extend(py_unpack_lines(s, "        "))
                out.append("        values[%s] = %s" % (py_str(s.name), py_value_expr(s)))
        out.append("    return values")

    out.append("")
    out.append("")
    out.append("DECODERS = {")
    for m, fname in entries:
        out.append("    (0x%X, %s): %s," % (m.frame_id, "True" if m.is_extended else "False", fname))
    out.append("}")
    out.append("")
    out.append("MESSAGE_NAMES = {")
    for m, _ in entries:
        out.append("    (0x%X, %s): %s," % (m.frame_id, "True" if m.is_extended else "False", py_str(m.name)))
    out.append("}")
    out.append("")
    out.append("")
    out.append("def decode(frame_id, data, is_extended=False):")
    out.append('    """Decode a frame by ID. Raises DecodeError for unknown frames."""')
    out.append("    fn = DECODERS.get((frame_id, is_extended))")
    out.append("    if fn is None:")
    out.append('        raise DecodeError("unknown frame ID 0x%X" % frame_id)')
    out.append("    return fn(bytes(data))")
    out.append("")
    return "\n".join(out)


# ---------------------------------------------------------------------------
# Diff
# ---------------------------------------------------------------------------


def change(level, kind, message, msg_name="", sig_name=""):
    return {"level": level, "kind": kind, "message": message, "message_name": msg_name, "signal_name": sig_name}


def same_layout(a, b):
    return (a.start == b.start and a.length == b.length and a.little_endian == b.little_endian
            and a.signed == b.signed and a.value_type == b.value_type and a.factor == b.factor
            and a.offset == b.offset and a.mux == b.mux)


def describe_order(s):
    return "Intel" if s.little_endian else "Motorola"


def mux_label(m):
    if m is None:
        return "none"
    if m == MUX_SWITCH:
        return "multiplexer"
    return "m%d" % m


def compare_signals(m_name, a, b, out):
    q = "%s.%s" % (m_name, a.name)
    layout = []
    if a.start != b.start:
        layout.append("start bit %d -> %d" % (a.start, b.start))
    if a.length != b.length:
        layout.append("length %d -> %d" % (a.length, b.length))
    if a.little_endian != b.little_endian:
        layout.append("byte order %s -> %s" % (describe_order(a), describe_order(b)))
    if a.signed != b.signed:
        layout.append("%s -> %s" % ("signed" if a.signed else "unsigned", "signed" if b.signed else "unsigned"))
    if a.value_type != b.value_type:
        layout.append("type %s -> %s" % (a.value_type, b.value_type))
    if layout:
        out.append(change("breaking", "signal-layout-changed", "%s layout changed: %s" % (q, ", ".join(layout)), m_name, a.name))
    scaling = []
    if a.factor != b.factor:
        scaling.append("factor %s -> %s" % (fmt_f64(a.factor), fmt_f64(b.factor)))
    if a.offset != b.offset:
        scaling.append("offset %s -> %s" % (fmt_f64(a.offset), fmt_f64(b.offset)))
    if scaling:
        out.append(change("breaking", "signal-scaling-changed", "%s scaling changed: %s" % (q, ", ".join(scaling)), m_name, a.name))
    if a.mux != b.mux:
        out.append(change("breaking", "signal-multiplexing-changed", "%s multiplexing changed: %s -> %s" % (q, mux_label(a.mux), mux_label(b.mux)), m_name, a.name))
    if a.unit != b.unit:
        out.append(change("caution", "signal-unit-changed", "%s unit changed: \"%s\" -> \"%s\"" % (q, a.unit, b.unit), m_name, a.name))
    if a.minimum != b.minimum or a.maximum != b.maximum:
        narrowed = b.minimum > a.minimum or b.maximum < a.maximum
        rng = "[%s|%s] -> [%s|%s]" % (fmt_f64(a.minimum), fmt_f64(a.maximum), fmt_f64(b.minimum), fmt_f64(b.maximum))
        if narrowed:
            out.append(change("caution", "signal-range-narrowed", "%s range narrowed: %s" % (q, rng), m_name, a.name))
        else:
            out.append(change("compatible", "signal-range-widened", "%s range widened: %s" % (q, rng), m_name, a.name))
    if a.choices != b.choices:
        old_map = dict(a.choices)
        new_map = dict(b.choices)
        lost = [v for v in old_map if v not in new_map or new_map[v] != old_map[v]]
        if lost:
            out.append(change("caution", "signal-choices-changed", "%s value descriptions changed or removed for %s" % (q, ", ".join(str(v) for v in lost)), m_name, a.name))
        else:
            added = [v for v in new_map if v not in old_map]
            out.append(change("compatible", "signal-choices-added", "%s value descriptions added for %s" % (q, ", ".join(str(v) for v in added)), m_name, a.name))
    if a.receivers != b.receivers:
        out.append(change("caution", "signal-receivers-changed", "%s receivers changed: %s -> %s" % (q, ",".join(a.receivers) or "none", ",".join(b.receivers) or "none"), m_name, a.name))
    if a.comment != b.comment:
        out.append(change("compatible", "comment-changed", "%s comment changed" % q, m_name, a.name))


def compare_messages(a, b, out):
    name = b.name
    if b.dlc < a.dlc:
        out.append(change("breaking", "frame-length-reduced", "%s length reduced from %d to %d bytes" % (name, a.dlc, b.dlc), name))
    elif b.dlc > a.dlc:
        out.append(change("caution", "frame-length-increased", "%s length increased from %d to %d bytes" % (name, a.dlc, b.dlc), name))
    if a.sender != b.sender:
        out.append(change("caution", "transmitter-changed", "%s transmitter changed: %s -> %s" % (name, a.sender, b.sender), name))
    old_names = [s.name for s in a.signals]
    new_names = [s.name for s in b.signals]
    removed = [s for s in a.signals if s.name not in new_names]
    added = [s for s in b.signals if s.name not in old_names]
    renamed_new = set()
    for s in a.signals:
        if s.name in new_names:
            t = find_signal(b, s.name)
            compare_signals(name, s, t, out)
    for s in removed:
        target = None
        for t in added:
            if t.name not in renamed_new and same_layout(s, t):
                target = t
                break
        if target is not None:
            renamed_new.add(target.name)
            out.append(change("caution", "signal-renamed", "%s.%s renamed to %s" % (name, s.name, target.name), name, s.name))
        else:
            out.append(change("breaking", "signal-removed", "%s.%s removed" % (name, s.name), name, s.name))
    for t in added:
        if t.name not in renamed_new:
            out.append(change("compatible", "signal-added", "%s.%s added at %d|%d@%s%s" % (name, t.name, t.start, t.length, "1" if t.little_endian else "0", "-" if t.signed else "+"), name, t.name))
    if a.comment != b.comment:
        out.append(change("compatible", "comment-changed", "%s comment changed" % name, name))


def diff(old, new):
    out = []
    old_keys = [(m.frame_id, m.is_extended) for m in old.messages]
    matched = []
    for om in old.messages:
        key = (om.frame_id, om.is_extended)
        nm = None
        for m in new.messages:
            if (m.frame_id, m.is_extended) == key:
                nm = m
                break
        if nm is None:
            moved = None
            for m in new.messages:
                if m.name == om.name and (m.frame_id, m.is_extended) not in old_keys:
                    moved = m
                    break
            if moved is not None:
                matched.append((moved.frame_id, moved.is_extended))
                out.append(change("breaking", "frame-id-changed", "%s frame ID changed from %s to %s" % (om.name, fmt_id(om), fmt_id(moved)), om.name))
                compare_messages(om, moved, out)
            else:
                out.append(change("breaking", "message-removed", "%s (%s) removed" % (om.name, fmt_id(om)), om.name))
            continue
        matched.append(key)
        if nm.name != om.name:
            out.append(change("caution", "message-renamed", "%s renamed to %s" % (om.name, nm.name), nm.name))
        compare_messages(om, nm, out)
    for nm in new.messages:
        if (nm.frame_id, nm.is_extended) not in matched:
            out.append(change("compatible", "message-added", "%s (%s) added" % (nm.name, fmt_id(nm)), nm.name))
    return out


def verdict(changes):
    levels = [c["level"] for c in changes]
    if "breaking" in levels:
        return "breaking"
    if "caution" in levels:
        return "caution"
    if levels:
        return "compatible"
    return "identical"


# ---------------------------------------------------------------------------
# CLI used by CI for differential testing
# ---------------------------------------------------------------------------


def decode_json(db, frame_id, data):
    try:
        r = decode(db, frame_id, data)
    except DecodeError as e:
        return {"ok": False, "error": str(e)}
    r["ok"] = True
    return r


def main(argv):
    if len(argv) < 2:
        print("usage: canforge_ref.py lint|decode|gen-c|gen-py|diff ...", file=sys.stderr)
        return 2
    cmd = argv[1]
    try:
        if cmd == "lint":
            print(json.dumps(lint(parse_file(argv[2])), indent=1))
        elif cmd == "decode":
            db = parse_file(argv[2])
            fid = int(argv[3], 0)
            data = bytes.fromhex(argv[4])
            print(json.dumps(decode_json(db, fid, data)))
        elif cmd == "gen-c":
            db = parse_file(argv[2])
            prefix, out_dir = argv[3], argv[4]
            name = argv[2].replace("\\", "/").split("/")[-1]
            hdr, src = gen_c(db, prefix, name)
            with open("%s/%s.h" % (out_dir, prefix), "w") as f:
                f.write(hdr)
            with open("%s/%s.c" % (out_dir, prefix), "w") as f:
                f.write(src)
        elif cmd == "gen-py":
            db = parse_file(argv[2])
            prefix, out_dir = argv[3], argv[4]
            name = argv[2].replace("\\", "/").split("/")[-1]
            with open("%s/%s.py" % (out_dir, prefix), "w") as f:
                f.write(gen_py(db, prefix, name))
        elif cmd == "diff":
            ch = diff(parse_file(argv[2]), parse_file(argv[3]))
            print(json.dumps({"verdict": verdict(ch), "changes": ch}, indent=1))
        else:
            print("unknown command %s" % cmd, file=sys.stderr)
            return 2
    except DbcError as e:
        print("error: %s" % e, file=sys.stderr)
        return 1
    except CodegenError as e:
        print("error: %s" % e, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
