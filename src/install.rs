//! Creating and removing the per-conversion command shims.
//!
//! Every shim is the same binary under a different name; the name is what
//! selects the conversion. Both linking methods share one copy on disk, so the
//! choice between them is about what survives an upgrade:
//!
//! - **Unix leads with symlinks.** `cargo install --force` writes a *new*
//!   binary. A symlink follows it; a hardlink is left pointing at the old
//!   inode, which is why every shim used to go stale on every upgrade.
//! - **Windows leads with hardlinks**, because creating a symlink there needs
//!   Developer Mode or an elevated prompt.
//!
//! An outright copy is the last resort on both. `--method` overrides the order.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::names;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Hardlink,
    Symlink,
    Copy,
}

impl Method {
    pub fn label(self) -> &'static str {
        match self {
            Method::Hardlink => "hardlink",
            Method::Symlink => "symlink",
            Method::Copy => "copy",
        }
    }

    /// Every spelling `--method` accepts, and the order they are listed in help.
    pub const ALL: [Method; 3] = [Method::Hardlink, Method::Symlink, Method::Copy];

    pub fn parse(s: &str) -> Option<Method> {
        Method::ALL.into_iter().find(|m| m.label() == s)
    }
}

/// The order to try when the caller has not insisted on one. See the module
/// docs for why the two platforms disagree.
#[cfg(windows)]
const DEFAULT_ORDER: [Method; 3] = [Method::Hardlink, Method::Symlink, Method::Copy];
#[cfg(not(windows))]
const DEFAULT_ORDER: [Method; 3] = [Method::Symlink, Method::Hardlink, Method::Copy];

const EXE_SUFFIX: &str = std::env::consts::EXE_SUFFIX;

/// Where the shims go, and what they are linked from.
pub struct Target {
    pub exe: PathBuf,
    pub dir: PathBuf,
    exe_bytes: OnceLock<Option<Vec<u8>>>,
}

impl Target {
    pub fn resolve(dir: Option<&str>) -> io::Result<Target> {
        let exe = std::env::current_exe()?;
        let exe = fs::canonicalize(&exe).unwrap_or(exe);
        let dir = match dir {
            Some(d) => PathBuf::from(d),
            None => exe
                .parent()
                .ok_or_else(|| io::Error::other("cannot determine the binary's directory"))?
                .to_path_buf(),
        };
        Ok(Target {
            exe,
            dir,
            exe_bytes: OnceLock::new(),
        })
    }

    fn path_for(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}{EXE_SUFFIX}"))
    }

    fn exe_bytes(&self) -> Option<&Vec<u8>> {
        self.exe_bytes.get_or_init(|| fs::read(&self.exe).ok()).as_ref()
    }

    /// Is this path the binary that is currently running?
    ///
    /// A symlink shim *resolves* to that binary, so canonicalizing alone would
    /// call every one of them "self" — and `uninstall`, which skips anything
    /// that is self, would then remove nothing at all. Only the real file
    /// counts; a link that merely points at it does not.
    fn is_self(&self, path: &Path) -> bool {
        if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
            return false;
        }
        fs::canonicalize(path).is_ok_and(|p| p == self.exe)
    }

    /// A symlink naming the same file as our binary — ours, even if it dangles.
    fn points_at_us(&self, path: &Path) -> bool {
        fs::read_link(path).is_ok_and(|t| t.file_name() == self.exe.file_name())
    }

    /// Work out what a file occupying one of our names actually is.
    ///
    /// Byte-identity alone is not enough: `cargo install` writes a brand new
    /// binary, which instantly makes every existing shim a non-match even
    /// though we put it there. So a shim from any build is recognized by the
    /// marker below, and byte-identity only distinguishes "already current"
    /// from "ours but stale".
    fn classify(&self, path: &Path) -> Kind {
        let Ok(bytes) = fs::read(path) else {
            // Unreadable, which for a valid symlink cannot happen — `read`
            // follows it. So this is a shim left dangling by a binary that
            // moved away, and it is still ours to replace.
            return if self.points_at_us(path) {
                Kind::Stale
            } else {
                Kind::Foreign
            };
        };
        if self.exe_bytes().is_some_and(|ours| &bytes == ours) {
            return Kind::Current;
        }
        if contains(&bytes, SHIM_MARKER) {
            return Kind::Stale;
        }
        Kind::Foreign
    }
}

