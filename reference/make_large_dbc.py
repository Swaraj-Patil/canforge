#!/usr/bin/env python3
"""Write a large synthetic DBC file for performance checks.

    python3 reference/make_large_dbc.py [out.dbc]      (default: build/large.dbc)

The output is deterministic: 500 messages and 5,000 signals with the mix a
real vehicle database has, namely Intel and Motorola byte order, signed,
scaled and float32 signals, 64-bit fields, 11-bit and 29-bit frame IDs,
64-byte CAN FD frames, multiplexed messages, and comments, value
descriptions and attributes. Before writing, the reference model parses the
text and must find no lint errors or warnings, so the file is also a valid
input for code generation.

CI generates this file for its timing checks. It is not committed.
"""

import os
import random
import sys

import canforge_ref as ref

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
DEFAULT_OUT = os.path.join(ROOT, "build", "large.dbc")

SEED = 5000
MESSAGES = 500
SIGNALS = 5000
FD_MESSAGES = 20  # 64-byte CAN FD frames
FD_SIGNALS = 40
MUX_MESSAGES = 30  # 8-byte frames with a multiplexer and pages of signals
MUX_PAGES = ["Speeds", "Temperatures", "Voltages", "Faults"]
MUX_PAGE_SIGNALS = 4
EXTENDED_MESSAGES = 100  # 29-bit IDs; the rest are 11-bit

NODES = ["VCU", "BMS", "INV", "OBC", "BCM", "ESC", "EPS", "GW", "TCU", "ADAS"]
SYSTEMS = [
    "Engine", "Brake", "Steering", "Battery", "Inverter", "Charger", "Body", "Door", "Seat", "Light",
    "Climate", "Wiper", "Airbag", "Tire", "Chassis", "Gearbox", "Motor", "Pump", "Valve", "Cabin",
]
KINDS = ["Status", "Command", "Limits", "Feedback", "Request", "Diag", "Temps", "Faults", "Config", "Data"]
QUANTITIES = [
    ("Speed", "km/h"), ("Torque", "Nm"), ("Temp", "degC"), ("Pressure", "bar"), ("Current", "A"),
    ("Voltage", "V"), ("Position", "mm"), ("Angle", "deg"), ("Level", "%"), ("Flow", "L/min"),
    ("Rate", "deg/s"), ("Mode", ""), ("Counter", ""), ("Fault", ""), ("Limit", "A"),
    ("Target", ""), ("Demand", "%"), ("Power", "kW"), ("Energy", "kWh"), ("Duty", "%"),
]
PLACES = [
    "", "FL", "FR", "RL", "RR", "Left", "Right", "Front", "Rear", "Main",
    "Aux", "Inner", "Outer", "Upper", "Lower", "One", "Two", "Three", "Four", "Five",
]
SCALINGS = [
    (1, 0), (1, 0), (1, 0), (0.1, 0), (0.01, 0), (0.5, 0), (0.25, 0),
    (0.05, 0), (2, 0), (1, -40), (0.1, -40), (0.001, 0), (0.2, -100),
]
LENGTHS = [1, 1, 1, 2, 2, 2, 3, 4, 4, 8, 8, 8, 10, 12, 12, 16, 16, 16]
LABELS = [
    "Off", "On", "Error", "Not available", "Init", "Ready", "Active", "Fault",
    "Standby", "Reset", "Locked", "Unlocked", "Low", "High", "Idle", "Busy",
]
CYCLE_TIMES = [10, 20, 50, 100, 100, 200, 500, 1000]


def num(x):
    """A DBC number: shortest round-trip digits, without a trailing .0."""
    s = ref.fmt_f64(float(x))
    return s[:-2] if s.endswith(".0") else s


def decimals(x):
    s = num(x)
    return len(s.split(".")[1]) if "." in s else 0


def physical_bounds(rmin, rmax, factor, offset):
    """Physical values of two raw bounds, computed the way lint W001 does and
    rounded to the precision of the scaling, as a DBC author would write them.
    The rounding stays far inside W001's tolerance."""
    a = float(rmin) * factor + offset
    b = float(rmax) * factor + offset
    places = max(decimals(factor), decimals(offset))
    return round(min(a, b), places), round(max(a, b), places)


