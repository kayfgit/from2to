//! Argument definitions.
//!
//! The same flag set applies whether the binary was invoked through a shim
//! (`hex2bin -u ff`) or by its own name (`f2t hex bin -u ff`); only the
//! positional arguments differ, so the command is built in two shapes.

use clap::{Arg, ArgAction, ArgMatches, Command};

use crate::convert::Options;
use crate::endian::Endian;

const AFTER_HELP: &str = "\
EXAMPLES:
    hex2bin ff                    11111111
    hex2bin 0f                    00001111   (digit width is preserved)
    hex2bin -W 0f                 1111       (bare numeric)
    char2hex hello                68656c6c6f
    hex2b64 68656c6c6f            aGVsbG8=
    f2t 36 10 zz                  1295       (any base 2-36)
    f2t 1628 ff                   377        (numeric shorthand: 16 to 8)
    whatbase bDNhcm5fdGgzX3IwcDM1 b64        (when you do not know the base)

    lehex2dec 0d0c0b0a            168496141  (a word out of a memory dump)
    dec2lehex --word 4 1234       d2040000   (pack one back)

    hex2hex '4438+1a'             4452       (math, in the source base)
    hex2hex --word 2 '4438-4500'  ff38       (in a 16-bit register)

    nc example.com 4000 | dec2char
        decode a stream of decimal code points as it arrives

Input comes from the arguments, or from stdin when none are given.
Output streams as it is produced, except for base64/base32 targets, which
must see the whole input before they can pad.";

/// Every flag that is common to both invocation shapes.
fn common_args(cmd: Command) -> Command {
    cmd.arg(
        Arg::new("width")
            .short('w')
            .long("width")
            .value_name("N")
            .value_parser(clap::value_parser!(usize))
            .help("Pad each result to N digits"),
    )
    .arg(
        Arg::new("no-width")
            .short('W')
            .long("no-width")
            .action(ArgAction::SetTrue)
            .help("Drop the natural digit width; emit bare numbers"),
    )
    .arg(
        Arg::new("group")
            .short('g')
            .long("group")
            .value_name("N")
            .value_parser(clap::value_parser!(usize))
            .help("Group output digits in runs of N"),
    )
    .arg(
        Arg::new("word")
            .long("word")
            .value_name("BYTES")
            .value_parser(clap::value_parser!(usize))
            .help("Bytes per word, for the le/be command names"),
    )
    .arg(
        Arg::new("upper")
            .short('u')
            .long("upper")
            .action(ArgAction::SetTrue)
            .help("Uppercase the output digits"),
    )
    .arg(
        Arg::new("prefix")
            .short('x')
            .long("prefix")
            .action(ArgAction::SetTrue)
            .help("Prefix results with 0x / 0b / 0o"),
    )
    .arg(
        Arg::new("delim")
            .short('d')
            .long("delim")
            .value_name("S")
            .help("Separator between results"),
    )
    .arg(
        Arg::new("join")
            .short('j')
            .long("join")
            .action(ArgAction::SetTrue)
            .conflicts_with("delim")
            .help("Run results together with no separator"),
    )
    .arg(
        Arg::new("bytes")
            .short('b')
            .long("bytes")
            .action(ArgAction::SetTrue)
            .help("Treat char as raw bytes"),
    )
    .arg(
        Arg::new("codepoint")
            .short('c')
            .long("codepoint")
            .action(ArgAction::SetTrue)
            .conflicts_with("bytes")
            .help("Treat char as Unicode code points"),
    )
    .arg(
        Arg::new("keep-going")
            .short('k')
            .long("keep-going")
            .action(ArgAction::SetTrue)
            .help("Skip tokens that fail instead of stopping"),
    )
    .arg(
        Arg::new("quiet")
            .short('q')
            .long("quiet")
            .action(ArgAction::SetTrue)
            .help("Suppress warnings"),
    )
    .arg(
        Arg::new("no-newline")
            .short('n')
            .long("no-newline")
            .action(ArgAction::SetTrue)
            .help("Do not add a trailing newline"),
    )
    .arg(
        Arg::new("strict")
            .long("strict")
            .action(ArgAction::SetTrue)
            .help("Reject 0x/0b/0o prefixes, _ spacers and unpadded base64"),
    )
    .arg(
        Arg::new("values")
            .num_args(0..)
            .value_name("VALUE")
            .help("Values to convert; omit to read stdin"),
    )
}