/// What a file sitting on one of our command names turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Byte-identical to the binary we are installing from.
    Current,
    /// A from2to build, but not this one — safe to replace or remove.
    Stale,
    /// Someone else's file. Never touched without `--force`.
    Foreign,
}

/// Compiled into every build so that any version can recognize its own shims.
///
/// It is referenced by `contains` below, which keeps the literal in the
/// binary's data section where a scan of the file will find it.
const SHIM_MARKER: &[u8] = b"from2to/shim-marker/9f3ac41d-0e2b-4c77-8a15-6db0e3f9c2a1";

/// Substring search, scanning on the first byte so a megabyte-sized executable
/// does not turn into a quadratic walk.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    let first = needle[0];
    let last = haystack.len() - needle.len();
    let mut i = 0;
    while i <= last {
        if haystack[i] == first && &haystack[i..i + needle.len()] == needle {
            return true;
        }
        i += 1;
    }
    false
}

/// Names to create: every conversion, the detector, plus the short self-name.
fn all_names() -> Vec<String> {
    let mut names: Vec<String> = names::shim_names().into_iter().map(|(n, _, _)| n).collect();
    names.push(names::DETECT_NAME.to_string());
    names.push(names::SHORT_NAME.to_string());
    names
}

// ------------------------------------------------------------ linking

#[cfg(windows)]
fn try_symlink(src: &Path, dst: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_file(src, dst)
}

#[cfg(unix)]
fn try_symlink(src: &Path, dst: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(src, dst)
}

/// Do two paths name the same directory?
fn same_dir(a: &Path, b: &Path) -> bool {
    a == b
        || match (fs::canonicalize(a), fs::canonicalize(b)) {
            (Ok(x), Ok(y)) => x == y,
            _ => false,
        }
}

/// What a symlink at `dst` should point at.
///
/// A bare file name when the two sit in the same directory, so the pair can be
/// moved or renamed as a unit. An absolute target would bake in wherever the
/// binary happened to be when `install` ran, which breaks the moment anything
/// relocates the directory. Pointing across directories leaves no choice.
fn symlink_target(src: &Path, dst: &Path) -> PathBuf {
    match (src.parent(), dst.parent()) {
        (Some(a), Some(b)) if same_dir(a, b) => {
            PathBuf::from(src.file_name().unwrap_or(src.as_os_str()))
        }
        _ => src.to_path_buf(),
    }
}

fn try_method(m: Method, src: &Path, dst: &Path) -> io::Result<()> {
    match m {
        Method::Hardlink => fs::hard_link(src, dst),
        Method::Symlink => try_symlink(&symlink_target(src, dst), dst),
        Method::Copy => fs::copy(src, dst).map(|_| ()),
    }
}

