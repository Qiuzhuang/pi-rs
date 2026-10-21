//! npm (`node-semver`) version and range semantics on top of the `semver` crate.
//!
//! The `semver` crate parses versions but implements Cargo's requirement syntax, where `1.2.3`
//! means `^1.2.3` and comparators are comma separated. npm ranges differ (`1.2.3` is exact,
//! comparators are space separated, `||` alternatives, hyphen ranges, x-ranges), so ranges are
//! parsed here and desugared exactly like `node-semver`'s `Range` class. Versions are
//! `semver::Version`s compared without build metadata, like `node-semver`.

use std::cmp::Ordering;

use semver::{Prerelease, Version};

/// Maximum length of a version string accepted by `node-semver`.
const MAX_LENGTH: usize = 256;

/// `semver.valid(v)` (non-loose): a trimmed version with an optional leading `v`.
pub fn parse_version(value: &str) -> Option<Version> {
    let trimmed = value.trim();
    if trimmed.len() > MAX_LENGTH {
        return None;
    }
    let stripped = trimmed.strip_prefix('v').unwrap_or(trimmed);
    Version::parse(stripped).ok()
}

/// `semver.valid(v) !== null`.
pub fn is_valid_version(value: &str) -> bool {
    parse_version(value).is_some()
}

/// `semver.compare(a, b)`: precedence ignoring build metadata.
pub fn compare(a: &Version, b: &Version) -> Ordering {
    a.major
        .cmp(&b.major)
        .then(a.minor.cmp(&b.minor))
        .then(a.patch.cmp(&b.patch))
        .then_with(|| a.pre.cmp(&b.pre))
}

/// `semver.gt(a, b)`; errors when either side is not a valid version (node-semver throws).
pub fn gt(a: &str, b: &str) -> anyhow::Result<bool> {
    let left = parse_version(a).ok_or_else(|| anyhow::anyhow!("Invalid Version: {a}"))?;
    let right = parse_version(b).ok_or_else(|| anyhow::anyhow!("Invalid Version: {b}"))?;
    Ok(compare(&left, &right) == Ordering::Greater)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
}

#[derive(Debug, Clone)]
struct Comparator {
    op: Op,
    version: Version,
}

impl Comparator {
    fn test(&self, version: &Version) -> bool {
        let ord = compare(version, &self.version);
        match self.op {
            Op::Lt => ord == Ordering::Less,
            Op::Le => ord != Ordering::Greater,
            Op::Gt => ord == Ordering::Greater,
            Op::Ge => ord != Ordering::Less,
            Op::Eq => ord == Ordering::Equal,
        }
    }
}

/// A parsed npm range: alternatives (`||`) of comparator sets. An empty set matches anything.
#[derive(Debug, Clone)]
pub struct Range {
    sets: Vec<Vec<Comparator>>,
}

/// A partial version: `None` components are `x`, `X`, `*` or missing.
#[derive(Debug, Clone, Default)]
struct Partial {
    major: Option<u64>,
    minor: Option<u64>,
    patch: Option<u64>,
    pre: String,
}

impl Partial {
    fn any_x(&self) -> bool {
        self.major.is_none() || self.minor.is_none() || self.patch.is_none()
    }
}

fn version(major: u64, minor: u64, patch: u64, pre: &str) -> Option<Version> {
    let pre = if pre.is_empty() {
        Prerelease::EMPTY
    } else {
        Prerelease::new(pre).ok()?
    };
    Some(Version {
        major,
        minor,
        patch,
        pre,
        build: semver::BuildMetadata::EMPTY,
    })
}

fn parse_xr(part: &str) -> Option<Option<u64>> {
    if part == "x" || part == "X" || part == "*" {
        return Some(None);
    }
    if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // Numeric identifiers in ranges may not have leading zeros (except "0").
    if part.len() > 1 && part.starts_with('0') {
        return None;
    }
    part.parse::<u64>().ok().map(Some)
}