def start_bit(linear, little_endian):
    """The DBC start bit of a signal whose first bit sits at a linear position.

    Intel signals run upward through absolute bits, so the linear position is
    the start bit. Motorola signals run from the MSB at bit 7 of a byte down
    to bit 0 and on into the next byte, so linear position byte * 8 + k is
    bit 7 - k of that byte.
    """
    if little_endian:
        return linear
    return (linear // 8) * 8 + (7 - linear % 8)


def fit(lengths, budget):
    """Halve the longest signals until the lengths fit in budget bits."""
    lengths = list(lengths)
    while sum(lengths) > budget:
        i = max(range(len(lengths)), key=lambda k: lengths[k])
        lengths[i] = max(1, lengths[i] // 2)
    return lengths


def place(rng, lengths, first, last, little_endian):
    """Start bits for signals laid end to end between two linear positions,
    with the spare bits spread randomly between them."""
    spare = last - first - sum(lengths)
    gaps = [0] * (len(lengths) + 1)
    for _ in range(spare):
        gaps[rng.randrange(len(gaps))] += 1
    starts = []
    cursor = first
    for length, gap in zip(lengths, gaps):
        cursor += gap
        starts.append(start_bit(cursor, little_endian))
        cursor += length
    return starts


class Names:
    """Signal names that stay unique, also as C identifiers, within a message."""

    def __init__(self, rng):
        self.pool = [(q, p) for q in QUANTITIES for p in PLACES]
        rng.shuffle(self.pool)

    def take(self):
        (quantity, unit), place_name = self.pool.pop()
        return quantity + place_name, unit


def integer_signal(rng, name, unit, length, start, little_endian, sender):
    s = {
        "name": name, "start": start, "length": length, "little": little_endian,
        "signed": False, "factor": 1, "offset": 0, "unit": unit if length > 3 else "", "float32": False,
        "choices": [], "mux": "",
        "receivers": rng.sample([n for n in NODES if n != sender], rng.randint(1, 3)),
    }
    if length <= 4 and rng.random() < 0.5:
        count = min(1 << length, rng.randint(2, 6))
        s["choices"] = list(zip(range(count), rng.sample(LABELS, count)))
        s["unit"] = ""
    elif length >= 4:
        s["signed"] = rng.random() < 0.3
        s["factor"], s["offset"] = rng.choice(SCALINGS)
    rmin, rmax = ref.raw_range(length, s["signed"])
    pick = rng.random()
    if pick < 0.1:
        s["minimum"], s["maximum"] = 0.0, 0.0  # range not specified
        return s
    if pick < 0.2 and length > 2:
        # A declared range narrower than the bits allow.
        quarter = (rmax - rmin) // 4
        rmin, rmax = rmin + quarter, rmax - quarter
    s["minimum"], s["maximum"] = physical_bounds(rmin, rmax, s["factor"], s["offset"])
    return s


def float_signal(rng, name, unit, start, little_endian, sender):
    s = integer_signal(rng, name, unit, 32, start, little_endian, sender)
    s.update({"float32": True, "signed": True, "factor": 1, "offset": 0, "choices": [], "unit": unit})
    s["minimum"], s["maximum"] = (-1000.0, 1000.0) if rng.random() < 0.5 else (0.0, 0.0)
    return s


def wide_signal(rng, name, start, little_endian, sender, signed):
    s = integer_signal(rng, name, "", 64, start, little_endian, sender)
    s.update({"signed": signed, "factor": 1, "offset": 0, "choices": [], "minimum": 0.0, "maximum": 0.0})
    return s


def classic_signals(rng, count, little_endian, sender):
    names = Names(rng)
    lengths = fit([rng.choice(LENGTHS) for _ in range(count)], 64)
    out = []
    for length, start in zip(lengths, place(rng, lengths, 0, 64, little_endian)):
        name, unit = names.take()
        out.append(integer_signal(rng, name, unit, length, start, little_endian, sender))
    return out


def mux_signals(rng, little_endian, sender):
    names = Names(rng)
    mux = integer_signal(rng, "Page", "", 8, start_bit(0, little_endian), little_endian, sender)
    mux.update({"signed": False, "factor": 1, "offset": 0, "minimum": 0.0, "maximum": float(len(MUX_PAGES) - 1)})
    mux["choices"] = list(enumerate(MUX_PAGES))
    mux["mux"] = " M"
    out = [mux]
    for page in range(len(MUX_PAGES)):
        lengths = fit([rng.choice(LENGTHS) for _ in range(MUX_PAGE_SIGNALS)], 56)
        for length, start in zip(lengths, place(rng, lengths, 8, 64, little_endian)):
            name, unit = names.take()
            s = integer_signal(rng, name, unit, length, start, little_endian, sender)
            s["mux"] = " m%d" % page
            out.append(s)
    return out


def fd_signals(rng, little_endian, sender, index):
    names = Names(rng)
    kinds = ["float", "float", "wide"] + ["int"] * (FD_SIGNALS - 3)
    rng.shuffle(kinds)
    flexible = fit([rng.choice(LENGTHS + [32]) for k in kinds if k == "int"], 512 - 2 * 32 - 64)
    lengths = []
    for k in kinds:
        lengths.append(32 if k == "float" else 64 if k == "wide" else flexible.pop())
    out = []
    for k, length, start in zip(kinds, lengths, place(rng, lengths, 0, 512, little_endian)):
        name, unit = names.take()
        if k == "float":
            out.append(float_signal(rng, name, unit, start, little_endian, sender))
        elif k == "wide":
            out.append(wide_signal(rng, name, start, little_endian, sender, signed=index % 2 == 1))
        else:
            out.append(integer_signal(rng, name, unit, length, start, little_endian, sender))
    return out


def plan_messages(rng):
    kinds = ["fd"] * FD_MESSAGES + ["mux"] * MUX_MESSAGES
    kinds += ["classic"] * (MESSAGES - len(kinds))
    rng.shuffle(kinds)
    classic = kinds.count("classic")
    target = SIGNALS - FD_MESSAGES * FD_SIGNALS - MUX_MESSAGES * (1 + len(MUX_PAGES) * MUX_PAGE_SIGNALS)
    counts = [rng.randint(5, 11) for _ in range(classic)]
    while sum(counts) != target:
        i = rng.randrange(classic)
        if sum(counts) < target and counts[i] < 12:
            counts[i] += 1
        elif sum(counts) > target and counts[i] > 4:
            counts[i] -= 1
    extended = [True] * EXTENDED_MESSAGES + [False] * (MESSAGES - EXTENDED_MESSAGES)
    rng.shuffle(extended)
    standard_ids = sorted(rng.sample(range(0x020, 0x7F0), MESSAGES - EXTENDED_MESSAGES))
    plans = [{"kind": kind, "extended": ext} for kind, ext in zip(kinds, extended)]
    std = iter(standard_ids)
    ext = iter(range(EXTENDED_MESSAGES))
    count = iter(counts)
    for i, p in enumerate(plans):
        if p["extended"]:
            p["raw_id"] = 0x80000000 | (0x18F00010 + next(ext) * 0x100)
        else:
            p["raw_id"] = next(std)
        p["name"] = "%s%s%03d" % (rng.choice(SYSTEMS), rng.choice(KINDS), i)
        p["sender"] = rng.choice(NODES)
        p["little"] = rng.random() < 0.6
        if p["kind"] == "classic":
            p["count"] = next(count)
    return plans


def generate():
    rng = random.Random(SEED)
    plans = plan_messages(rng)
    out = ['VERSION "canforge synthetic"', "", "", "NS_ :"]
    for kw in ["NS_DESC_", "CM_", "BA_DEF_", "BA_", "VAL_", "CAT_DEF_", "CAT_", "FILTER", "BA_DEF_DEF_",
               "EV_DATA_", "ENVVAR_DATA_", "SGTYPE_", "SGTYPE_VAL_", "BA_DEF_SGTYPE_", "BA_SGTYPE_",
               "SIG_TYPE_REF_", "VAL_TABLE_", "SIG_GROUP_", "SIG_VALTYPE_", "SIGTYPE_VALTYPE_",
               "BO_TX_BU_", "BA_DEF_REL_", "BA_REL_", "BA_DEF_DEF_REL_", "BU_SG_REL_", "BU_EV_REL_",
               "BU_BO_REL_", "SG_MUL_VAL_"]:
        out.append("\t" + kw)
    out += ["", "BS_:", "", "BU_: " + " ".join(NODES), ""]
    out.append('VAL_TABLE_ OnOff 1 "On" 0 "Off" ;')
    out.append('VAL_TABLE_ Readiness 3 "Fault" 2 "Active" 1 "Ready" 0 "Init" ;')
    out += ["", ""]

    comments = ['CM_ "Synthetic database for canforge performance checks: %d messages, %d signals.";'
                % (MESSAGES, SIGNALS)]
    for node in NODES:
        comments.append('CM_ BU_ %s "Synthetic node %s.";' % (node, node))
    attributes = []
    choices = []
    valtypes = []
    for i, p in enumerate(plans):
        if p["kind"] == "fd":
            dlc, signals = 64, fd_signals(rng, p["little"], p["sender"], i)
        elif p["kind"] == "mux":
            dlc, signals = 8, mux_signals(rng, p["little"], p["sender"])
        else:
            dlc, signals = 8, classic_signals(rng, p["count"], p["little"], p["sender"])
        raw_id = p["raw_id"]
        out.append("BO_ %d %s: %d %s" % (raw_id, p["name"], dlc, p["sender"]))
        for s in signals:
            out.append(' SG_ %s%s : %d|%d@%d%s (%s,%s) [%s|%s] "%s"  %s' % (
                s["name"], s["mux"], s["start"], s["length"], 1 if s["little"] else 0,
                "-" if s["signed"] else "+", num(s["factor"]), num(s["offset"]),
                num(s["minimum"]), num(s["maximum"]), s["unit"], ",".join(s["receivers"])))
            if s["choices"]:
                pairs = " ".join('%d "%s"' % (v, label) for v, label in reversed(s["choices"]))
                choices.append("VAL_ %d %s %s ;" % (raw_id, s["name"], pairs))
            if s["float32"]:
                valtypes.append("SIG_VALTYPE_ %d %s : 1;" % (raw_id, s["name"]))
            pick = rng.random()
            if pick < 0.02:
                comments.append('CM_ SG_ %d %s "Synthetic signal %s.\nA second line, as some tools write.";'
                                % (raw_id, s["name"], s["name"]))
            elif pick < 0.1:
                comments.append('CM_ SG_ %d %s "Synthetic signal %s."' % (raw_id, s["name"], s["name"]) + ";")
            if rng.random() < 0.2:
                attributes.append('BA_ "GenSigStartValue" SG_ %d %s 0;' % (raw_id, s["name"]))
        out.append("")
        if rng.random() < 0.25:
            comments.append('CM_ BO_ %d "Synthetic message %d for performance checks.";' % (raw_id, i))
        attributes.append('BA_ "GenMsgCycleTime" BO_ %d %d;' % (raw_id, rng.choice(CYCLE_TIMES)))

    out.append("")
    out += comments
    out.append('BA_DEF_ BO_  "GenMsgCycleTime" INT 0 10000;')
    out.append('BA_DEF_ SG_  "GenSigStartValue" INT 0 100000;')
    out.append('BA_DEF_  "BusType" STRING ;')
    out.append('BA_DEF_DEF_  "GenMsgCycleTime" 100;')
    out.append('BA_DEF_DEF_  "GenSigStartValue" 0;')
    out.append('BA_DEF_DEF_  "BusType" "CAN FD";')
    out.append('BA_ "BusType" "CAN FD";')
    out += attributes
    out += choices
    out += valtypes
    return "\n".join(out) + "\n"


def check(text):
    """Parse the text with the reference model and require a clean result.

    Returns the parsed database. Raises ValueError on wrong counts or on any
    lint finding other than the CAN FD note (I002).
    """
    db = ref.parse(text)
    signals = sum(len(m.signals) for m in db.messages)
    if len(db.messages) != MESSAGES or signals != SIGNALS:
        raise ValueError("expected %d messages and %d signals, got %d and %d"
                         % (MESSAGES, SIGNALS, len(db.messages), signals))
    findings = [d for d in ref.lint(db) if d["rule"] != "I002"]
    if findings:
        shown = "; ".join("%s %s" % (d["rule"], d["message"]) for d in findings[:5])
        raise ValueError("%d lint findings, for example: %s" % (len(findings), shown))
    return db


def main(argv):
    path = argv[1] if len(argv) > 1 else DEFAULT_OUT
    text = generate()
    try:
        check(text)
    except ValueError as e:
        print("make_large_dbc.py: the generated database is not clean: %s" % e, file=sys.stderr)
        return 1
    folder = os.path.dirname(os.path.abspath(path))
    os.makedirs(folder, exist_ok=True)
    with open(path, "w", newline="\n") as f:
        f.write(text)
    print("wrote %s: %d messages, %d signals, %d bytes" % (path, MESSAGES, SIGNALS, len(text.encode("utf-8"))))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
