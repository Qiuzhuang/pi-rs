//! JavaScript-compatible number formatting, so rendered traces and serialized snapshots match the
//! TypeScript implementation byte for byte (`2`, not `2.0`; `1e+21`, not `1000000000000000000000`).

use serde::Serializer;

/// `Number.prototype.toString()` for a finite or non-finite `f64`.
pub fn number_to_string(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    if value == 0.0 {
        return "0".to_string();
    }
    let negative = value < 0.0;
    // Shortest round-trip digits in scientific form, for example `1.2345e-7`.
    let scientific = format!("{:e}", value.abs());
    let (mantissa, exponent) = scientific
        .split_once('e')
        .unwrap_or((scientific.as_str(), "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n = exponent + 1;
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if k <= n && n <= 21 {
        out.push_str(&digits);
        for _ in 0..(n - k) {
            out.push('0');
        }
    } else if 0 < n && n <= 21 {
        out.push_str(&digits[..n as usize]);
        out.push('.');
        out.push_str(&digits[n as usize..]);
    } else if -6 < n && n <= 0 {
        out.push_str("0.");
        for _ in 0..(-n) {
            out.push('0');
        }
        out.push_str(&digits);
    } else {
        let e = n - 1;
        let sign = if e < 0 { '-' } else { '+' };
        out.push_str(&digits[..1]);
        if k > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        out.push(sign);
        out.push_str(&e.abs().to_string());
    }
    out
}

/// `JSON.stringify` of a number: non-finite values become `null`.
pub fn number_to_json(value: f64) -> String {
    if value.is_finite() {
        number_to_string(value)
    } else {
        "null".to_string()
    }
}

/// Serialize an `f64` the way `JSON.stringify` would: integral values without a fraction,
/// non-finite values as `null`.
pub fn serialize_number<S: Serializer>(value: f64, serializer: S) -> Result<S::Ok, S::Error> {
    if !value.is_finite() {
        return serializer.serialize_unit();
    }
    if value.fract() == 0.0 && value.abs() < 9_007_199_254_740_992.0 {
        if value == 0.0 {
            return serializer.serialize_i64(0);
        }
        return serializer.serialize_i64(value as i64);
    }
    serializer.serialize_f64(value)
}

pub(crate) mod number {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &f64, serializer: S) -> Result<S::Ok, S::Error> {
        super::serialize_number(*value, serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
        f64::deserialize(deserializer)
    }
}

pub(crate) mod numbers {
    use serde::Serializer;
    use serde::ser::SerializeSeq;

    pub fn serialize<S: Serializer>(values: &[f64], serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(values.len()))?;
        for value in values {
            seq.serialize_element(&super::JsNumber(*value))?;
        }
        seq.end()
    }
}

pub(crate) mod option_numbers {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(
        values: &Option<Vec<f64>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match values {
            Some(values) => super::numbers::serialize(values, serializer),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Vec<f64>>, D::Error> {
        Option::<Vec<f64>>::deserialize(deserializer)
    }
}

/// An `f64` that serializes like a JavaScript number.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JsNumber(pub f64);

impl serde::Serialize for JsNumber {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_number(self.0, serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::number_to_string;

    #[test]
    fn formats_like_javascript() {
        let cases: &[(f64, &str)] = &[
            (0.0, "0"),
            (-0.0, "0"),
            (2.0, "2"),
            (3.5, "3.5"),
            (-4.5, "-4.5"),
            (0.1, "0.1"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (1.5e-7, "1.5e-7"),
            (0.000001, "0.000001"),
            (123456.789, "123456.789"),
            (f64::NAN, "NaN"),
            (f64::INFINITY, "Infinity"),
        ];
        for (value, expected) in cases {
            assert_eq!(number_to_string(*value), *expected, "{value}");
        }
    }
}