/// Parse `XRANGEPLAIN`: `[v=\s]*xr(.xr(.xr(-pre)?(+build)?)?)?`.
fn parse_partial(input: &str) -> Option<Partial> {
    let s = input.trim_start_matches(|c: char| c == 'v' || c == '=' || c.is_whitespace());
    let s = match s.find('+') {
        Some(index) => {
            let build = &s[index + 1..];
            if build.is_empty()
                || !build
                    .split('.')
                    .all(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
            {
                return None;
            }
            &s[..index]
        }
        None => s,
    };
    let (main, pre) = match s.find('-') {
        Some(index) => (&s[..index], &s[index + 1..]),
        None => (s, ""),
    };
    let parts: Vec<&str> = main.split('.').collect();
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    let major = parse_xr(parts[0])?;
    let minor = match parts.get(1) {
        Some(p) => parse_xr(p)?,
        None => None,
    };
    let patch = match parts.get(2) {
        Some(p) => parse_xr(p)?,
        None => None,
    };
    if !pre.is_empty() || s.contains('-') {
        // A prerelease is only allowed after a full major.minor.patch.
        if parts.len() != 3 || pre.is_empty() || Prerelease::new(pre).is_err() {
            return None;
        }
    }
    Some(Partial {
        major,
        minor,
        patch,
        pre: pre.to_string(),
    })
}

fn push(out: &mut Vec<Comparator>, op: Op, v: Option<Version>) -> Option<()> {
    out.push(Comparator { op, version: v? });
    Some(())
}

/// The comparator that matches nothing: `<0.0.0-0`.
fn nothing(out: &mut Vec<Comparator>) -> Option<()> {
    push(out, Op::Lt, version(0, 0, 0, "0"))
}

fn replace_caret(p: &Partial, out: &mut Vec<Comparator>) -> Option<()> {
    let Some(major) = p.major else {
        return Some(());
    };
    match (p.minor, p.patch) {
        (None, _) => {
            push(out, Op::Ge, version(major, 0, 0, ""))?;
            push(out, Op::Lt, version(major + 1, 0, 0, "0"))
        }
        (Some(minor), None) => {
            push(out, Op::Ge, version(major, minor, 0, ""))?;
            if major == 0 {
                push(out, Op::Lt, version(major, minor + 1, 0, "0"))
            } else {
                push(out, Op::Lt, version(major + 1, 0, 0, "0"))
            }
        }
        (Some(minor), Some(patch)) => {
            push(out, Op::Ge, version(major, minor, patch, &p.pre))?;
            if major == 0 {
                if minor == 0 {
                    push(out, Op::Lt, version(major, minor, patch + 1, "0"))
                } else {
                    push(out, Op::Lt, version(major, minor + 1, 0, "0"))
                }
            } else {
                push(out, Op::Lt, version(major + 1, 0, 0, "0"))
            }
        }
    }
}

fn replace_tilde(p: &Partial, out: &mut Vec<Comparator>) -> Option<()> {
    let Some(major) = p.major else {
        return Some(());
    };
    match (p.minor, p.patch) {
        (None, _) => {
            push(out, Op::Ge, version(major, 0, 0, ""))?;
            push(out, Op::Lt, version(major + 1, 0, 0, "0"))
        }
        (Some(minor), None) => {
            push(out, Op::Ge, version(major, minor, 0, ""))?;
            push(out, Op::Lt, version(major, minor + 1, 0, "0"))
        }
        (Some(minor), Some(patch)) => {
            push(out, Op::Ge, version(major, minor, patch, &p.pre))?;
            push(out, Op::Lt, version(major, minor + 1, 0, "0"))
        }
    }
}

/// `replaceXRange` for primitives (`<`, `<=`, `>`, `>=`, `=` or none).
fn replace_xrange(op: &str, p: &Partial, out: &mut Vec<Comparator>) -> Option<()> {
    let mut gtlt = op;
    if gtlt == "=" && p.any_x() {
        gtlt = "";
    }
    let Some(major) = p.major else {
        if gtlt == ">" || gtlt == "<" {
            return nothing(out);
        }
        return Some(());
    };
    if !gtlt.is_empty() && p.any_x() {
        let mut major = major;
        let mut minor = p.minor.unwrap_or(0);
        let mut patch = 0;
        let mut pre = "";
        let mut op = gtlt;
        if gtlt == ">" {
            op = ">=";
            if p.minor.is_none() {
                major += 1;
                minor = 0;
                patch = 0;
            } else {
                minor += 1;
                patch = 0;
            }
        } else if gtlt == "<=" {
            op = "<";
            if p.minor.is_none() {
                major += 1;
            } else {
                minor += 1;
            }
        }
        if op == "<" {
            pre = "0";
        }
        let op = match op {
            ">=" => Op::Ge,
            "<" => Op::Lt,
            ">" => Op::Gt,
            "<=" => Op::Le,
            _ => Op::Eq,
        };
        return push(out, op, version(major, minor, patch, pre));
    }
    match (p.minor, p.patch) {
        (None, _) => {
            push(out, Op::Ge, version(major, 0, 0, ""))?;
            push(out, Op::Lt, version(major + 1, 0, 0, "0"))
        }
        (Some(minor), None) => {
            push(out, Op::Ge, version(major, minor, 0, ""))?;
            push(out, Op::Lt, version(major, minor + 1, 0, "0"))
        }
        (Some(minor), Some(patch)) => {
            let v = version(major, minor, patch, &p.pre);
            let op = match gtlt {
                "<" => Op::Lt,
                "<=" => Op::Le,
                ">" => Op::Gt,
                ">=" => Op::Ge,
                _ => Op::Eq,
            };
            push(out, op, v)
        }
    }
}

fn parse_simple(token: &str, out: &mut Vec<Comparator>) -> Option<()> {
    if token == "*" || token.is_empty() {
        return Some(());
    }
    for (prefix, kind) in [
        ("~>", "~"),
        ("~", "~"),
        ("^", "^"),
        ("<=", "<="),
        (">=", ">="),
        ("<", "<"),
        (">", ">"),
        ("=", "="),
    ] {
        if let Some(rest) = token.strip_prefix(prefix) {
            let partial = parse_partial(rest)?;
            return match kind {
                "~" => replace_tilde(&partial, out),
                "^" => replace_caret(&partial, out),
                op => replace_xrange(op, &partial, out),
            };
        }
    }
    let partial = parse_partial(token)?;
    replace_xrange("", &partial, out)
}

fn parse_hyphen(range: &str) -> Option<Option<Vec<Comparator>>> {
    // /^\s*(XRANGEPLAIN)\s+-\s+(XRANGEPLAIN)\s*$/
    let tokens: Vec<&str> = range.split_whitespace().collect();
    if tokens.len() != 3 || tokens[1] != "-" {
        return Some(None);
    }
    let Some(from) = parse_partial(tokens[0]) else {
        return Some(None);
    };
    let Some(to) = parse_partial(tokens[2]) else {
        return Some(None);
    };
    let mut out = Vec::new();
    match (from.major, from.minor, from.patch) {
        (None, _, _) => {}
        (Some(m), None, _) => push(&mut out, Op::Ge, version(m, 0, 0, ""))?,
        (Some(m), Some(n), None) => push(&mut out, Op::Ge, version(m, n, 0, ""))?,
        (Some(m), Some(n), Some(p)) => push(&mut out, Op::Ge, version(m, n, p, &from.pre))?,
    }
    match (to.major, to.minor, to.patch) {
        (None, _, _) => {}
        (Some(m), None, _) => push(&mut out, Op::Lt, version(m + 1, 0, 0, "0"))?,
        (Some(m), Some(n), None) => push(&mut out, Op::Lt, version(m, n + 1, 0, "0"))?,
        (Some(m), Some(n), Some(p)) => push(&mut out, Op::Le, version(m, n, p, &to.pre))?,
    }
    Some(Some(out))
}

/// Join operators to their versions: `>= 1.2.3` → `>=1.2.3`, `~ 1.2` → `~1.2`.
fn trim_operators(range: &str) -> String {
    let mut out = String::with_capacity(range.len());
    let mut chars = range.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if matches!(c, '<' | '>' | '=' | '~' | '^') {
            let next_is_op = matches!(chars.peek(), Some('=') | Some('>'));
            if !next_is_op {
                while chars.peek().is_some_and(|c| c.is_whitespace()) {
                    chars.next();
                }
            }
        }
    }
    out
}

