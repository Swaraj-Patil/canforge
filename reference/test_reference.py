#!/usr/bin/env python3
"""Tests for the canforge reference model. Run: python3 -m unittest -v"""

import os
import random
import unittest

import canforge_ref as ref
import make_large_dbc

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
LINT_DIR = os.path.join(ROOT, "tests", "fixtures", "lint")
DIFF_DIR = os.path.join(ROOT, "tests", "fixtures", "diff")
EXAMPLE = os.path.join(ROOT, "examples", "powertrain.dbc")


def expected_rules(path):
    with open(path) as f:
        first = f.readline().strip()
    assert first.startswith("// expect:"), path
    body = first[len("// expect:"):].strip()
    if body == "none":
        return []
    return sorted(r.strip() for r in body.split(","))


class Formatting(unittest.TestCase):
    def test_fmt_f64(self):
        # A list, not a dict: 0.0 and -0.0 are equal keys in a dict.
        cases = [
            (0.0, "0.0"), (-0.0, "-0.0"), (1.0, "1.0"), (0.25, "0.25"), (0.1, "0.1"),
            (-40.0, "-40.0"), (655.35, "655.35"), (1e-7, "0.0000001"),
            (1e22, "10000000000000000000000.0"),
            # Shortest round-trip digits, so trailing digits become zeros.
            (18446744073709549568.0, "18446744073709550000.0"),
        ]
        for value, text in cases:
            self.assertEqual(ref.fmt_f64(value), text, value)

    def test_float_bounds(self):
        self.assertEqual(ref.f64_at_most((1 << 64) - 1), 18446744073709549568.0)
        self.assertEqual(ref.f64_at_most((1 << 63) - 1), 9223372036854774784.0)
        self.assertEqual(ref.f64_at_least(-(1 << 63)), -9223372036854775808.0)
        self.assertEqual(ref.f64_at_most(65535), 65535.0)


class Naming(unittest.TestCase):
    def test_snake(self):
        cases = {
            "EngineData": "engine_data", "ABSStatus": "abs_status", "RPM2Value": "rpm2_value",
            "Speed_Rear_Left": "speed_rear_left", "VehicleSpeed_kph": "vehicle_speed_kph",
            "DC link and faults": "dc_link_and_faults", "N/A": "n_a", "50%": "x_50",
            "": "x", "__": "x", "WheelSpeedFL": "wheel_speed_fl", "IgbtTemp": "igbt_temp",
        }
        for name, want in cases.items():
            self.assertEqual(ref.snake(name), want, name)

    def test_c_keywords_and_uniqueness(self):
        self.assertEqual(ref.c_ident("Static"), "static_")
        self.assertEqual(ref.c_ident("Default"), "default_")
        self.assertEqual(ref.unique_idents(["a", "a", "a_2", "a"]), ["a", "a_2", "a_2_2", "a_3"])


class Lexer(unittest.TestCase):
    def test_tokens(self):
        toks = ref.lex('SG_ X : 0|16@1- (1E-005,-40) [.5|2.] "a \\"q\\" b" // tail\nBO_')
        kinds = [(t.kind, t.text) for t in toks]
        self.assertIn(("num", "1E-005"), kinds)
        self.assertIn(("num", ".5"), kinds)
        self.assertIn(("num", "2."), kinds)
        self.assertIn(("str", 'a "q" b'), kinds)
        self.assertEqual(toks[-1].text, "BO_")
        self.assertEqual(toks[-1].line, 2)

    def test_multiline_string_counts_lines(self):
        toks = ref.lex('CM_ "one\ntwo";\nBO_')
        self.assertEqual(toks[1].text, "one\ntwo")
        self.assertEqual(toks[-1].line, 3)

    def test_bom_is_ignored(self):
        db = ref.parse('\ufeffVERSION "x"\n')
        self.assertEqual(db.version, "x")


