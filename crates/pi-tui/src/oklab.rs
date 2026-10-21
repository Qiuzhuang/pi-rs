//! Oklab and OKHSL <-> sRGB conversion. Port of `packages/tui/src/oklab.ts`.
//!
//! Oklab and OKHSL are Björn Ottosson's color spaces; this is a port of his reference
//! implementation (<https://bottosson.github.io/posts/colorpicker/>), Copyright (c) 2021
//! Björn Ottosson, used under the MIT license.

use crate::terminal_colors::RgbColor;

pub type Vector = [f64; 3];
type Matrix = [Vector; 3];

fn multiply(m: &Matrix, v: Vector) -> Vector {
    let [x, y, z] = v;
    [
        m[0][0] * x + m[0][1] * y + m[0][2] * z,
        m[1][0] * x + m[1][1] * y + m[1][2] * z,
        m[2][0] * x + m[2][1] * y + m[2][2] * z,
    ]
}

const LINEAR_SRGB_TO_LMS: Matrix = [
    [0.4122214694707629, 0.5363325372617349, 0.0514459932675022],
    [0.2119034958178251, 0.6806995506452344, 0.1073969535369405],
    [0.0883024591900564, 0.2817188391361215, 0.6299787016738222],
];
const LMS_TO_LAB: Matrix = [
    [0.210454268309314, 0.793617774702305, -0.0040720430116193],
    [1.9779985324311684, -2.42859224204858, 0.450593709617411],
    [0.0259040424655478, 0.7827717124575296, -0.8086757549230774],
];
const LAB_TO_LMS: Matrix = [
    [1.0, 0.3963377773761749, 0.2158037573099136],
    [1.0, -0.1055613458156586, -0.0638541728258133],
    [1.0, -0.0894841775298119, -1.2914855480194092],
];
#[allow(clippy::excessive_precision)]
const LMS_TO_LINEAR_SRGB: Matrix = [
    [4.0767416360759583, -3.3077115392580629, 0.2309699031821043],
    [-1.2684379732850315, 2.6097573492876882, -0.341319376002657],
    [-0.0041960761386756, -0.7034186179359362, 1.7076146940746117],
];

type SaturationFit = ([f64; 2], [f64; 5]);
const SATURATION_FIT: [SaturationFit; 3] = [
    ([-1.8817031, -0.80936501], [1.19086277, 1.76576728, 0.59662641, 0.75515197, 0.56771245]),
    ([1.8144408, -1.19445267], [0.73956515, -0.45954404, 0.08285427, 0.12541073, -0.14503204]),
    ([0.13110758, 1.81333971], [1.35733652, -0.00915799, -1.1513021, -0.50559606, 0.00692167]),
];
const K1: f64 = 0.206;
const K2: f64 = 0.03;
const K3: f64 = (1.0 + K1) / (1.0 + K2);

/// Oklab lightness to OKHSL lightness.
pub fn oklab_to_okhsl_lightness(x: f64) -> f64 {
    0.5 * (K3 * x - K1 + ((K3 * x - K1).powf(2.0) + 4.0 * K2 * K3 * x).sqrt())
}

fn okhsl_to_oklab_lightness(x: f64) -> f64 {
    (x * x + K1 * x) / (K3 * (x + K2))
}

fn linear_to_srgb(value: f64) -> f64 {
    if value > 0.0031308 { 1.055 * value.powf(1.0 / 2.4) - 0.055 } else { 12.92 * value }
}

fn srgb_to_linear(value: f64) -> f64 {
    if value <= 0.04045 { value / 12.92 } else { ((value + 0.055) / 1.055).powf(2.4) }
}

/// Oklab `[L, a, b]` to linear sRGB `[r, g, b]` (0-1, may leave the gamut).
pub fn oklab_to_linear_srgb(lab: Vector) -> Vector {
    let lms = multiply(&LAB_TO_LMS, lab).map(|v| v.powf(3.0));
    multiply(&LMS_TO_LINEAR_SRGB, lms)
}

fn linear_srgb_to_oklab(rgb: Vector) -> Vector {
    let lms = multiply(&LINEAR_SRGB_TO_LMS, rgb).map(f64::cbrt);
    multiply(&LMS_TO_LAB, lms)
}