impl Range {
    /// `new Range(range)`; `None` where node-semver throws (`validRange` returns null).
    pub fn parse(range: &str) -> Option<Range> {
        let trimmed = range.trim();
        if trimmed.len() > MAX_LENGTH {
            return None;
        }
        let mut sets = Vec::new();
        for part in trimmed.split("||") {
            let part = part.trim();
            if let Some(set) = parse_hyphen(part)? {
                sets.push(set);
                continue;
            }
            let normalized = trim_operators(part);
            let mut set = Vec::new();
            for token in normalized.split_whitespace() {
                parse_simple(token, &mut set)?;
            }
            sets.push(set);
        }
        if sets.is_empty() {
            sets.push(Vec::new());
        }
        Some(Range { sets })
    }

    /// `range.test(version)`, including the prerelease rule.
    pub fn test(&self, version: &Version) -> bool {
        self.sets.iter().any(|set| test_set(set, version))
    }
}

fn test_set(set: &[Comparator], version: &Version) -> bool {
    if !set.iter().all(|c| c.test(version)) {
        return false;
    }
    if !version.pre.is_empty() {
        // Prereleases only match a comparator set that names a prerelease of the same tuple.
        return set.iter().any(|c| {
            !c.version.pre.is_empty()
                && c.version.major == version.major
                && c.version.minor == version.minor
                && c.version.patch == version.patch
        });
    }
    true
}

/// `semver.validRange(range) !== null`.
pub fn is_valid_range(range: &str) -> bool {
    Range::parse(range).is_some()
}

/// `semver.satisfies(version, range)`.
pub fn satisfies(version: &str, range: &str) -> bool {
    let (Some(v), Some(r)) = (parse_version(version), Range::parse(range)) else {
        return false;
    };
    r.test(&v)
}

