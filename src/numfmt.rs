//! Number formatting shared by generated code and JSON output.
//!
//! Rust's `Display` for `f64` prints the shortest decimal that round-trips
//! and never switches to exponent notation. The reference model produces the
//! same text with `format(Decimal(repr(x)), "f")`, so generated code is
//! byte-identical between the two implementations.

/// Shortest round-trip decimal, without an exponent, always with a decimal point.
pub fn fmt_f64(x: f64) -> String {
    let s = format!("{}", x);
    if s.contains('.') || s.contains("inf") || s.contains("NaN") {
        s
    } else {
        format!("{}.0", s)
    }
}

/// A JSON number, or `null` for values JSON cannot represent.
pub fn json_num(x: f64) -> String {
    if x.is_finite() {
        fmt_f64(x)
    } else {
        "null".to_string()
    }
}

/// The next representable double toward positive infinity.
pub fn next_up(f: f64) -> f64 {
    if f.is_nan() || f == f64::INFINITY {
        return f;
    }
    if f == 0.0 {
        return f64::from_bits(1);
    }
    let bits = f.to_bits();
    if f > 0.0 {
        f64::from_bits(bits + 1)
    } else {
        f64::from_bits(bits - 1)
    }
}

/// The next representable double toward negative infinity.
pub fn next_down(f: f64) -> f64 {
    -next_up(-f)
}

/// The largest double that is less than or equal to `n`.
pub fn f64_at_most(n: i128) -> f64 {
    let mut f = n as f64;
    if (f as i128) > n {
        f = next_down(f);
    }
    f
}

/// The smallest double that is greater than or equal to `n`.
pub fn f64_at_least(n: i128) -> f64 {
    let mut f = n as f64;
    if (f as i128) < n {
        f = next_up(f);
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_like_the_reference_model() {
        let cases: [(f64, &str); 10] = [
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (1.0, "1.0"),
            (0.25, "0.25"),
            (0.1, "0.1"),
            (-40.0, "-40.0"),
            (655.35, "655.35"),
            (1e-7, "0.0000001"),
            (1e22, "10000000000000000000000.0"),
            (18446744073709549568.0, "18446744073709550000.0"),
        ];
        for (value, text) in cases.iter() {
            assert_eq!(fmt_f64(*value), *text, "formatting {:?}", value);
        }
    }

    #[test]
    fn float_bounds_are_exact() {
        assert_eq!(f64_at_most((1i128 << 64) - 1), 18446744073709549568.0);
        assert_eq!(f64_at_most((1i128 << 63) - 1), 9223372036854774784.0);
        assert_eq!(f64_at_least(-(1i128 << 63)), -9223372036854775808.0);
        assert_eq!(f64_at_most(65535), 65535.0);
        assert_eq!(next_up(0.0), f64::from_bits(1));
    }
}
