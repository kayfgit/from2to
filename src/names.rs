//! The command-name table: the single source of truth for what `hex2bin`,
//! `dec2ascii` and `1628` mean, and for which shims `install` creates.

use crate::format::Format;

/// Formats that take part in the generated `<from>2<to>` shim cross product.
pub const SHIM_FORMATS: [Format; 7] = [
    Format::BIN,
    Format::OCT,
    Format::DEC,
    Format::HEX,
    Format::Char,
    Format::B64,
    Format::B32,
];

/// Positional bases that get the `ascii` spelling and the numeric shorthands.
const CORE_BASES: [(Format, &str); 4] = [
    (Format::BIN, "2"),
    (Format::OCT, "8"),
    (Format::DEC, "10"),
    (Format::HEX, "16"),
];

/// Names the binary answers to as itself rather than as a conversion.
///
/// `from2to` happens to match the `<from>2<to>` shape it dispatches on, so it
/// is listed here explicitly rather than left to the splitting rule.
pub const SELF_NAMES: [&str; 2] = ["from2to", "f2t"];

/// The short name installed alongside the conversions.
pub const SHORT_NAME: &str = "f2t";

/// The one installed command that is not a conversion: it identifies a format
/// instead of converting between two. It has no `2` in it, so the splitting
/// rule below never claims it.
pub const DETECT_NAME: &str = "whatbase";

/// The numeric shorthand table, e.g. `1628` = hex to oct.
///
/// Only bases {2, 8, 10, 16} appear here, and that restriction is load-bearing:
/// with `2` doing double duty as both a base and the "to" separator, every one
/// of these 16 strings still has exactly one valid `<from>2<to>` split. Adding
/// any further base breaks that (`216` would read as both 2->16 and 21->6), so
/// this table must not be generalized.
pub fn shorthands() -> Vec<(String, Format, Format)> {
    let mut out = Vec::new();
    for (from, fd) in CORE_BASES {
        for (to, td) in CORE_BASES {
            out.push((format!("{fd}2{td}"), from, to));
        }
    }
    out
}

/// Every command name `install` should create a shim for.
pub fn shim_names() -> Vec<(String, Format, Format)> {
    let mut out = Vec::new();

    // The word cross product, minus the no-op identity pairs.
    for from in SHIM_FORMATS {
        for to in SHIM_FORMATS {
            if from != to {
                out.push((format!("{}2{}", from.name(), to.name()), from, to));
            }
        }
    }

    // `ascii` reads better than `char` in some directions; both spellings work.
    for (base, _) in CORE_BASES {
        out.push((format!("{}2ascii", base.name()), base, Format::Char));
        out.push((format!("ascii2{}", base.name()), Format::Char, base));
    }

    // Numeric shorthands, identity pairs dropped as above.
    for (name, from, to) in shorthands() {
        if from != to {
            out.push((name, from, to));
        }
    }

    out
}

/// Strip directory and extension, leaving the bare command name.
pub fn basename(argv0: &str) -> String {
    let path = std::path::Path::new(argv0);
    path.file_stem()
        .map(|s| s.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_else(|| argv0.to_ascii_lowercase())
}

/// Resolve a command name to the pair of formats it converts between.
///
/// Returns `None` for the binary's own names, and for anything unrecognized.
pub fn resolve(cmd: &str) -> Option<(Format, Format)> {
    let cmd = cmd.to_ascii_lowercase();
    if SELF_NAMES.contains(&cmd.as_str()) {
        return None;
    }

    // Shorthands first: they are an explicit table, so they never depend on
    // the generic splitting rule below.
    for (name, from, to) in shorthands() {
        if name == cmd {
            return Some((from, to));
        }
    }

    // Otherwise split on `2` and require exactly one reading where both halves
    // name a format. This is what makes `b642hex` and `dec2ascii` work without
    // being listed anywhere.
    let mut found = None;
    for (i, ch) in cmd.char_indices() {
        if ch != '2' {
            continue;
        }
        let (left, right) = (&cmd[..i], &cmd[i + 1..]);
        if let (Some(f), Some(t)) = (Format::parse(left), Format::parse(right)) {
            if found.is_some() {
                return None; // ambiguous, refuse to guess
            }
            found = Some((f, t));
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn resolves_word_names() {
        assert_eq!(resolve("hex2bin"), Some((Format::HEX, Format::BIN)));
        assert_eq!(resolve("dec2char"), Some((Format::DEC, Format::Char)));
        assert_eq!(resolve("dec2ascii"), Some((Format::DEC, Format::Char)));
        assert_eq!(resolve("b642hex"), Some((Format::B64, Format::HEX)));
        assert_eq!(resolve("char2b32"), Some((Format::Char, Format::B32)));
    }

    #[test]
    fn resolves_numeric_shorthands() {
        assert_eq!(resolve("1628"), Some((Format::HEX, Format::OCT)));
        assert_eq!(resolve("10216"), Some((Format::DEC, Format::HEX)));
        assert_eq!(resolve("2216"), Some((Format::BIN, Format::HEX)));
        assert_eq!(resolve("822"), Some((Format::OCT, Format::BIN)));
    }

    #[test]
    fn self_names_are_not_conversions() {
        assert_eq!(resolve("f2t"), None);
        assert_eq!(resolve("from2to"), None);
        // The detector is dispatched on separately; it must never look like a
        // conversion, or it would be handled as one.
        assert_eq!(resolve(DETECT_NAME), None);
    }

    /// The property the shorthand table depends on. If this ever fails, the
    /// table has been extended past what the encoding can support.
    #[test]
    fn every_shorthand_has_exactly_one_parse() {
        let bases = ["2", "8", "10", "16"];
        for (name, from, to) in shorthands() {
            let parses: Vec<_> = name
                .char_indices()
                .filter(|(_, c)| *c == '2')
                .filter_map(|(i, _)| {
                    let (l, r) = (&name[..i], &name[i + 1..]);
                    (bases.contains(&l) && bases.contains(&r)).then(|| (l.to_string(), r.to_string()))
                })
                .collect();
            assert_eq!(parses.len(), 1, "{name} has {} parses: {parses:?}", parses.len());
            assert_eq!(resolve(&name), Some((from, to)), "{name} resolved wrong");
        }
    }

    #[test]
    fn shim_names_are_unique_and_resolve_back() {
        let names = shim_names();
        let mut seen: HashMap<&str, ()> = HashMap::new();
        for (name, from, to) in &names {
            assert!(seen.insert(name, ()).is_none(), "duplicate shim {name}");
            assert_eq!(resolve(name), Some((*from, *to)), "{name} resolved wrong");
        }
        // 42 word pairs + 8 ascii aliases + 12 shorthands
        assert_eq!(names.len(), 62);
    }

    #[test]
    fn basename_strips_path_and_extension() {
        assert_eq!(basename(r"C:\Users\x\.cargo\bin\hex2bin.exe"), "hex2bin");
        assert_eq!(basename("/usr/local/bin/dec2char"), "dec2char");
        assert_eq!(basename("HEX2BIN.EXE"), "hex2bin");
    }
}
