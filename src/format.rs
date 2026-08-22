//! The set of things we can convert between.
//!
//! A `Format` is either a real positional number base (2..=36) or one of the
//! "pseudo-bases" that are really byte encodings. Treating them uniformly is
//! what lets `hex2b64` and `hex2bin` be the same code path.

use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    /// A positional base, 2..=36.
    Positional(u32),
    /// Text: Unicode scalars, or raw bytes.
    Char,
    /// Standard base64 (RFC 4648, padded).
    B64,
    /// URL-safe base64.
    B64Url,
    /// Standard base32.
    B32,
    /// base32hex (extended hex alphabet).
    B32Hex,
    /// Escaped hex, the `He` notation payloads are written in.
    Esc,
}

impl Format {
    pub const BIN: Format = Format::Positional(2);
    pub const OCT: Format = Format::Positional(8);
    pub const DEC: Format = Format::Positional(10);
    pub const HEX: Format = Format::Positional(16);

    /// The radix, if this is a real positional base.
    pub fn radix(self) -> Option<u32> {
        match self {
            Format::Positional(r) => Some(r),
            _ => None,
        }
    }

    pub fn is_positional(self) -> bool {
        matches!(self, Format::Positional(_))
    }

    /// Bits per digit, but only for bases that are exact powers of two.
    ///
    /// This is the hinge of the whole design: when both sides answer `Some`,
    /// conversion is pure bit regrouping (no bignum, leading zeros preserved),
    /// and a digit count translates directly into a byte width.
    pub fn bits_per_digit(self) -> Option<u32> {
        let r = self.radix()?;
        if r.is_power_of_two() {
            Some(r.trailing_zeros())
        } else {
            None
        }
    }

    /// True when a digit boundary can also be a byte boundary.
    ///
    /// Bases 2, 4 and 16 pack a whole number of digits into a byte; base 8
    /// (3 bits) and base 32 (5 bits) do not. That matters for byte order: in a
    /// byte-aligned base the digits you wrote *are* a byte count, and
    /// everywhere else the width has to be supplied with `--word`.
    pub fn byte_aligned(self) -> bool {
        matches!(self.bits_per_digit(), Some(b) if 8 % b == 0)
    }

    /// True for formats whose natural currency is bytes rather than a number.
    pub fn is_byteish(self) -> bool {
        !self.is_positional()
    }

    /// True for the base64/base32 family only.
    ///
    /// This is not "is it a byte encoding" — it is "does it need to see the
    /// whole stream before it can emit", which is what padding requires.
    /// `esc` is a byte encoding that streams a byte at a time, so it is
    /// deliberately excluded.
    pub fn is_encoding(self) -> bool {
        matches!(
            self,
            Format::B64 | Format::B64Url | Format::B32 | Format::B32Hex
        )
    }

    /// Parse a format name: a word (`hex`), an alias (`ascii`), or a bare
    /// radix (`16`, `36`).
    pub fn parse(s: &str) -> Option<Format> {
        let lower = s.to_ascii_lowercase();
        let f = match lower.as_str() {
            "bin" | "b" | "binary" => Format::BIN,
            "oct" | "o" | "octal" => Format::OCT,
            "dec" | "d" | "decimal" | "int" => Format::DEC,
            "hex" | "h" | "hexadecimal" => Format::HEX,
            "char" | "chr" | "ascii" | "text" | "str" | "raw" | "utf8" => Format::Char,
            "b64" | "base64" => Format::B64,
            "b64url" | "base64url" | "b64u" => Format::B64Url,
            "b32" | "base32" => Format::B32,
            "b32hex" | "base32hex" => Format::B32Hex,
            "esc" | "escape" | "fhex" | "xhex" => Format::Esc,
            _ => {
                let r: u32 = lower.parse().ok()?;
                if (2..=36).contains(&r) {
                    Format::Positional(r)
                } else {
                    return None;
                }
            }
        };
        Some(f)
    }

