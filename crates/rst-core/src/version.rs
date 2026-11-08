//! Comparaison de versions semver (majeure.mineure.correctif ; préfixe `v`, suffixes `-pre`/`+build` ignorés).

use std::cmp::Ordering;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u64, pub u64, pub u64);

impl Version {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches(['v', 'V']);
        let core = s.split(['-', '+']).next()?;
        let mut it = core.split('.');
        let mut next = || it.next().and_then(|p| p.parse::<u64>().ok());
        let v = Self(next()?, next()?, next()?);
        it.next().is_none().then_some(v)
    }

    pub fn cmp_str(a: &str, b: &str) -> Option<Ordering> {
        Some(Self::parse(a)?.cmp(&Self::parse(b)?))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses() {
        assert_eq!(Version::parse("v0.1.2"), Some(Version(0, 1, 2)));
        assert_eq!(Version::parse("1.2.3-rc1+x"), Some(Version(1, 2, 3)));
        assert_eq!(Version::parse("1.2"), None);
        assert_eq!(Version::parse("1.2.3.4"), None);
        assert_eq!(Version::parse("abc"), None);
    }

    #[test]
    fn compares_numerically() {
        assert!(Version::parse("0.10.0") > Version::parse("0.9.9"));
        assert!(Version::parse("v0.2.0") > Version::parse("0.1.2"));
        assert_eq!(Version::cmp_str("0.2.0", "v0.2.0"), Some(Ordering::Equal));
    }
}
