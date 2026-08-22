//! The CLI shell.
//!
//! One binary answers to ~66 names. `argv[0]` picks the conversion; under its
//! own name (`f2t`) the formats come from the first two arguments instead.

use std::io::{self, BufRead, BufWriter, Read, Write};
use std::process::ExitCode;

use from2to::convert::{ConvError, Converter, Options};
use from2to::endian::Endpoint;
use from2to::format::Format;
use from2to::{detect, install, names, opts};

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{}: {e}", prog_name());
            ExitCode::FAILURE
        }
    }
}

fn prog_name() -> String {
    std::env::args()
        .next()
        .map(|a| names::basename(&a))
        .unwrap_or_else(|| names::SHORT_NAME.to_string())
}

type Fallible<T> = Result<T, Box<dyn std::error::Error>>;

fn run() -> Fallible<ExitCode> {
    let argv: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let prog = names::basename(&argv[0].to_string_lossy());

    // Invoked through a shim: the name settles the conversion.
    if let Some((from, to)) = names::resolve(&prog) {
        let about = format!("Convert {from} to {to}");
        let m = opts::shim_command(&prog, about).get_matches_from(&argv);
        let values = collect_values(&m, Vec::new());
        return convert(from, to, values, &m);
    }

    // The one installed command that identifies a format instead of converting.
    if prog == names::DETECT_NAME {
        let m = opts::detect_command(&prog).get_matches_from(&argv);
        return run_detect(&m);
    }

    // Invoked as itself.
    let m = opts::self_command(&prog).get_matches_from(&argv);

    match m.subcommand() {
        Some(("detect", sm)) => return run_detect(sm),
        Some(("install", sm)) => {
            let dir = sm.get_one::<String>("dir").map(String::as_str);
            let dry = sm.get_flag("dry-run");
            // clap has already restricted this to the three valid spellings.
            let method = sm
                .get_one::<String>("method")
                .and_then(|m| install::Method::parse(m));
            let report = install::install(dir, dry, sm.get_flag("force"), method)?;
            let target = install::Target::resolve(dir)?;
            install::print_install_report(&target.dir, &report, dry);
            return Ok(if report.conflicts.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            });
        }
        Some(("uninstall", sm)) => {
            let dir = sm.get_one::<String>("dir").map(String::as_str);
            let dry = sm.get_flag("dry-run");
            let report = install::uninstall(dir, dry)?;
            let target = install::Target::resolve(dir)?;
            install::print_uninstall_report(&target.dir, &report, dry);
            return Ok(ExitCode::SUCCESS);
        }
        Some(("list", _)) => {
            install::list();
            return Ok(ExitCode::SUCCESS);
        }
        _ => {}
    }

    let Some(from_arg) = m.get_one::<String>("from") else {
        opts::self_command(&prog).print_help()?;
        println!();
        return Ok(ExitCode::from(2));
    };
    let to_arg = m.get_one::<String>("to");

    // `f2t hex bin ff` and `f2t hex2bin ff` should both work, so the first
    // argument gets a chance to be a whole conversion name.
    let (from, to, leading) = match names::resolve(from_arg) {
        Some((f, t)) => (f, t, to_arg.cloned().into_iter().collect()),
        None => {
            // `Endpoint::parse`, not `Format::parse`, so `f2t lehex bin ff`
            // works alongside `f2t lehex2bin ff`.
            let f = Endpoint::parse(from_arg)
                .ok_or_else(|| format!("unknown format '{from_arg}'"))?;
            let t_arg = to_arg.ok_or("missing target format (try: f2t hex bin ff)")?;
            let t = Endpoint::parse(t_arg).ok_or_else(|| format!("unknown format '{t_arg}'"))?;
            (f, t, Vec::new())
        }
    };

    let values = collect_values(&m, leading);
    convert(from, to, values, &m)
}

/// `whatbase` and `f2t detect`: say what the input is instead of converting it.
///
/// Exits non-zero when there is nothing to identify, so a script can branch on
/// it the same way it would on a failed conversion.
fn run_detect(m: &clap::ArgMatches) -> Fallible<ExitCode> {
    let values = collect_values(m, Vec::new());
    let input = if values.is_empty() {
        let mut buf = Vec::new();
        io::stdin().lock().read_to_end(&mut buf)?;
        String::from_utf8_lossy(&buf).into_owned()
    } else {
        values.join(" ")
    };
    let input = input.trim();

    let cands = detect::detect(input);
    let Some(best) = cands.first() else {
        eprintln!("{}: nothing to identify", prog_name());
        return Ok(ExitCode::FAILURE);
    };

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    if m.get_flag("name") {
        writeln!(out, "{}", best.format)?;
    } else if m.get_flag("decode") {
        let to = match m.get_one::<String>("to") {
            Some(t) => Format::parse(t).ok_or_else(|| format!("unknown format '{t}'"))?,
            None => Format::Char,
        };
        let text = Converter::new(best.format, to, Options::default()).convert(input)?;
        writeln!(out, "{}", text.strip_suffix('\n').unwrap_or(&text))?;
    } else {
        let all = m.get_flag("all");
        // Terse by default: the question is "what is this", and the
        // reasoning is only interesting once you doubt the answer.
        let text = if m.get_flag("why") {
            detect::report(input, &cands, all)
        } else {
            detect::report_terse(&cands, all)
        };
        write!(out, "{text}")?;
    }
    out.flush()?;
    Ok(ExitCode::SUCCESS)
}