/// `semver.maxSatisfying(versions, range)`.
pub fn max_satisfying<'a>(versions: impl IntoIterator<Item = &'a str>, range: &str) -> Option<String> {
    let range = Range::parse(range)?;
    let mut best: Option<(Version, &str)> = None;
    for candidate in versions {
        let Some(v) = parse_version(candidate) else {
            continue;
        };
        if !range.test(&v) {
            continue;
        }
        if best.as_ref().is_none_or(|(b, _)| compare(&v, b) == Ordering::Greater) {
            best = Some((v, candidate));
        }
    }
    best.map(|(_, s)| s.to_string())
}

/// The highest valid version (`[...versions].sort(rcompare)[0]`).
pub fn max_version<'a>(versions: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let mut best: Option<(Version, &str)> = None;
    for candidate in versions {
        let Some(v) = parse_version(candidate) else {
            continue;
        };
        if best.as_ref().is_none_or(|(b, _)| compare(&v, b) == Ordering::Greater) {
            best = Some((v, candidate));
        }
    }
    best.map(|(_, s)| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_versions_like_node_semver() {
        assert!(is_valid_version("1.2.3"));
        assert!(is_valid_version("v1.2.3"));
        assert!(is_valid_version(" 1.2.3-beta.1+build "));
        assert!(!is_valid_version("^1.2.3"));
        assert!(!is_valid_version("latest"));
        assert!(!is_valid_version("1.2"));
        assert!(!is_valid_version("=1.2.3"));
        assert!(!is_valid_version("01.2.3"));
    }

    #[test]
    fn validates_ranges_like_node_semver() {
        for r in [
            "^1.2.3", "~1.2", "1.x", "*", "", ">=1 <2", "1.2.3 - 2", "1 || 2", ">= 1.2.3", "=1.2.3",
            "v1.2.3", "~>1.2", "1.2.3-beta.1",
        ] {
            assert!(is_valid_range(r), "{r}");
        }
        for r in ["latest", "next", "beta", "^x.y", "1.2.3.4", "foo || 1"] {
            assert!(!is_valid_range(r), "{r}");
        }
    }

    #[test]
    fn satisfies_matches_node_semver() {
        let cases: &[(&str, &str, bool)] = &[
            ("1.2.3", "1.2.3", true),
            ("1.2.4", "1.2.3", false),
            ("1.9.9", "^1.2.3", true),
            ("2.0.0", "^1.2.3", false),
            ("0.2.5", "^0.2.3", true),
            ("0.3.0", "^0.2.3", false),
            ("0.0.3", "^0.0.3", true),
            ("0.0.4", "^0.0.3", false),
            ("1.2.9", "~1.2.3", true),
            ("1.3.0", "~1.2.3", false),
            ("1.5.0", "1.x", true),
            ("2.0.0", "1.x", false),
            ("1.5.0", ">=1 <2", true),
            ("2.0.0", ">=1 <2", false),
            ("2.3.4", "1.2.3 - 2.3.4", true),
            ("2.3.5", "1.2.3 - 2.3.4", false),
            ("2.9.0", "1.2.3 - 2", true),
            ("3.0.0", "1.2.3 - 2", false),
            ("3.0.0", "1 || 3", true),
            ("2.0.0", "1 || 3", false),
            ("2.0.0", "*", true),
            ("2.0.0", "", true),
            ("1.2.4-beta", "^1.2.3", false),
            ("1.2.3-beta.2", "^1.2.3-beta.1", true),
            ("1.3.0-beta.2", "^1.2.3-beta.1", false),
            ("2.0.0", ">1.x", true),
            ("1.9.0", ">1.x", false),
            ("1.2.9", "<=1.2", true),
            ("1.3.0", "<=1.2", false),
            ("1.0.0", "<1.x", false),
            ("0.9.0", "<1.x", true),
            ("1.2.3+build", "1.2.3", true),
            ("1.2.3", ">= 1.2.3", true),
        ];
        for (v, r, expected) in cases {
            assert_eq!(satisfies(v, r), *expected, "{v} satisfies {r}");
        }
    }

    #[test]
    fn picks_max_versions() {
        let versions = ["1.0.0", "1.2.0", "1.3.1", "2.0.0", "2.1.0-beta"];
        assert_eq!(max_satisfying(versions, "^1.0.0").as_deref(), Some("1.3.1"));
        assert_eq!(max_satisfying(versions, "^3").as_deref(), None);
        assert_eq!(max_version(versions).as_deref(), Some("2.1.0-beta"));
        assert!(gt("2.0.0", "1.9.9").unwrap());
        assert!(!gt("1.0.0", "1.0.0").unwrap());
        assert!(gt("bad", "1.0.0").is_err());
    }
}