    /// Every word spelling [`parse`](Format::parse) accepts.
    ///
    /// It exists so tests can assert properties over the whole set — in
    /// particular that none of them starts with `le` or `be`, and that none
    /// holds both a `2` and an `l`. Those two facts are what make the endian
    /// prefixes in command names unambiguous; see `names`.
    pub const SPELLINGS: [&'static str; 33] = [
        "bin", "b", "binary", "oct", "o", "octal", "dec", "d", "decimal", "int", "hex", "h",
        "hexadecimal", "char", "chr", "ascii", "text", "str", "raw", "utf8", "b64", "base64",
        "b64url", "base64url", "b64u", "b32", "base32", "b32hex", "base32hex",
        "esc", "escape", "fhex", "xhex",
    ];

    /// The canonical short name, as used in command names.
    pub fn name(self) -> &'static str {
        match self {
            Format::Positional(2) => "bin",
            Format::Positional(8) => "oct",
            Format::Positional(10) => "dec",
            Format::Positional(16) => "hex",
            Format::Positional(_) => "base",
            Format::Char => "char",
            Format::B64 => "b64",
            Format::B64Url => "b64url",
            Format::B32 => "b32",
            Format::B32Hex => "b32hex",
            Format::Esc => "esc",
        }
    }
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Format::Positional(r) if self.name() == "base" => write!(f, "base{r}"),
            other => write!(f, "{}", other.name()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_words_aliases_and_radices() {
        assert_eq!(Format::parse("hex"), Some(Format::HEX));
        assert_eq!(Format::parse("HEX"), Some(Format::HEX));
        assert_eq!(Format::parse("16"), Some(Format::HEX));
        assert_eq!(Format::parse("ascii"), Some(Format::Char));
        assert_eq!(Format::parse("base64"), Some(Format::B64));
        assert_eq!(Format::parse("36"), Some(Format::Positional(36)));
    }

    #[test]
    fn rejects_out_of_range_radices() {
        assert_eq!(Format::parse("1"), None);
        assert_eq!(Format::parse("37"), None);
        assert_eq!(Format::parse("0"), None);
        assert_eq!(Format::parse("nonsense"), None);
    }

    #[test]
    fn bits_per_digit_only_for_powers_of_two() {
        assert_eq!(Format::BIN.bits_per_digit(), Some(1));
        assert_eq!(Format::OCT.bits_per_digit(), Some(3));
        assert_eq!(Format::HEX.bits_per_digit(), Some(4));
        assert_eq!(Format::Positional(32).bits_per_digit(), Some(5));
        assert_eq!(Format::DEC.bits_per_digit(), None);
        assert_eq!(Format::Positional(36).bits_per_digit(), None);
        assert_eq!(Format::Char.bits_per_digit(), None);
    }

    /// `SPELLINGS` is a hand-written copy of the `match` in `parse`, so it can
    /// drift. This is what stops it.
    #[test]
    fn byte_alignment_follows_the_bit_width() {
        // 8, 4 and 2 digits to the byte.
        assert!(Format::BIN.byte_aligned());
        assert!(Format::Positional(4).byte_aligned());
        assert!(Format::HEX.byte_aligned());
        // 3 and 5 bits per digit never land on a byte boundary.
        assert!(!Format::OCT.byte_aligned());
        assert!(!Format::Positional(32).byte_aligned());
        // Not a power of two, so not even a bit width.
        assert!(!Format::DEC.byte_aligned());
        assert!(!Format::Char.byte_aligned());
    }

    #[test]
    fn every_spelling_parses() {
        for s in Format::SPELLINGS {
            assert!(Format::parse(s).is_some(), "`{s}` is listed but does not parse");
        }
    }

    #[test]
    fn display_names_arbitrary_bases() {
        assert_eq!(Format::HEX.to_string(), "hex");
        assert_eq!(Format::Positional(36).to_string(), "base36");
    }
}