fn collect_values(m: &clap::ArgMatches, mut leading: Vec<String>) -> Vec<String> {
    if let Some(vals) = m.get_many::<String>("values") {
        leading.extend(vals.cloned());
    }
    leading
}

fn convert(
    from: Endpoint,
    to: Endpoint,
    values: Vec<String>,
    m: &clap::ArgMatches,
) -> Fallible<ExitCode> {
    let quiet = m.get_flag("quiet");
    let no_newline = m.get_flag("no-newline");

    // The command name carried the byte order this far; the converter wants it
    // in `Options`, which is what keeps `Converter::new` down to two formats.
    let mut options = opts::options_from(m);
    options.from_endian = from.endian;
    options.to_endian = to.endian;

    let conv = Converter::new(from.format, to.format, options);
    // Fail on a configuration that cannot mean anything before reading stdin.
    conv.check()?;

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    let mut sink = Sink::new(conv.delim().to_string());
    let mut warnings: Vec<ConvError> = Vec::new();
    let mut carry: Vec<u8> = Vec::new();

    if !values.is_empty() {
        let input = values.join(" ");
        let text = conv.convert_chunk(&input, &mut carry, &mut warnings)?;
        sink.push(&mut out, &text)?;
    } else if conv.can_stream() {
        stream_stdin(&conv, &mut out, &mut sink, &mut carry, &mut warnings)?;
    } else {
        let mut buf = Vec::new();
        io::stdin().lock().read_to_end(&mut buf)?;
        let input = String::from_utf8_lossy(&buf);
        let text = conv.convert_chunk(trim_one_newline(&input), &mut carry, &mut warnings)?;
        sink.push(&mut out, &text)?;
    }

    // Anything still held back was an incomplete UTF-8 sequence at EOF.
    if !carry.is_empty() {
        let tail = String::from_utf8_lossy(&carry).into_owned();
        sink.push_raw(&mut out, &tail)?;
    }
    if sink.wrote_any && !no_newline && !sink.ends_with_newline {
        out.write_all(b"\n")?;
    }
    out.flush()?;

    if !quiet {
        for w in &warnings {
            eprintln!("{}: {w}", prog_name());
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// Read stdin a line at a time so output appears as data arrives.
fn stream_stdin(
    conv: &Converter,
    out: &mut impl Write,
    sink: &mut Sink,
    carry: &mut Vec<u8>,
    warnings: &mut Vec<ConvError>,
) -> Fallible<()> {
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut buf = Vec::new();
    // For `char` input the newlines are data, not separators, so a line ending
    // is held back until we know another line follows. That way the newline
    // `echo` adds does not turn up in the output.
    let char_source = conv.from == Format::Char;
    let mut held = String::new();

    loop {
        buf.clear();
        if reader.read_until(b'\n', &mut buf)? == 0 {
            break;
        }
        let chunk = String::from_utf8_lossy(&buf);
        let text = if char_source {
            let (body, ending) = split_trailing_newline(&chunk);
            let joined = format!("{held}{body}");
            held = ending.to_string();
            conv.convert_chunk(&joined, carry, warnings)?
        } else {
            conv.convert_chunk(&chunk, carry, warnings)?
        };
        sink.push(out, &text)?;
        out.flush()?;
    }
    Ok(())
}

fn split_trailing_newline(s: &str) -> (&str, &str) {
    if let Some(rest) = s.strip_suffix("\r\n") {
        (rest, "\r\n")
    } else if let Some(rest) = s.strip_suffix('\n') {
        (rest, "\n")
    } else {
        (s, "")
    }
}

fn trim_one_newline(s: &str) -> &str {
    split_trailing_newline(s).0
}

/// Writes results, inserting the separator between chunks rather than after
/// them, and remembering whether a trailing newline is still needed.
struct Sink {
    delim: String,
    wrote_any: bool,
    ends_with_newline: bool,
}

impl Sink {
    fn new(delim: String) -> Self {
        Sink {
            delim,
            wrote_any: false,
            ends_with_newline: false,
        }
    }

    fn push(&mut self, out: &mut impl Write, text: &str) -> io::Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        if self.wrote_any && !self.delim.is_empty() {
            out.write_all(self.delim.as_bytes())?;
        }
        self.write(out, text)
    }

    /// Append without a separator, for the UTF-8 tail at EOF.
    fn push_raw(&mut self, out: &mut impl Write, text: &str) -> io::Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        self.write(out, text)
    }

    fn write(&mut self, out: &mut impl Write, text: &str) -> io::Result<()> {
        out.write_all(text.as_bytes())?;
        self.wrote_any = true;
        self.ends_with_newline = text.ends_with('\n');
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_both_line_endings() {
        assert_eq!(split_trailing_newline("abc\n"), ("abc", "\n"));
        assert_eq!(split_trailing_newline("abc\r\n"), ("abc", "\r\n"));
        assert_eq!(split_trailing_newline("abc"), ("abc", ""));
        assert_eq!(split_trailing_newline("\n"), ("", "\n"));
    }

    #[test]
    fn sink_puts_separators_between_not_after() {
        let mut buf = Vec::new();
        let mut sink = Sink::new("\n".into());
        sink.push(&mut buf, "ff").unwrap();
        sink.push(&mut buf, "").unwrap();
        sink.push(&mut buf, "10").unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "ff\n10");
        assert!(!sink.ends_with_newline);
    }
}