class ParseErrors(unittest.TestCase):
    def assert_error(self, src, line, fragment):
        with self.assertRaises(ref.DbcError) as cm:
            ref.parse(src)
        self.assertEqual(cm.exception.line, line, cm.exception.message)
        self.assertIn(fragment, cm.exception.message)

    def test_errors(self):
        self.assert_error('CM_ "never closed;\n', 1, "unterminated string")
        self.assert_error('BO_ 1 M: 8 A\n SG_ X : 0|8@2+ (1,0) [0|1] "" A\n', 2, "byte order")
        self.assert_error('BO_ 1 M: 8 A\n SG_ X : 0|8@1* (1,0) [0|1] "" A\n', 2, "unexpected character")
        self.assert_error('BO_ 1 M: 8 A\n SG_ X q7 : 0|8@1+ (1,0) [0|1] "" A\n', 2, "multiplexer indicator")
        self.assert_error('SG_ X : 0|8@1+ (1,0) [0|1] "" A\n', 1, "must follow a BO_")
        self.assert_error("WHAT_ 1;\n", 1, "unknown keyword")
        self.assert_error("BO_ 1 M 8 A\n", 1, "expected ':'")

    def test_skips_unknown_statements(self):
        db = ref.parse('BA_DEF_ BO_ "x;y" INT 0 1;\nBO_ 5 M: 1 A\nBA_ "x" BO_ 5 1;\n')
        self.assertEqual(len(db.messages), 1)


class Bits(unittest.TestCase):
    def pack_segments(self, s, raw, dlc):
        """Pack the way the generated code does: one shift/mask per byte."""
        data = bytearray(dlc)
        u = raw & ((1 << s.length) - 1)
        for byte, p_lo, r_lo, count in ref.segments(s):
            data[byte] |= ((u >> r_lo) & ((1 << count) - 1)) << p_lo
        return bytes(data)

    def test_every_layout_round_trips(self):
        rng = random.Random(1)
        dlc = 8
        layouts = 0
        for little in (True, False):
            for start in range(dlc * 8):
                for length in range(1, 65):
                    s = ref.Signal()
                    s.start, s.length, s.little_endian = start, length, little
                    if any(byte >= dlc for byte, _ in ref.signal_bits(s)):
                        continue
                    layouts += 1
                    for signed in (False, True):
                        s.signed = signed
                        lo, hi = ref.raw_range(length, signed)
                        for raw in (lo, hi, rng.randint(lo, hi), rng.randint(lo, hi)):
                            data = bytearray(dlc)
                            ref.insert_raw(data, s, raw)
                            self.assertEqual(ref.raw_value(bytes(data), s), raw)
                            self.assertEqual(self.pack_segments(s, raw, dlc), bytes(data))
        # 8-byte frames hold 2080 Intel and 2080 Motorola layouts.
        self.assertEqual(layouts, 4160)

    def test_signals_do_not_disturb_neighbours(self):
        db = ref.parse_file(EXAMPLE)
        rng = random.Random(2)
        for m in db.messages:
            plain, groups, mux_sig = ref.split_groups(m)
            active = groups[0][1] if groups else []
            sigs = plain + active
            for _ in range(200):
                data = bytearray(m.dlc)
                values = {}
                for s in sigs:
                    lo, hi = ref.raw_range(s.length, s.signed and s.value_type == "integer")
                    raw = groups[0][0] if s is mux_sig else rng.randint(lo, hi)
                    values[s.name] = raw
                    ref.insert_raw(data, s, raw)
                for s in sigs:
                    self.assertEqual(ref.raw_value(bytes(data), s), values[s.name], s.name)


class Lint(unittest.TestCase):
    def test_fixtures(self):
        files = sorted(f for f in os.listdir(LINT_DIR) if f.endswith(".dbc"))
        self.assertGreaterEqual(len(files), 20)
        for name in files:
            path = os.path.join(LINT_DIR, name)
            got = sorted(d["rule"] for d in ref.lint(ref.parse_file(path)))
            self.assertEqual(got, expected_rules(path), name)

    def test_every_rule_has_a_fixture(self):
        covered = set()
        for name in os.listdir(LINT_DIR):
            covered.update(expected_rules(os.path.join(LINT_DIR, name)))
        self.assertEqual(covered, set(ref.RULE_INFO))

    def test_example_is_clean(self):
        rules = [d["rule"] for d in ref.lint(ref.parse_file(EXAMPLE))]
        self.assertEqual(rules, ["I002"])

    def test_codegen_refuses_errors(self):
        db = ref.parse_file(os.path.join(LINT_DIR, "e001_signal_overlap.dbc"))
        with self.assertRaises(ref.CodegenError):
            ref.gen_c(db, "x", "x.dbc")


