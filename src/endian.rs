//! Byte order, which is orthogonal to `Format`.
//!
//! A [`Format`] says how bytes are *written*; an [`Endian`] says what order
//! they are written *in*. Keeping the two apart is what lets `detect` and the
//! `encoding_for` / `encoding_shape` / `in_alphabet` catch-alls go on assuming
//! that every non-positional format is a byte encoding.

use std::fmt;

use crate::format::Format;

/// Which end the most significant byte sits at.
///
/// There is deliberately no `Native`: nothing here ever reads a host integer,
/// so a result that depended on the machine it ran on would be a bug rather
/// than a feature. Big-endian is the default because it is the order the
/// digits are written in.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Endian {
    #[default]
    Big,
    Little,
}

impl Endian {
    pub fn is_little(self) -> bool {
        self == Endian::Little
    }
}

/// One end of a conversion: a format, and the order its bytes arrive in.
///
/// This is a *names-layer* type. It exists to carry what `lehex2bin` meant
/// from the command name to the converter, which takes the two apart again —
/// the byte order lands in `Options`, so `Converter::new` keeps its shape.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Endpoint {
    pub format: Format,
    pub endian: Endian,
}

impl Endpoint {
    pub const fn big(format: Format) -> Endpoint {
        Endpoint { format, endian: Endian::Big }
    }

    pub const fn little(format: Format) -> Endpoint {
        Endpoint { format, endian: Endian::Little }
    }

    /// Parse one half of a command name: a format, optionally carrying an
    /// `le`/`be` prefix.
    ///
    /// The whole string is tried as a format *first*. No spelling currently
    /// begins with `le` or `be`, so the two readings can never both succeed
    /// and the order is documentation rather than a tiebreak — but it is what
    /// keeps a future `be`-spelled format from silently changing an existing
    /// name's meaning.
    ///
    /// A bare `le` or `be` is **not** an endpoint. That is load-bearing, not
    /// fussiness: if it were, `be22hex` would read both as `be`/`22hex` and as
    /// `be2`/`hex`, and [`crate::names::resolve`] would refuse the lot as
    /// ambiguous. See the property test in `names`.
    pub fn parse(s: &str) -> Option<Endpoint> {
        let lower = s.to_ascii_lowercase();
        if let Some(f) = Format::parse(&lower) {
            return Some(Endpoint::big(f));
        }
        for (prefix, endian) in [("le", Endian::Little), ("be", Endian::Big)] {
            if let Some(rest) = lower.strip_prefix(prefix).filter(|r| !r.is_empty()) {
                if let Some(format) = Format::parse(rest) {
                    return Some(Endpoint { format, endian });
                }
            }
        }
        None
    }

    /// The spelling that names this endpoint in a command, e.g. `lehex`.
    pub fn command_name(self) -> String {
        match self.endian {
            Endian::Big => self.format.to_string(),
            Endian::Little => format!("le{}", self.format),
        }
    }
}

/// Prose, for help text and `from2to list` — not the command spelling.
impl fmt::Display for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.endian {
            Endian::Big => write!(f, "{}", self.format),
            Endian::Little => write!(f, "little-endian {}", self.format),
        }
    }
}

/// Reverse the bytes within each `word`-sized chunk, or within the whole slice
/// when no word size is pinned.
///
/// Returns the length of a ragged final chunk, which has been swapped among
/// itself; the caller decides whether that is an error.
pub fn swap_words(bytes: &mut [u8], word: Option<usize>) -> Result<(), usize> {
    match word.filter(|w| *w > 0) {
        None => {
            bytes.reverse();
            Ok(())
        }
        Some(w) => {
            let tail = bytes.len() % w;
            for chunk in bytes.chunks_mut(w) {
                chunk.reverse();
            }
            if tail == 0 { Ok(()) } else { Err(tail) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_prefixed_format() {
        assert_eq!(Endpoint::parse("lehex"), Some(Endpoint::little(Format::HEX)));
        assert_eq!(Endpoint::parse("behex"), Some(Endpoint::big(Format::HEX)));
        assert_eq!(Endpoint::parse("hex"), Some(Endpoint::big(Format::HEX)));
        assert_eq!(Endpoint::parse("LEHEX"), Some(Endpoint::little(Format::HEX)));
        assert_eq!(Endpoint::parse("leb64"), Some(Endpoint::little(Format::B64)));
        // `b` is an alias for bin, so `leb` is little-endian binary.
        assert_eq!(Endpoint::parse("leb"), Some(Endpoint::little(Format::BIN)));
        // Bare radices take a prefix too.
        assert_eq!(Endpoint::parse("le16"), Some(Endpoint::little(Format::HEX)));
    }

    /// The guard rule the whole naming scheme rests on.
    #[test]
    fn a_bare_prefix_is_not_an_endpoint() {
        for bare in ["", "le", "be", "l", "e", "b3", "leb3", "nonsense"] {
            assert_eq!(Endpoint::parse(bare), None, "`{bare}` must not be an endpoint");
        }
    }

    #[test]
    fn displays_as_prose_but_names_as_a_command() {
        assert_eq!(Endpoint::little(Format::HEX).to_string(), "little-endian hex");
        assert_eq!(Endpoint::big(Format::HEX).to_string(), "hex");
        assert_eq!(Endpoint::little(Format::HEX).command_name(), "lehex");
        assert_eq!(Endpoint::big(Format::HEX).command_name(), "hex");
    }

    #[test]
    fn swap_words_reverses_within_each_chunk() {
        let mut b = [1, 2, 3, 4];
        assert_eq!(swap_words(&mut b, Some(2)), Ok(()));
        assert_eq!(b, [2, 1, 4, 3]);

        let mut b = [1, 2, 3, 4];
        assert_eq!(swap_words(&mut b, None), Ok(()));
        assert_eq!(b, [4, 3, 2, 1]);

        // A ragged tail is reported, not hidden.
        let mut b = [1, 2, 3];
        assert_eq!(swap_words(&mut b, Some(2)), Err(1));
        assert_eq!(b, [2, 1, 3]);

        // A word size of zero means "unset", matching how --group is handled.
        let mut b = [1, 2, 3];
        assert_eq!(swap_words(&mut b, Some(0)), Ok(()));
        assert_eq!(b, [3, 2, 1]);
    }
}