/// Create one shim, degrading through the fallbacks until something works.
///
/// `want` pins a single method instead — if it fails, that is an error rather
/// than a cue to try something else, since the caller asked for it by name.
fn link_one(src: &Path, dst: &Path, want: Option<Method>) -> io::Result<Method> {
    let pinned;
    let order: &[Method] = match want {
        Some(m) => {
            pinned = [m];
            &pinned
        }
        None => &DEFAULT_ORDER,
    };

    let mut last = None;
    for &m in order {
        match try_method(m, src, dst) {
            Ok(()) => return Ok(m),
            // Something is already sitting there; that is the caller's call to
            // make, not something to work around by trying another method.
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => return Err(e),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or_else(|| io::Error::other("no way to create a link")))
}

// ------------------------------------------------------------ install

#[derive(Debug, Default)]
pub struct Report {
    pub created: usize,
    pub already: usize,
    pub replaced: usize,
    pub removed: usize,
    pub conflicts: Vec<PathBuf>,
    pub methods: Vec<Method>,
}

/// Probe before creating fifty-odd files, so a permission problem surfaces as
/// one clear message instead of halfway through the run.
fn check_writable(dir: &Path) -> io::Result<()> {
    let probe = dir.join(format!(".from2to-write-probe-{}", std::process::id()));
    match fs::File::create(&probe) {
        Ok(_) => {
            let _ = fs::remove_file(&probe);
            Ok(())
        }
        Err(e) => Err(io::Error::other(format!(
            "cannot write to {} ({e}) — choose somewhere else with --dir",
            dir.display()
        ))),
    }
}

pub fn install(
    dir: Option<&str>,
    dry_run: bool,
    force: bool,
    method: Option<Method>,
) -> io::Result<Report> {
    let target = Target::resolve(dir)?;
    if !target.dir.is_dir() {
        return Err(io::Error::other(format!(
            "{} is not a directory",
            target.dir.display()
        )));
    }
    if !dry_run {
        check_writable(&target.dir)?;
    }

    let mut report = Report::default();
    let mut planned = Vec::new();

    for name in all_names() {
        let dst = target.path_for(&name);

        if dst.exists() {
            // It *is* the binary doing the installing.
            if target.is_self(&dst) {
                report.already += 1;
                continue;
            }
            match target.classify(&dst) {
                Kind::Current => {
                    report.already += 1;
                    continue;
                }
                // Ours from an earlier build: refresh it, no --force needed.
                Kind::Stale => {
                    if !dry_run {
                        fs::remove_file(&dst)?;
                    }
                    report.replaced += 1;
                }
                Kind::Foreign => {
                    if !force {
                        report.conflicts.push(dst);
                        continue;
                    }
                    if !dry_run {
                        fs::remove_file(&dst)?;
                    }
                    report.replaced += 1;
                }
            }
        }

        planned.push(dst);
    }

    if dry_run {
        for p in &planned {
            println!("would create {}", p.display());
        }
        report.created = planned.len();
        return Ok(report);
    }

    for dst in planned {
        let used = link_one(&target.exe, &dst, method).map_err(|e| {
            io::Error::other(format!("could not create {}: {e}", dst.display()))
        })?;
        if !report.methods.contains(&used) {
            report.methods.push(used);
        }
        report.created += 1;
    }

    Ok(report)
}

pub fn uninstall(dir: Option<&str>, dry_run: bool) -> io::Result<Report> {
    let target = Target::resolve(dir)?;
    let mut report = Report::default();

    for name in all_names() {
        let dst = target.path_for(&name);
        if !dst.exists() {
            continue;
        }
        // Never delete the binary that is doing the uninstalling.
        if target.is_self(&dst) {
            continue;
        }
        // A shim from any build of ours is fair game; anything else is not.
        if target.classify(&dst) == Kind::Foreign {
            report.conflicts.push(dst);
            continue;
        }
        if dry_run {
            println!("would remove {}", dst.display());
        } else {
            fs::remove_file(&dst)?;
        }
        report.removed += 1;
    }

    Ok(report)
}

pub fn print_install_report(target_dir: &Path, r: &Report, dry_run: bool) {
    let verb = if dry_run { "would install" } else { "installed" };
    let how = if r.methods.is_empty() {
        String::new()
    } else {
        format!(
            " via {}",
            r.methods.iter().map(|m| m.label()).collect::<Vec<_>>().join(" + ")
        )
    };
    println!(
        "{verb} {} command{}{} in {}",
        r.created,
        if r.created == 1 { "" } else { "s" },
        how,
        target_dir.display()
    );
    if r.already > 0 {
        println!("  {} already present", r.already);
    }
    if r.replaced > 0 {
        println!("  {} replaced", r.replaced);
    }
    if !r.conflicts.is_empty() {
        eprintln!(
            "  {} name{} already taken by something else (use --force to replace):",
            r.conflicts.len(),
            if r.conflicts.len() == 1 { "" } else { "s" }
        );
        for c in &r.conflicts {
            eprintln!("    {}", c.display());
        }
    }
}

pub fn print_uninstall_report(target_dir: &Path, r: &Report, dry_run: bool) {
    let verb = if dry_run { "would remove" } else { "removed" };
    println!("{verb} {} commands from {}", r.removed, target_dir.display());
    if !r.conflicts.is_empty() {
        eprintln!(
            "  left {} file{} alone (not ours):",
            r.conflicts.len(),
            if r.conflicts.len() == 1 { "" } else { "s" }
        );
        for c in &r.conflicts {
            eprintln!("    {}", c.display());
        }
    }
}

/// Print every command name and what it does.
pub fn list() {
    for (name, from, to) in names::shim_names() {
        println!("{name:<12} {from} -> {to}");
    }
    println!("{:<12} identify the format of some text", names::DETECT_NAME);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_names_includes_shims_and_short_self_name() {
        let names = all_names();
        assert_eq!(names.len(), 52);
        assert!(names.contains(&"hex2bin".to_string()));
        assert!(names.contains(&"f2t".to_string()));
        assert!(names.contains(&"whatbase".to_string()));
        // Numeric shorthands resolve, but a file called `1628` never lands in
        // a directory on PATH.
        assert!(!names.contains(&"1628".to_string()));
        // The real binary name is never a shim; it is what shims point at.
        assert!(!names.contains(&"from2to".to_string()));
    }

    #[test]
    fn a_file_that_is_not_ours_is_not_claimed() {
        let dir = std::env::temp_dir().join(format!("f2t-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let impostor = dir.join("hex2bin.txt");
        fs::write(&impostor, b"not the binary").unwrap();
        let target = Target::resolve(None).unwrap();
        assert_eq!(target.classify(&impostor), Kind::Foreign);
        fs::remove_dir_all(&dir).ok();
    }

    /// The marker has to survive compilation, or every shim looks foreign
    /// after the next rebuild.
    #[test]
    fn the_marker_is_present_in_our_own_binary() {
        let exe = std::env::current_exe().unwrap();
        let bytes = fs::read(&exe).unwrap();
        assert!(
            contains(&bytes, SHIM_MARKER),
            "SHIM_MARKER was optimized out of {}",
            exe.display()
        );
    }

    #[test]
    fn a_build_that_is_not_this_one_still_reads_as_ours() {
        let dir = std::env::temp_dir().join(format!("f2t-stale-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let target = Target::resolve(None).unwrap();

        // Stand in for a different version: carries the marker, differs byte-wise.
        let stale = dir.join("hex2bin.bin");
        let mut bytes = target.exe_bytes().unwrap().clone();
        bytes.extend_from_slice(b"a later build");
        fs::write(&stale, &bytes).unwrap();
        assert_eq!(target.classify(&stale), Kind::Stale);

        // And the exact same bytes read as current.
        let current = dir.join("hex2dec.bin");
        fs::write(&current, target.exe_bytes().unwrap()).unwrap();
        assert_eq!(target.classify(&current), Kind::Current);

        fs::remove_dir_all(&dir).ok();
    }

    /// The property that makes a package relocatable: a shim next to the binary
    /// points at a bare name, never at wherever `install` happened to run.
    #[test]
    fn a_symlink_beside_the_binary_uses_a_bare_name() {
        let src = Path::new("/opt/somewhere/bin/from2to");
        let dst = Path::new("/opt/somewhere/bin/hex2bin");
        assert_eq!(symlink_target(src, dst), PathBuf::from("from2to"));

        // Across directories there is nothing to be relative to.
        let far = Path::new("/usr/local/bin/hex2bin");
        assert_eq!(symlink_target(src, far), src.to_path_buf());
    }

    #[test]
    fn method_names_round_trip() {
        for m in Method::ALL {
            assert_eq!(Method::parse(m.label()), Some(m));
        }
        assert_eq!(Method::parse("nonsense"), None);
    }

    /// `uninstall` skips anything that is "self". Before the symlink switch
    /// that only ever matched the real binary; a symlink resolves to it too, so
    /// treating one as self would make uninstall silently remove nothing.
    #[test]
    #[cfg(unix)]
    fn a_symlink_to_the_binary_is_not_itself() {
        let dir = std::env::temp_dir().join(format!("f2t-self-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let target = Target::resolve(None).unwrap();

        let link = dir.join("hex2bin");
        try_symlink(&target.exe, &link).unwrap();
        assert!(!target.is_self(&link), "a symlink shim is not the binary");
        assert!(target.is_self(&target.exe), "the binary still is");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn substring_search_finds_edges_and_misses() {
        assert!(contains(b"abcdef", b"abc"));
        assert!(contains(b"abcdef", b"def"));
        assert!(contains(b"abcdef", b"cd"));
        assert!(!contains(b"abcdef", b"dc"));
        assert!(!contains(b"ab", b"abc"));
        assert!(!contains(b"", b"a"));
    }
}