class Decode(unittest.TestCase):
    def test_example_frames(self):
        db = ref.parse_file(EXAMPLE)
        r = ref.decode(db, 0x100, bytes.fromhex("e803035a18fc0005"))
        values = {s["name"]: s for s in r["signals"]}
        self.assertAlmostEqual(values["VehicleSpeed"]["physical"], 10.0)
        self.assertEqual(values["GearPosition"]["label"], "Drive")
        self.assertAlmostEqual(values["SteeringAngle"]["physical"], -100.0)
        r = ref.decode(db, 0x300, bytes.fromhex("0128320000000002"))
        names = [s["name"] for s in r["signals"]]
        self.assertEqual(r["mux"], "1")
        self.assertIn("StatorTemp", names)
        self.assertNotIn("MotorSpeed", names)
        self.assertEqual(r["signals"][-1]["name"], "InverterState")
        self.assertEqual(r["signals"][-1]["label"], "Running")

    def test_errors(self):
        db = ref.parse_file(EXAMPLE)
        with self.assertRaises(ref.DecodeError):
            ref.decode(db, 0x7AB, bytes(8))
        with self.assertRaises(ref.DecodeError):
            ref.decode(db, 0x100, bytes(3))


class Diff(unittest.TestCase):
    def test_fixture_pair(self):
        old = ref.parse_file(os.path.join(DIFF_DIR, "v1.dbc"))
        new = ref.parse_file(os.path.join(DIFF_DIR, "v2.dbc"))
        changes = ref.diff(old, new)
        kinds = [(c["level"], c["kind"]) for c in changes]
        self.assertEqual(ref.verdict(changes), "breaking")
        for expected in [
            ("breaking", "signal-layout-changed"), ("breaking", "signal-scaling-changed"),
            ("breaking", "message-removed"), ("breaking", "frame-id-changed"),
            ("caution", "signal-renamed"), ("caution", "signal-unit-changed"),
            ("caution", "signal-range-narrowed"), ("caution", "frame-length-increased"),
            ("compatible", "signal-added"), ("compatible", "message-added"),
            ("compatible", "signal-choices-added"), ("compatible", "comment-changed"),
            ("compatible", "signal-range-widened"),
        ]:
            self.assertIn(expected, kinds)
        self.assertNotIn(("breaking", "signal-removed"), kinds)

    def test_identical(self):
        db = ref.parse_file(EXAMPLE)
        self.assertEqual(ref.verdict(ref.diff(db, ref.parse_file(EXAMPLE))), "identical")


class LargeDatabase(unittest.TestCase):
    def test_deterministic_clean_and_varied(self):
        text = make_large_dbc.generate()
        self.assertEqual(text, make_large_dbc.generate())
        # 500 messages, 5,000 signals, and no lint findings but the CAN FD note.
        db = make_large_dbc.check(text)
        sigs = [s for m in db.messages for s in m.signals]
        present = {
            "Motorola signals": any(not s.little_endian for s in sigs),
            "signed signals": any(s.signed and s.value_type == "integer" for s in sigs),
            "float32 signals": any(s.value_type == "float32" for s in sigs),
            "64-bit signals": any(s.length == 64 for s in sigs),
            "29-bit frame IDs": any(m.is_extended for m in db.messages),
            "CAN FD frames": any(m.dlc == 64 for m in db.messages),
            "multiplexed messages": any(s.mux == ref.MUX_SWITCH for s in sigs),
            "value descriptions": any(s.choices for s in sigs),
            "multi-line comments": any("\n" in s.comment for s in sigs),
        }
        for what, found in present.items():
            self.assertTrue(found, what)


if __name__ == "__main__":
    unittest.main()