const DETECT_ABOUT: &str = "Work out what format a piece of text is in";

const DETECT_AFTER_HELP: &str = "\
EXAMPLES:
    whatbase bDNhcm5fdGgzX3IwcDM1     b64 96%   l3arn_th3_r0p35
    whatbase -d bDNhcm5fdGgzX3IwcDM1  l3arn_th3_r0p35
    whatbase -q bDNhcm5fdGgzX3IwcDM1  b64
    whatbase -a 01010101              every reading, worst included

Input comes from the arguments, or from stdin when none are given. The score
is a confidence, and only means anything next to the other candidates.";

/// The detector's flags, shared by the `whatbase` shim and `f2t detect` so the
/// two cannot drift apart.
fn detect_args(cmd: Command) -> Command {
    cmd.arg(
        Arg::new("all")
            .short('a')
            .long("all")
            .action(ArgAction::SetTrue)
            .help("Show every candidate, not just the likely ones"),
    )
    .arg(
        Arg::new("why")
            .short('w')
            .long("why")
            .action(ArgAction::SetTrue)
            .help("Show the reasoning, the scores and every candidate"),
    )
    .arg(
        Arg::new("decode")
            .short('d')
            .long("decode")
            .action(ArgAction::SetTrue)
            .help("Print what the best guess decodes to, and nothing else"),
    )
    .arg(
        Arg::new("name")
            .short('q')
            .long("quiet")
            .action(ArgAction::SetTrue)
            .conflicts_with("decode")
            .help("Print just the format name, for scripts"),
    )
    .arg(
        Arg::new("to")
            .long("to")
            .value_name("FMT")
            .help("With --decode, the format to decode into (default: char)"),
    )
    .arg(
        Arg::new("values")
            .num_args(0..)
            .value_name("TEXT")
            .help("Text to identify; omit to read stdin"),
    )
}

/// The detector under its own command name.
pub fn detect_command(invoked_as: &str) -> Command {
    let name: &'static str = Box::leak(invoked_as.to_string().into_boxed_str());
    detect_args(
        Command::new(name)
            .about(DETECT_ABOUT)
            .version(env!("CARGO_PKG_VERSION"))
            .after_help(DETECT_AFTER_HELP),
    )
}

/// The command as seen through a shim: formats are already known.
///
/// clap wants a `'static` name and the process is about to exit either way, so
/// the shim's name is leaked rather than threaded through a lifetime.
pub fn shim_command(name: &str, desc: String) -> Command {
    let name: &'static str = Box::leak(name.to_string().into_boxed_str());
    common_args(
        Command::new(name)
            .about(desc)
            .version(env!("CARGO_PKG_VERSION"))
            .after_help(AFTER_HELP),
    )
}

