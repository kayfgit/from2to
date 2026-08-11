//! Argument definitions.
//!
//! The same flag set applies whether the binary was invoked through a shim
//! (`hex2bin -u ff`) or by its own name (`f2t hex bin -u ff`); only the
//! positional arguments differ, so the command is built in two shapes.

use clap::{Arg, ArgAction, ArgMatches, Command};

use crate::convert::Options;

const AFTER_HELP: &str = "\
EXAMPLES:
    hex2bin ff                    11111111
    hex2bin 0f                    00001111   (digit width is preserved)
    hex2bin -W 0f                 1111       (bare numeric)
    nc host 1234 | dec2char       decode a stream of decimal code points
    char2hex picoCTF              7069636f435446
    hex2b64 706963                cGlj
    f2t 36 10 zz                  1295       (any base 2-36)

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
            .subcommand(Command::new("list").about("List every command name")),
    )
}

/// Pull the conversion options out of parsed arguments.
pub fn options_from(m: &ArgMatches) -> Options {
    let bytes_mode = if m.get_flag("bytes") {
        Some(true)
    } else if m.get_flag("codepoint") {
        Some(false)
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
