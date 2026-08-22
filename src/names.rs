//! The command-name table: the single source of truth for what `hex2bin`,
//! `dec2ascii` and `1628` mean, and for which shims `install` creates.

use crate::endian::Endpoint;
use crate::format::Format;

/// Formats that take part in the generated `<from>2<to>` shim cross product.
pub const SHIM_FORMATS: [Format; 8] = [
    Format::BIN,
    Format::OCT,
    Format::DEC,
    Format::HEX,
    Format::Char,
    Format::B64,
    Format::B32,
    Format::Esc,
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

/// The little-endian shims worth putting on `PATH`.
///
/// Deliberately a hand-picked list rather than a cross product. `le`/`be` on
/// both sides of seven formats is 420 files, and `be` is the identity anyway,
/// so no `be` name ever earns one. These fourteen are the conversions that come
/// up reading a value out of a memory dump or packing one back into it.
///
/// Everything else still resolves — `f2t leb642hex`, `f2t behex2bin` — it just
/// does not get a file. That is the same bargain already struck with the
/// numeric shorthands above.
const LE_SHIMS: [(&str, Endpoint, Endpoint); 14] = [
    // Reading little-endian bytes.
    ("lehex2hex", Endpoint::little(Format::HEX), Endpoint::big(Format::HEX)),
    ("lehex2dec", Endpoint::little(Format::HEX), Endpoint::big(Format::DEC)),
    ("lehex2bin", Endpoint::little(Format::HEX), Endpoint::big(Format::BIN)),
    ("lehex2char", Endpoint::little(Format::HEX), Endpoint::big(Format::Char)),
    ("lebin2hex", Endpoint::little(Format::BIN), Endpoint::big(Format::HEX)),
    ("lebin2dec", Endpoint::little(Format::BIN), Endpoint::big(Format::DEC)),
    // Needs --word: base 10 carries no byte width of its own.
    ("ledec2hex", Endpoint::little(Format::DEC), Endpoint::big(Format::HEX)),
    // Writing little-endian bytes.
    ("hex2lehex", Endpoint::big(Format::HEX), Endpoint::little(Format::HEX)),
    ("hex2lebin", Endpoint::big(Format::HEX), Endpoint::little(Format::BIN)),
    ("hex2ledec", Endpoint::big(Format::HEX), Endpoint::little(Format::DEC)),
    ("bin2lehex", Endpoint::big(Format::BIN), Endpoint::little(Format::HEX)),
    ("char2lehex", Endpoint::big(Format::Char), Endpoint::little(Format::HEX)),
    // Both need --word, for the same reason as ledec2hex.
    ("dec2lehex", Endpoint::big(Format::DEC), Endpoint::little(Format::HEX)),
    ("dec2lebin", Endpoint::big(Format::DEC), Endpoint::little(Format::BIN)),
];

/// The numeric shorthand table, e.g. `1628` = hex to oct.
///
/// Only bases {2, 8, 10, 16} appear here, and that restriction is load-bearing:
/// with `2` doing double duty as both a base and the "to" separator, every one
/// of these 16 strings still has exactly one valid `<from>2<to>` split. Adding
/// any further base breaks that (`216` would read as both 2->16 and 21->6), so
/// this table must not be generalized.
///
/// These are understood by [`resolve`] but deliberately *not* installed as
/// commands: a file called `822` in a directory on `PATH` is a poor citizen,
/// and it is the one name shape a package manager would rightly object to.
/// `f2t 1628 ff` works whether or not anything was installed.
pub fn shorthands() -> Vec<(String, Endpoint, Endpoint)> {
    let mut out = Vec::new();
    for (from, fd) in CORE_BASES {
        for (to, td) in CORE_BASES {
            out.push((format!("{fd}2{td}"), Endpoint::big(from), Endpoint::big(to)));
        }
    }
    out
}

/// Every command name `install` should create a shim for.
///
/// Word names only. The numeric shorthands from [`shorthands`] are understood
/// by [`resolve`] but never written to disk — see that function for why.
pub fn shim_names() -> Vec<(String, Endpoint, Endpoint)> {
    let mut out = Vec::new();

    // The word cross product, minus the no-op identity pairs.
    for from in SHIM_FORMATS {
        for to in SHIM_FORMATS {
            if from != to {
                out.push((
                    format!("{}2{}", from.name(), to.name()),
                    Endpoint::big(from),
                    Endpoint::big(to),
                ));
            }
        }
    }

    // `ascii` reads better than `char` in some directions; both spellings work.
    for (base, _) in CORE_BASES {
        out.push((
            format!("{}2ascii", base.name()),
            Endpoint::big(base),
            Endpoint::big(Format::Char),
        ));
        out.push((
            format!("ascii2{}", base.name()),
            Endpoint::big(Format::Char),
            Endpoint::big(base),
        ));
    }

    // The curated little-endian set. Not a cross product — see LE_SHIMS.
    for (name, from, to) in LE_SHIMS {
        out.push((name.to_string(), from, to));
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
pub fn resolve(cmd: &str) -> Option<(Endpoint, Endpoint)> {
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
    // name an endpoint. This is what makes `b642hex`, `dec2ascii` and
    // `lehex2bin` work without being listed anywhere.
    let mut found = None;
    for (i, ch) in cmd.char_indices() {
        if ch != '2' {
            continue;
        }
        let (left, right) = (&cmd[..i], &cmd[i + 1..]);
        if let (Some(f), Some(t)) = (Endpoint::parse(left), Endpoint::parse(right)) {
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
    use crate::endian::Endian;
    use std::collections::HashMap;

    #[test]
    fn resolves_word_names() {
        let big = |f, t| Some((Endpoint::big(f), Endpoint::big(t)));
        assert_eq!(resolve("hex2bin"), big(Format::HEX, Format::BIN));
        assert_eq!(resolve("dec2char"), big(Format::DEC, Format::Char));
        assert_eq!(resolve("dec2ascii"), big(Format::DEC, Format::Char));
        assert_eq!(resolve("b642hex"), big(Format::B64, Format::HEX));
        assert_eq!(resolve("char2b32"), big(Format::Char, Format::B32));
    }

    #[test]
    fn resolves_numeric_shorthands() {
        let big = |f, t| Some((Endpoint::big(f), Endpoint::big(t)));
        assert_eq!(resolve("1628"), big(Format::HEX, Format::OCT));
        assert_eq!(resolve("10216"), big(Format::DEC, Format::HEX));
        assert_eq!(resolve("2216"), big(Format::BIN, Format::HEX));
        assert_eq!(resolve("822"), big(Format::OCT, Format::BIN));
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
        // 56 word pairs + 8 ascii aliases + 14 little-endian. The numeric
        // shorthands resolve but are not installed.
        assert_eq!(names.len(), 78);
    }

    /// The shorthands must keep working through `resolve` even though nothing
    /// installs them, or `f2t 1628 ff` breaks for everyone who learned it.
    #[test]
    fn shorthands_resolve_but_are_never_installed() {
        let hex2oct = Some((Endpoint::big(Format::HEX), Endpoint::big(Format::OCT)));
        assert_eq!(resolve("1628"), hex2oct);
        let installed = shim_names();
        for (name, _, _) in shorthands() {
            assert!(
                !installed.iter().any(|(n, _, _)| *n == name),
                "{name} must not be installed as a command"
            );
        }
    }

    #[test]
    fn resolves_endian_prefixes() {
        let l = Endpoint::little;
        let b = Endpoint::big;
        assert_eq!(resolve("lehex2bin"), Some((l(Format::HEX), b(Format::BIN))));
        assert_eq!(resolve("hex2lebin"), Some((b(Format::HEX), l(Format::BIN))));
        assert_eq!(resolve("lehex2ledec"), Some((l(Format::HEX), l(Format::DEC))));
        assert_eq!(resolve("leb642hex"), Some((l(Format::B64), b(Format::HEX))));
        assert_eq!(resolve("char2lehex"), Some((b(Format::Char), l(Format::HEX))));
        // `be` is the identity, spelled out.
        assert_eq!(resolve("behex2dec"), Some((b(Format::HEX), b(Format::DEC))));
        // The numeric shorthands pick up prefixes for free, via the split rule.
        assert_eq!(resolve("le1628"), Some((l(Format::HEX), b(Format::OCT))));
    }

    /// A bare `le` says nothing about *what* is little-endian. Letting it
    /// through would make `be22hex` read two ways; see `Endpoint::parse`.
    #[test]
    fn a_bare_endian_prefix_is_not_a_conversion() {
        for cmd in ["le2hex", "hex2le", "le2be", "be2be", "2le", "le2"] {
            assert_eq!(resolve(cmd), None, "`{cmd}` must not resolve");
        }
    }

    /// Every valid `<from>2<to>` reading of a name, under the current rules.
    fn all_splits(cmd: &str) -> Vec<(Endpoint, Endpoint)> {
        cmd.char_indices()
            .filter(|(_, c)| *c == '2')
            .filter_map(|(i, _)| {
                Some((Endpoint::parse(&cmd[..i])?, Endpoint::parse(&cmd[i + 1..])?))
            })
            .collect()
    }

    /// The property the entire endian syntax rests on: an `le`/`be` prefix is
    /// *transparent* to the splitting rule, so decorating a name that worked
    /// yields a name that still works and means the same thing.
    ///
    /// It holds because no format spelling starts with `le`/`be`, and none
    /// contains both a `2` and an `l`. Those two facts are asserted first,
    /// because if either stops being true then some existing name has quietly
    /// gone ambiguous — and this is where that should surface, rather than in
    /// a bug report.
    #[test]
    fn an_endian_prefix_never_changes_how_a_name_splits() {
        for s in Format::SPELLINGS {
            assert!(
                !s.starts_with("le") && !s.starts_with("be"),
                "the spelling `{s}` shadows an endian prefix"
            );
            assert!(
                !(s.contains('2') && s.contains('l')),
                "the spelling `{s}` holds both a 2 and an l, which breaks the proof"
            );
        }

        // Only undecorated names: a prefix does not stack onto one that
        // already carries it, which `prefixes_do_not_stack` pins down.
        let names: Vec<String> = shim_names()
            .into_iter()
            .chain(shorthands())
            .filter(|(_, f, t)| f.endian == Endian::Big && t.endian == Endian::Big)
            .map(|(n, _, _)| n)
            .collect();

        for name in &names {
            let plain = all_splits(name);
            assert_eq!(plain.len(), 1, "{name} already splits {} ways", plain.len());
            let i = name
                .char_indices()
                .filter(|(_, c)| *c == '2')
                .find(|&(i, _)| {
                    Endpoint::parse(&name[..i]).is_some()
                        && Endpoint::parse(&name[i + 1..]).is_some()
                })
                .map(|(i, _)| i)
                .unwrap();
            let (l, r) = (&name[..i], &name[i + 1..]);

            for (decorated, want) in [
                (format!("le{l}2{r}"), (Endian::Little, Endian::Big)),
                (format!("be{l}2{r}"), (Endian::Big, Endian::Big)),
                (format!("{l}2le{r}"), (Endian::Big, Endian::Little)),
                (format!("{l}2be{r}"), (Endian::Big, Endian::Big)),
                (format!("le{l}2le{r}"), (Endian::Little, Endian::Little)),
                (format!("be{l}2le{r}"), (Endian::Big, Endian::Little)),
            ] {
                let got = all_splits(&decorated);
                assert_eq!(got.len(), 1, "{decorated} splits {} ways: {got:?}", got.len());
                assert_eq!(got[0].0.format, plain[0].0.format, "{decorated} from");
                assert_eq!(got[0].1.format, plain[0].1.format, "{decorated} to");
                assert_eq!((got[0].0.endian, got[0].1.endian), want, "{decorated} endian");
                assert_eq!(resolve(&decorated), Some(got[0]), "{decorated} resolved wrong");
            }
        }
    }

    /// One prefix per endpoint. `lelehex` is not "extra little-endian", it is
    /// nothing at all — and a swap that applied twice would be the identity
    /// anyway, so there is nothing to express.
    #[test]
    fn prefixes_do_not_stack() {
        for cmd in ["lelehex2hex", "beleb2hex", "hex2lelebin"] {
            assert_eq!(resolve(cmd), None, "`{cmd}` must not resolve");
        }
    }

    /// `be` is the identity, so a `be*` file on `PATH` would earn nothing.
    #[test]
    fn only_little_endian_shims_are_installed() {
        for (name, from, to) in shim_names() {
            assert!(
                !name.starts_with("be") && !name.contains("2be"),
                "{name}: `be` is the identity and never earns a file"
            );
            assert_eq!(resolve(&name), Some((from, to)), "{name} resolved wrong");
        }
    }

    #[test]
    fn basename_strips_path_and_extension() {
        assert_eq!(basename("/usr/local/bin/dec2char"), "dec2char");
        assert_eq!(basename("hex2bin"), "hex2bin");
        assert_eq!(basename("HEX2BIN.EXE"), "hex2bin");

        // `Path` splits on the *host's* separator, so a Windows path only
        // reads as one on Windows. On Unix a backslash is an ordinary
        // filename character, and `argv[0]` there would never carry one —
        // so asserting the Windows shape everywhere tested the host, not us.
        #[cfg(windows)]
        assert_eq!(basename(r"C:\Users\x\.cargo\bin\hex2bin.exe"), "hex2bin");
        #[cfg(not(windows))]
        assert_eq!(basename("/home/you/.cargo/bin/hex2bin"), "hex2bin");
    }
}
