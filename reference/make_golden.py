#!/usr/bin/env python3
"""Regenerate tests/golden/ from the reference model.

The Rust test suite reads these files and fails if the Rust implementation
produces anything different. CI runs this script and then checks that git
reports no changes, so the reference model and the goldens cannot drift.

Formats are tab-separated text so the Rust tests need no parsing library.
Physical values are stored as IEEE-754 bit patterns, so comparisons are
exact rather than approximate.
"""

import os
import random
import struct

import canforge_ref as ref
from harness import frame_case

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
GOLDEN = os.path.join(ROOT, "tests", "golden")
EXAMPLE = os.path.join(ROOT, "examples", "powertrain.dbc")
LINT_DIR = os.path.join(ROOT, "tests", "fixtures", "lint")
DIFF_DIR = os.path.join(ROOT, "tests", "fixtures", "diff")


def bits(x):
    return "%016x" % struct.unpack("<Q", struct.pack("<d", x))[0]


def clean(text):
    return text.replace("\t", " ").replace("\r", " ").replace("\n", " ")


def write(name, text):
    with open(os.path.join(GOLDEN, name), "w", newline="\n") as f:
        f.write(text)


def main():
    os.makedirs(GOLDEN, exist_ok=True)
    db = ref.parse_file(EXAMPLE)

    header, source = ref.gen_c(db, "powertrain", "powertrain.dbc")
    write("powertrain.h", header)
    write("powertrain.c", source)
    write("powertrain.py", ref.gen_py(db, "powertrain", "powertrain.dbc"))

    # Decode vectors: frame_id, extended, data, mux, signal, raw, physical bits, label
    rng = random.Random(4242)
    lines = ["# index\tframe_id\textended\tdata\tmux\tsignal\traw\tphysical_bits\tlabel"]
    index = 0
    for m in db.messages:
        plain, groups, mux_sig = ref.split_groups(m)
        choices = [g[0] for g in groups]
        for k in range(40):
            mux_choice = None
            if mux_sig is not None:
                lo, hi = ref.raw_range(mux_sig.length, mux_sig.signed)
                mux_choice = choices[k % len(choices)] if choices and k % 5 != 4 else rng.randint(lo, hi)
            _, _, data = frame_case(rng, m, mux_choice)
            r = ref.decode(db, m.frame_id, data, m.is_extended)
            mux = r["mux"] if r["mux"] is not None else "-"
            for s in r["signals"]:
                label = clean(s["label"]) if s["label"] is not None else "-"
                lines.append("%d\t%x\t%d\t%s\t%s\t%s\t%s\t%s\t%s" % (
                    index, m.frame_id, 1 if m.is_extended else 0, data.hex(), mux,
                    s["name"], s["raw"], bits(s["physical"]), label))
            index += 1
    write("decode_vectors.tsv", "\n".join(lines) + "\n")

    # Lint output for every fixture and the example: file, rule, severity, line, message
    lines = ["# file\trule\tseverity\tline\tmessage"]
    files = sorted(f for f in os.listdir(LINT_DIR) if f.endswith(".dbc"))
    targets = [("lint/" + f, os.path.join(LINT_DIR, f)) for f in files]
    targets.append(("examples/powertrain.dbc", EXAMPLE))
    for label, path in targets:
        for d in ref.lint(ref.parse_file(path)):
            lines.append("%s\t%s\t%s\t%d\t%s" % (label, d["rule"], d["severity"], d["line"], clean(d["message"])))
    write("lint.tsv", "\n".join(lines) + "\n")

    # Diff of the fixture pair: level, kind, message
    old = ref.parse_file(os.path.join(DIFF_DIR, "v1.dbc"))
    new = ref.parse_file(os.path.join(DIFF_DIR, "v2.dbc"))
    changes = ref.diff(old, new)
    lines = ["# verdict\t%s" % ref.verdict(changes)]
    for c in changes:
        lines.append("%s\t%s\t%s" % (c["level"], c["kind"], clean(c["message"])))
    write("diff.tsv", "\n".join(lines) + "\n")
    print("golden files written to %s" % GOLDEN)


if __name__ == "__main__":
    main()