/// sRGB channels (0-255) to Oklab `[L, a, b]`.
pub fn rgb_to_oklab(rgb: RgbColor) -> Vector {
    linear_srgb_to_oklab([rgb.r / 255.0, rgb.g / 255.0, rgb.b / 255.0].map(srgb_to_linear))
}

/// JavaScript `Math.round` (rounds half up towards +infinity).
pub(crate) fn js_round(v: f64) -> f64 {
    (v + 0.5).floor()
}

/// Linear sRGB to sRGB channels (0-255, rounded), clipping out-of-gamut channels.
pub fn linear_srgb_to_rgb(linear: Vector) -> RgbColor {
    let [r, g, b] = linear.map(|v| js_round(linear_to_srgb(v).clamp(0.0, 1.0) * 255.0));
    RgbColor { r, g, b }
}

fn lms_slopes(a: f64, b: f64) -> Vector {
    [
        LAB_TO_LMS[0][1] * a + LAB_TO_LMS[0][2] * b,
        LAB_TO_LMS[1][1] * a + LAB_TO_LMS[1][2] * b,
        LAB_TO_LMS[2][1] * a + LAB_TO_LMS[2][2] * b,
    ]
}

fn max_saturation(a: f64, b: f64) -> f64 {
    let channel = SATURATION_FIT
        .iter()
        .enumerate()
        .position(|(index, ([x, y], _))| index == 2 || x * a + y * b > 1.0)
        .unwrap_or(2);
    let [k0, k1, k2, k3, k4] = SATURATION_FIT[channel].1;
    let weights = LMS_TO_LINEAR_SRGB[channel];
    let saturation = k0 + k1 * a + k2 * b + k3 * a * a + k4 * a * b;

    let slopes = lms_slopes(a, b);
    let base = slopes.map(|k| 1.0 + saturation * k);
    let dot = |values: [f64; 3]| values.iter().enumerate().fold(0.0, |sum, (i, v)| sum + weights[i] * v);
    let f = dot(base.map(|v| v.powf(3.0)));
    let f1 = dot([0, 1, 2].map(|i| 3.0 * slopes[i] * base[i].powf(2.0)));
    let f2 = dot([0, 1, 2].map(|i| 6.0 * slopes[i].powf(2.0) * base[i]));
    saturation - (f * f1) / (f1 * f1 - 0.5 * f * f2)
}

fn cusp(a: f64, b: f64) -> (f64, f64) {
    let saturation = max_saturation(a, b);
    let rgb = oklab_to_linear_srgb([1.0, saturation * a, saturation * b]);
    let max = rgb.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let lightness = (1.0 / max).cbrt();
    (lightness, lightness * saturation)
}

fn max_chroma(a: f64, b: f64, lightness: f64, (cusp_l, cusp_c): (f64, f64)) -> f64 {
    if lightness <= cusp_l {
        return (cusp_c * lightness) / cusp_l;
    }
    let t = (cusp_c * (lightness - 1.0)) / (cusp_l - 1.0);
    let slopes = lms_slopes(a, b);
    let lms = slopes.map(|k| lightness + t * k);
    let cubes = lms.map(|v| v.powf(3.0));
    let first = [0, 1, 2].map(|i| 3.0 * slopes[i] * lms[i].powf(2.0));
    let second = [0, 1, 2].map(|i| 6.0 * slopes[i].powf(2.0) * lms[i]);
    let dot = |row: &Vector, values: &[f64; 3]| row[0] * values[0] + row[1] * values[1] + row[2] * values[2];
    let min_step = LMS_TO_LINEAR_SRGB
        .iter()
        .map(|row| {
            let f = dot(row, &cubes) - 1.0;
            let f1 = dot(row, &first);
            let f2 = dot(row, &second);
            let u = f1 / (f1 * f1 - 0.5 * f * f2);
            if u >= 0.0 { -f * u } else { f64::MAX }
        })
        .fold(f64::INFINITY, f64::min);
    t + min_step
}