/// The command under its own name: formats come from the first arguments.
///
/// The name is whichever of `from2to` / `f2t` was actually typed, so `--help`
/// and `--version` echo the invocation rather than a canonical spelling.
pub fn self_command(invoked_as: &str) -> Command {
    let name: &'static str = Box::leak(invoked_as.to_string().into_boxed_str());
    common_args(
        Command::new(name)
            .about("Convert between number bases and byte encodings")
            .version(env!("CARGO_PKG_VERSION"))
            .after_help(AFTER_HELP)
            .arg(
                Arg::new("from")
                    .required(false)
                    .help("Source format (hex, dec, char, b64, or a radix 2-36), or a name like hex2bin"),
            )
            .arg(Arg::new("to").required(false).help("Target format"))
            .subcommand(
                Command::new("install")
                    .about("Create the per-conversion command shims")
                    .arg(Arg::new("dir").long("dir").value_name("PATH").help(
                        "Where to put them (default: next to this binary)",
                    ))
                    .arg(
                        Arg::new("dry-run")
                            .long("dry-run")
                            .action(ArgAction::SetTrue)
                            .help("Show what would be created, change nothing"),
                    )
                    .arg(
                        Arg::new("force")
                            .long("force")
                            .action(ArgAction::SetTrue)
                            .help("Replace existing files that are not ours"),
                    )
                    .arg(
                        Arg::new("method")
                            .long("method")
                            .value_name("HOW")
                            .value_parser(["symlink", "hardlink", "copy"])
                            .help("How to create them (default: symlink on Unix, hardlink on Windows)"),
                    ),
            )
            .subcommand(
                Command::new("uninstall")
                    .about("Remove shims that point at this binary")
                    .arg(Arg::new("dir").long("dir").value_name("PATH").help(
                        "Where to look (default: next to this binary)",
                    ))
                    .arg(
                        Arg::new("dry-run")
                            .long("dry-run")
                            .action(ArgAction::SetTrue)
                            .help("Show what would be removed, change nothing"),
                    ),
            )
            .subcommand(detect_args(
                Command::new("detect")
                    .about(DETECT_ABOUT)
                    .after_help(DETECT_AFTER_HELP),
            ))
            .subcommand(Command::new("list").about("List every command name")),
    )
}

/// Pull the conversion options out of parsed arguments.
pub fn options_from(m: &ArgMatches) -> Options {
    let word = m.get_one::<usize>("word").copied();
    let bytes_mode = if m.get_flag("bytes") {
        Some(true)
    } else if m.get_flag("codepoint") {
        Some(false)
    } else if word.is_some() {
        // A word size is a statement about bytes, so saying it is enough —
        // otherwise `char2lehex --word 4` would refuse itself for no reason.
        Some(true)
    } else {
        None
    };
    let delim = if m.get_flag("join") {
        Some(String::new())
    } else {
        m.get_one::<String>("delim").map(|s| unescape(s))
    };

    Options {
        width: m.get_one::<usize>("width").copied(),
        no_width: m.get_flag("no-width"),
        group: m.get_one::<usize>("group").copied(),
        upper: m.get_flag("upper"),
        prefix: m.get_flag("prefix"),
        delim,
        bytes_mode,
        strict: m.get_flag("strict"),
        keep_going: m.get_flag("keep-going"),
        // Byte order comes from the command name, not from a flag; `main`
        // fills these in once it has resolved the name.
        from_endian: Endian::default(),
        to_endian: Endian::default(),
        word,
    }
}

/// Let `-d '\n'` mean a newline rather than a backslash and an n.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('0') => out.push('\0'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unescapes_common_sequences() {
        assert_eq!(unescape(r"\n"), "\n");
        assert_eq!(unescape(r"\t"), "\t");
        assert_eq!(unescape(r"a\\b"), r"a\b");
        assert_eq!(unescape(r"\q"), r"\q");
        assert_eq!(unescape(", "), ", ");
    }

    #[test]
    fn shim_command_parses_flags_and_values() {
        let m = shim_command("hex2bin", "x".into())
            .get_matches_from(["hex2bin", "-u", "-w", "8", "ff", "0f"]);
        let opts = options_from(&m);
        assert!(opts.upper);
        assert_eq!(opts.width, Some(8));
        let vals: Vec<_> = m.get_many::<String>("values").unwrap().collect();
        assert_eq!(vals, ["ff", "0f"]);
    }

    #[test]
    fn join_and_delim_are_mutually_exclusive() {
        let res = shim_command("hex2bin", "x".into())
            .try_get_matches_from(["hex2bin", "-j", "-d", ","]);
        assert!(res.is_err());
    }
}
