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
fn make_shim(dir: &Path, name: &str) -> PathBuf {
    let dst = dir.join(format!("{name}{EXE_SUFFIX}"));
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
    assert_eq!(run(&["16", "b64", "706963"]).out(), "cGlj");
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
fn the_picoctf_case() {
    // What `nc host port | dec2char` sees: decimal code points, one per line.
    let out = run_stdin(&["dec", "char"], "112\n105\n99\n111\n");
    assert_eq!(out.out(), "pico");
    assert_eq!(out.code, 0);
}

#[test]
fn stdin_numbers_convert_line_by_line() {
    assert_eq!(run_stdin(&["dec", "hex"], "255\n16\n").out(), "ff\n10");
}

#[test]
fn char_input_from_a_pipe_drops_the_echo_newline() {
    assert_eq!(run_stdin(&["char", "hex"], "picoCTF\n").out(), "7069636f435446");
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
        ("b642char", vec!["cGljb0NURg=="], None, "picoCTF"),
        ("dec2ascii", vec!["112", "105"], None, "pi"),
        ("char2hex", vec!["picoCTF"], None, "7069636f435446"),
        ("dec2char", vec![], Some("112\n105\n99\n111\n"), "pico"),
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

// ----------------------------------------------------------- install

#[test]
fn list_names_every_command() {
    let out = run(&["list"]);
    assert_eq!(out.code, 0);
    let lines: Vec<&str> = out.stdout.lines().collect();
    assert_eq!(lines.len(), 62);
    assert!(lines.iter().any(|l| l.starts_with("hex2bin ")));
    assert!(lines.iter().any(|l| l.starts_with("1628 ")));
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
    // 62 shims + f2t + the binary.
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 64);

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