fn chroma_stops(l: f64, a: f64, b: f64) -> (f64, f64, f64) {
    let peak = cusp(a, b);
    let c_max = max_chroma(a, b, l, peak);
    let k = c_max / (l * (peak.1 / peak.0)).min((1.0 - l) * (peak.1 / (1.0 - peak.0)));
    let mid_s = 0.11516993
        + 1.0
            / (7.4477897
                + 4.1590124 * b
                + a * (-2.19557347
                    + 1.75198401 * b
                    + a * (-2.13704948 - 10.02301043 * b + a * (-4.24894561 + 5.38770819 * b + 4.69891013 * a))));
    let mid_t = 0.11239642
        + 1.0
            / (1.6132032 - 0.68124379 * b
                + a * (0.40370612
                    + 0.90148123 * b
                    + a * (-0.27087943 + 0.6122399 * b + a * (0.00299215 - 0.45399568 * b - 0.14661872 * a))));
    let c_mid = 0.9 * k * (1.0 / (1.0 / (l * mid_s).powf(4.0) + 1.0 / ((1.0 - l) * mid_t).powf(4.0))).sqrt().sqrt();
    let c0 = (1.0 / (1.0 / (l * 0.4).powf(2.0) + 1.0 / ((1.0 - l) * 0.8).powf(2.0))).sqrt();
    (c0, c_mid, c_max)
}

/// Convert OKHSL (hue degrees, saturation 0-1, lightness 0-1) to sRGB channels (0-255, rounded).
pub fn okhsl_to_rgb(hue: f64, saturation: f64, lightness: f64) -> RgbColor {
    let l = okhsl_to_oklab_lightness(lightness);
    let mut lab: Vector = [l, 0.0, 0.0];
    if l > 0.0 && l < 1.0 && saturation > 0.0 {
        let angle = (2.0 * std::f64::consts::PI * (((hue % 360.0) + 360.0) % 360.0)) / 360.0;
        let a = angle.cos();
        let b = angle.sin();
        let (c0, c_mid, c_max) = chroma_stops(l, a, b);
        let chroma = if saturation < 0.8 {
            let t = 1.25 * saturation;
            let k1 = 0.8 * c0;
            (t * k1) / (1.0 - (1.0 - k1 / c_mid) * t)
        } else {
            let t = 5.0 * (saturation - 0.8);
            let k1 = (0.2 * c_mid.powf(2.0) * 1.25f64.powf(2.0)) / c0;
            c_mid + (t * k1) / (1.0 - (1.0 - k1 / (c_max - c_mid)) * t)
        };
        lab = [l, chroma * a, chroma * b];
    }
    linear_srgb_to_rgb(oklab_to_linear_srgb(lab))
}

/// OKHSL channels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Okhsl {
    pub h: f64,
    pub s: f64,
    pub l: f64,
}

/// Convert sRGB channels (0-255) to OKHSL. Hue is 0 for grays.
pub fn rgb_to_okhsl(rgb: RgbColor) -> Okhsl {
    let [l, lab_a, lab_b] = rgb_to_oklab(rgb);
    let chroma = lab_a.hypot(lab_b);
    let lightness = oklab_to_okhsl_lightness(l);
    if chroma < 1e-9 || lightness <= 0.0 || lightness >= 1.0 {
        return Okhsl { h: 0.0, s: 0.0, l: lightness };
    }
    let hue = (lab_b.atan2(lab_a) * 180.0 / std::f64::consts::PI + 360.0) % 360.0;
    let (c0, c_mid, c_max) = chroma_stops(l, lab_a / chroma, lab_b / chroma);
    let saturation = if chroma < c_mid {
        let k1 = 0.8 * c0;
        0.8 * (chroma / (k1 + (1.0 - k1 / c_mid) * chroma))
    } else {
        let k1 = (0.2 * c_mid.powf(2.0) * 1.25f64.powf(2.0)) / c0;
        let offset = chroma - c_mid;
        0.8 + 0.2 * (offset / (k1 + (1.0 - k1 / (c_max - c_mid)) * offset))
    };
    Okhsl { h: hue, s: saturation.clamp(0.0, 1.0), l: lightness }
}
