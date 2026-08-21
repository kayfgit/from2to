//! End-to-end tests: the real binary, run the way a user runs it.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const EXE: &str = env!("CARGO_BIN_EXE_from2to");
const EXE_SUFFIX: &str = std::env::consts::EXE_SUFFIX;

struct Output {
    stdout: String,
    stderr: String,
    code: i32,
}

impl Output {
    /// stdout without the trailing newline the tool adds.
    fn out(&self) -> &str {
        self.stdout.strip_suffix('\n').unwrap_or(&self.stdout)
    }
}

fn run_exe(exe: &Path, args: &[&str], stdin: Option<&str>) -> Output {
    let mut child = Command::new(exe)
        .args(args)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn");

    if let Some(input) = stdin {
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
    }
    let out = child.wait_with_output().unwrap();
    Output {
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        code: out.status.code().unwrap_or(-1),
    }
}

fn run(args: &[&str]) -> Output {
    run_exe(Path::new(EXE), args, None)
}

fn run_stdin(args: &[&str], input: &str) -> Output {
    run_exe(Path::new(EXE), args, Some(input))
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("f2t-it-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

/// Put the binary in `dir` under a shim name, the way `install` would.
///
/// Idempotent: asking for the same name twice hands back what is already
/// there. On Windows the second `copy` would otherwise fail with a sharing
/// violation, because the first shim has just been run.
fn make_shim(dir: &Path, name: &str) -> PathBuf {
    let dst = dir.join(format!("{name}{EXE_SUFFIX}"));
    if dst.exists() {
        return dst;
    }
    if fs::hard_link(EXE, &dst).is_err() {
        fs::copy(EXE, &dst).unwrap();
    }
    dst
}

// ----------------------------------------------------------- arguments

#[test]
fn converts_values_given_as_arguments() {
    assert_eq!(run(&["16", "2", "0f"]).out(), "00001111");
    assert_eq!(run(&["16", "2", "-W", "0f"]).out(), "1111");
    assert_eq!(run(&["36", "10", "zz"]).out(), "1295");
    assert_eq!(run(&["16", "b64", "68656c6c6f"]).out(), "aGVsbG8=");
}

#[test]
fn accepts_word_names_and_combined_names() {
    assert_eq!(run(&["hex", "bin", "ff"]).out(), "11111111");
    assert_eq!(run(&["hex2bin", "ff"]).out(), "11111111");
    assert_eq!(run(&["1628", "ff"]).out(), "377");
}

#[test]
fn multiple_values_produce_multiple_results() {
    assert_eq!(run(&["hex2bin", "41", "42"]).out(), "01000001\n01000010");
}

// ----------------------------------------------------------- stdin

#[test]
fn a_stream_of_decimal_code_points_becomes_text() {
    // What `nc host port | dec2char` sees: decimal code points, one per line.
    let out = run_stdin(&["dec", "char"], "104\n101\n108\n108\n111\n");
    assert_eq!(out.out(), "hello");
    assert_eq!(out.code, 0);
}

#[test]
fn stdin_numbers_convert_line_by_line() {
    assert_eq!(run_stdin(&["dec", "hex"], "255\n16\n").out(), "ff\n10");
}

#[test]
fn char_input_from_a_pipe_drops_the_echo_newline() {
    assert_eq!(run_stdin(&["char", "hex"], "hello\n").out(), "68656c6c6f");
    // Interior newlines are data and must survive.
    assert_eq!(run_stdin(&["char", "hex"], "a\nb\n").out(), "610a62");
}

#[test]
fn trailing_newline_can_be_suppressed() {
    assert_eq!(run(&["dec", "char", "112"]).stdout, "p\n");
    assert_eq!(run(&["dec", "char", "-n", "112"]).stdout, "p");
}

// ----------------------------------------------------------- argv[0]

#[test]
fn argv0_selects_the_conversion() {
    let dir = temp_dir("argv0");
    for (name, args, stdin, expected) in [
        ("hex2bin", vec!["41", "42"], None, "01000001\n01000010"),
        ("1628", vec!["ff"], None, "377"),
        ("b642char", vec!["aGVsbG8="], None, "hello"),
        ("dec2ascii", vec!["104", "105"], None, "hi"),
        ("char2hex", vec!["hello"], None, "68656c6c6f"),
        ("dec2char", vec![], Some("104\n101\n108\n108\n111\n"), "hello"),
    ] {
        let shim = make_shim(&dir, name);
        let out = run_exe(&shim, &args, stdin);
        assert_eq!(out.out(), expected, "{name} produced the wrong output");
        assert_eq!(out.code, 0, "{name} exited {}", out.code);
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn shim_help_names_the_conversion() {
    let dir = temp_dir("help");
    let shim = make_shim(&dir, "hex2bin");
    let out = run_exe(&shim, &["--help"], None);
    assert!(out.stdout.contains("Convert hex to bin"), "{}", out.stdout);
    let _ = fs::remove_dir_all(&dir);
}

// ----------------------------------------------------------- byte order

#[test]
fn endian_shims_swap_bytes() {
    let dir = temp_dir("endian");
    for (name, args, stdin, expected) in [
        // 0x0d0c0b0a read back to front is 0x0a0b0c0d.
        ("lehex2dec", vec!["0d0c0b0a"], None, "168496141"),
        ("lehex2hex", vec!["0d0c0b0a"], None, "0a0b0c0d"),
        ("hex2lebin", vec!["ff00"], None, "0000000011111111"),
        ("lehex2char", vec!["6f6c6c6568"], None, "hello"),
        ("char2lehex", vec!["hello"], None, "6f6c6c6568"),
        // A single byte has no byte order.
        ("lehex2dec", vec!["ff"], None, "255"),
        // Per token, never across whitespace.
        ("lehex2hex", vec!["0102", "0304"], None, "0201\n0403"),
        // A word size pins the width and swaps each word on its own.
        ("lehex2hex", vec!["--word", "4", "0d0c0b0a78563412"], None, "0a0b0c0d12345678"),
        ("dec2lehex", vec!["--word", "4", "1234"], None, "d2040000"),
        ("lehex2dec", vec![], Some("0d0c0b0a\nff00\n"), "168496141\n255"),
    ] {
        let shim = make_shim(&dir, name);
        let out = run_exe(&shim, &args, stdin);
        assert_eq!(out.out(), expected, "{name} {args:?} produced the wrong output");
        assert_eq!(out.code, 0, "{name} exited {}: {}", out.code, out.stderr);
    }
    let _ = fs::remove_dir_all(&dir);
}

/// The `f2t` spellings must agree with the shim, or people will learn one and
/// be surprised by the other.
#[test]
fn every_spelling_of_a_byte_order_agrees() {
    for args in [
        vec!["lehex2bin", "0d0c0b0a"],
        vec!["lehex", "bin", "0d0c0b0a"],
        vec!["lehex", "2", "0d0c0b0a"],
    ] {
        let out = run(&args);
        assert_eq!(out.code, 0, "{args:?}: {}", out.stderr);
        assert_eq!(out.out(), "00001010000010110000110000001101", "{args:?}");
    }
    // `be` is the default written down, so it changes nothing.
    assert_eq!(run(&["behex2dec", "ff"]).out(), run(&["hex2dec", "ff"]).out());
}

/// A swap over a run of bytes has to see the whole run. Reading it a line at a
/// time would reverse each line on its own, which is a wrong answer.
#[test]
fn a_swapped_byte_run_is_read_whole() {
    let dir = temp_dir("swapstream");
    let shim = make_shim(&dir, "char2lehex");
    let out = run_exe(&shim, &[], Some("hello\nworld\n"));
    // The whole run reversed, newline included — not "olleh\ndlrow".
    assert_eq!(out.out(), "646c726f770a6f6c6c6568", "{}", out.stderr);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_byte_order_without_a_width_is_refused() {
    // Base 10 carries no byte width of its own.
    let out = run(&["ledec2hex", "258"]);
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("--word"), "{}", out.stderr);

    // An odd hex digit count has no byte boundary to swap on.
    let out = run(&["lehex2dec", "fff"]);
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("whole number of bytes"), "{}", out.stderr);

    // `-k` downgrades a token-level failure, as it does everywhere else.
    let out = run(&["lehex2dec", "-k", "fff", "0d0c0b0a"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(out.out(), "168496141");
}

#[test]
fn shim_help_names_the_byte_order() {
    let dir = temp_dir("lehelp");
    let shim = make_shim(&dir, "lehex2bin");
    let out = run_exe(&shim, &["--help"], None);
    assert!(
        out.stdout.contains("Convert little-endian hex to bin"),
        "{}",
        out.stdout
    );
    let _ = fs::remove_dir_all(&dir);
}

// ----------------------------------------------------------- detection

#[test]
fn detect_identifies_a_base64_challenge_string() {
    let out = run(&["detect", "bDNhcm5fdGgzX3IwcDM1"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(out.stdout.starts_with("b64"), "{}", out.stdout);
    assert!(out.stdout.contains("l3arn_th3_r0p35"), "{}", out.stdout);
    // And it hands back a command that finishes the job.
    assert!(
        out.stdout.contains("b642char bDNhcm5fdGgzX3IwcDM1"),
        "{}",
        out.stdout
    );
}

#[test]
fn detect_can_decode_in_one_step_or_name_the_format() {
    assert_eq!(
        run(&["detect", "-d", "bDNhcm5fdGgzX3IwcDM1"]).out(),
        "l3arn_th3_r0p35"
    );
    assert_eq!(run(&["detect", "-q", "bDNhcm5fdGgzX3IwcDM1"]).out(), "b64");
    assert_eq!(run(&["detect", "-q", "68656c6c6f"]).out(), "hex");
    assert_eq!(run(&["detect", "-q", "01010101"]).out(), "bin");
    // The named format is one the converter accepts, so the two compose.
    assert_eq!(run(&["b64", "char", "bDNhcm5fdGgzX3IwcDM1"]).out(), "l3arn_th3_r0p35");
}

#[test]
fn detect_reads_stdin_and_reports_runners_up() {
    let out = run_stdin(&["detect"], "aGVsbG8=\n");
    assert_eq!(out.code, 0);
    assert!(out.stdout.starts_with("b64"), "{}", out.stdout);
    assert!(out.stdout.contains("hello"), "{}", out.stdout);

    let all = run(&["detect", "--all", "01010101"]);
    assert!(all.stdout.lines().count() > 4, "{}", all.stdout);
    assert!(all.stdout.contains("oct"), "{}", all.stdout);
}

#[test]
fn whatbase_works_through_its_own_name() {
    let dir = temp_dir("whatbase");
    let shim = make_shim(&dir, "whatbase");

    assert_eq!(run_exe(&shim, &["-q", "bDNhcm5fdGgzX3IwcDM1"], None).out(), "b64");
    assert_eq!(
        run_exe(&shim, &["-d", "bDNhcm5fdGgzX3IwcDM1"], None).out(),
        "l3arn_th3_r0p35"
    );
    let help = run_exe(&shim, &["--help"], None);
    assert!(help.stdout.contains("Work out what format"), "{}", help.stdout);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn detect_says_when_text_is_already_text() {
    let out = run(&["detect", "-q", "hello world"]);
    assert_eq!(out.out(), "char");
    assert_eq!(out.code, 0);

    let out = run_stdin(&["detect"], "");
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("nothing to identify"), "{}", out.stderr);
}

// ----------------------------------------------------------- errors

#[test]
fn bad_digits_fail_loudly() {
    let out = run(&["hex", "dec", "zz"]);
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("not valid base 16"), "{}", out.stderr);
    assert_eq!(out.stdout, "");
}

#[test]
fn keep_going_skips_and_warns() {
    let out = run(&["hex", "dec", "-k", "ff", "zz", "10"]);
    assert_eq!(out.out(), "255\n16");
    assert_eq!(out.code, 0);
    assert!(out.stderr.contains("'zz'"), "{}", out.stderr);
}

#[test]
fn quiet_suppresses_the_warning_but_keeps_the_output() {
    let out = run(&["hex", "dec", "-k", "-q", "ff", "zz"]);
    assert_eq!(out.out(), "255");
    assert_eq!(out.stderr, "");
}

#[test]
fn unknown_format_is_a_usage_error() {
    let out = run(&["nope", "hex", "1"]);
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("unknown format"), "{}", out.stderr);
}

// ----------------------------------------------------------- formatting

#[test]
fn formatting_flags_apply() {
    assert_eq!(run(&["hex", "bin", "-g", "4", "ff"]).out(), "1111 1111");
    assert_eq!(run(&["dec", "hex", "-u", "255"]).out(), "FF");
    assert_eq!(run(&["dec", "hex", "-x", "255"]).out(), "0xff");
    assert_eq!(run(&["hex", "bin", "-w", "12", "0f"]).out(), "000000001111");
    assert_eq!(run(&["dec", "hex", "-d", ",", "255", "16"]).out(), "ff,10");
    assert_eq!(run(&["dec", "hex", "-j", "255", "16"]).out(), "ff10");
}

// ----------------------------------------------------------- docs

/// The man page is hand-written, so nothing stops it drifting behind the
/// argument definitions except this. The expectations are read out of `opts`
/// rather than listed here, so a new flag fails the test the day it is added.
#[test]
fn the_man_page_documents_every_flag_and_subcommand() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("man/from2to.1");
    let roff = fs::read_to_string(&path).expect("man/from2to.1 is missing");
    // roff escapes option dashes as `\-`; compare against plain text.
    let text = roff.replace("\\-", "-");

    fn walk(text: &str, cmd: &clap::Command, missing: &mut Vec<String>, seen: &mut usize) {
        for arg in cmd.get_arguments() {
            if let Some(long) = arg.get_long() {
                *seen += 1;
                let flag = format!("--{long}");
                if !text.contains(&flag) {
                    missing.push(flag);
                }
            }
        }
        for sub in cmd.get_subcommands() {
            *seen += 1;
            if !text.contains(sub.get_name()) {
                missing.push(sub.get_name().to_string());
            }
            walk(text, sub, missing, seen);
        }
    }

    let mut missing = Vec::new();
    let mut seen = 0;
    walk(&text, &from2to::opts::self_command("from2to"), &mut missing, &mut seen);
    walk(&text, &from2to::opts::detect_command("whatbase"), &mut missing, &mut seen);

    assert!(missing.is_empty(), "man page never mentions: {missing:?}");
    // Guard against the test passing because it inspected nothing.
    assert!(seen > 20, "only {seen} items checked; the walk is not working");
}

// ----------------------------------------------------------- install

#[test]
fn list_names_every_command() {
    let out = run(&["list"]);
    assert_eq!(out.code, 0);
    let lines: Vec<&str> = out.stdout.lines().collect();
    assert_eq!(lines.len(), 65);
    assert!(lines.iter().any(|l| l.starts_with("hex2bin ")));
    assert!(lines.iter().any(|l| l.starts_with("whatbase ")));
    // The numeric shorthands are not commands any more, only `f2t` forms.
    assert!(!lines.iter().any(|l| l.starts_with("1628 ")));
}

#[test]
fn dry_run_changes_nothing() {
    let dir = temp_dir("dryrun");
    let exe = dir.join(format!("from2to{EXE_SUFFIX}"));
    fs::copy(EXE, &exe).unwrap();

    let out = run_exe(&exe, &["install", "--dir", dir.to_str().unwrap(), "--dry-run"], None);
    assert_eq!(out.code, 0);
    assert!(out.stdout.contains("would create"));
    // Only the binary itself should be there.
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn install_then_uninstall_round_trips() {
    let dir = temp_dir("roundtrip");
    let exe = dir.join(format!("from2to{EXE_SUFFIX}"));
    fs::copy(EXE, &exe).unwrap();
    let dir_s = dir.to_str().unwrap();

    let out = run_exe(&exe, &["install", "--dir", dir_s], None);
    assert_eq!(out.code, 0, "{}", out.stderr);
    // 64 shims + whatbase + f2t + the binary.
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 67);

    // A shim really works.
    let shim = dir.join(format!("hex2bin{EXE_SUFFIX}"));
    assert_eq!(run_exe(&shim, &["ff"], None).out(), "11111111");

    // Installing again is a no-op rather than an error.
    let again = run_exe(&exe, &["install", "--dir", dir_s], None);
    assert_eq!(again.code, 0);
    assert!(again.stdout.contains("already present"), "{}", again.stdout);

    let out = run_exe(&exe, &["uninstall", "--dir", dir_s], None);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);

    let _ = fs::remove_dir_all(&dir);
}

/// The reason Unix defaults to symlinks. `cargo install --force` writes a *new*
/// binary; a symlink follows it, so the shims keep working with no reinstall.
/// A hardlink would still point at the old inode.
#[test]
fn symlink_shims_survive_the_binary_being_replaced() {
    let dir = temp_dir("upgrade");
    let exe = dir.join(format!("from2to{EXE_SUFFIX}"));
    fs::copy(EXE, &exe).unwrap();
    let dir_s = dir.to_str().unwrap();

    let out = run_exe(&exe, &["install", "--dir", dir_s, "--method", "symlink"], None);
    if out.code != 0 {
        // Windows without Developer Mode cannot make symlinks at all.
        eprintln!("skipping: symlinks unavailable here ({})", out.stderr);
        let _ = fs::remove_dir_all(&dir);
        return;
    }

    let shim = dir.join(format!("hex2bin{EXE_SUFFIX}"));
    // Relative, so the whole directory can be moved or renamed as a unit.
    assert_eq!(
        fs::read_link(&shim).unwrap(),
        PathBuf::from(format!("from2to{EXE_SUFFIX}")),
    );
    assert_eq!(run_exe(&shim, &["ff"], None).out(), "11111111");

    // Stand in for `cargo install --force`: same path, different bytes.
    let mut bytes = fs::read(EXE).unwrap();
    bytes.extend_from_slice(b"\n// a later build\n");
    fs::remove_file(&exe).unwrap();
    fs::write(&exe, &bytes).unwrap();

    assert_eq!(
        fs::read(&shim).unwrap(),
        bytes,
        "the shim should now resolve to the new binary, with no reinstall"
    );

    // And uninstall must still recognize symlinks as ours and remove them.
    let out = run_exe(&exe, &["uninstall", "--dir", dir_s], None);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 1, "only the binary left");

    let _ = fs::remove_dir_all(&dir);
}

/// Rebuilding changes the binary, so every shim stops being byte-identical to
/// it. Reinstalling must refresh them rather than call them foreign.
#[test]
fn reinstall_after_a_rebuild_refreshes_stale_shims() {
    let dir = temp_dir("stale");
    let exe = dir.join(format!("from2to{EXE_SUFFIX}"));
    fs::copy(EXE, &exe).unwrap();
    let dir_s = dir.to_str().unwrap();

    run_exe(&exe, &["install", "--dir", dir_s], None);

    // Stand in for "installed by an earlier build": still one of ours, but no
    // longer the same bytes as the binary doing the installing.
    let shim = dir.join(format!("hex2bin{EXE_SUFFIX}"));
    let mut bytes = fs::read(EXE).unwrap();
    bytes.extend_from_slice(b"\n// an earlier build\n");
    fs::remove_file(&shim).unwrap();
    fs::write(&shim, &bytes).unwrap();

    let out = run_exe(&exe, &["install", "--dir", dir_s], None);
    assert_eq!(out.code, 0, "should not need --force: {}", out.stderr);
    assert!(out.stdout.contains("1 replaced"), "{}", out.stdout);
    assert!(!out.stderr.contains("already taken"), "{}", out.stderr);
    // Refreshed to the current build, and working.
    assert_eq!(fs::read(&shim).unwrap(), fs::read(EXE).unwrap());
    assert_eq!(run_exe(&shim, &["ff"], None).out(), "11111111");

    // Uninstall also has to recognize a stale shim as ours.
    fs::remove_file(&shim).unwrap();
    fs::write(&shim, &bytes).unwrap();
    let out = run_exe(&exe, &["uninstall", "--dir", dir_s], None);
    assert_eq!(out.code, 0);
    assert!(!shim.exists(), "a stale shim should still be removed");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn uninstall_leaves_files_that_are_not_ours() {
    let dir = temp_dir("foreign");
    let exe = dir.join(format!("from2to{EXE_SUFFIX}"));
    fs::copy(EXE, &exe).unwrap();
    let dir_s = dir.to_str().unwrap();

    run_exe(&exe, &["install", "--dir", dir_s], None);

    // Someone else's tool happens to be called hex2bin.
    let impostor = dir.join(format!("hex2bin{EXE_SUFFIX}"));
    fs::remove_file(&impostor).unwrap();
    fs::write(&impostor, b"a different program entirely").unwrap();

    let out = run_exe(&exe, &["uninstall", "--dir", dir_s], None);
    assert_eq!(out.code, 0);
    assert!(out.stderr.contains("not ours"), "{}", out.stderr);
    assert_eq!(
        fs::read(&impostor).unwrap(),
        b"a different program entirely"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn install_refuses_to_clobber_without_force() {
    let dir = temp_dir("clobber");
    let exe = dir.join(format!("from2to{EXE_SUFFIX}"));
    fs::copy(EXE, &exe).unwrap();
    let dir_s = dir.to_str().unwrap();

    let impostor = dir.join(format!("hex2bin{EXE_SUFFIX}"));
    fs::write(&impostor, b"not ours").unwrap();

    let out = run_exe(&exe, &["install", "--dir", dir_s], None);
    assert_eq!(out.code, 1, "should report the conflict");
    assert!(out.stderr.contains("already taken"), "{}", out.stderr);
    assert_eq!(fs::read(&impostor).unwrap(), b"not ours");

    // --force goes through.
    let out = run_exe(&exe, &["install", "--dir", dir_s, "--force"], None);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(run_exe(&impostor, &["ff"], None).out(), "11111111");

    let _ = fs::remove_dir_all(&dir);
}
