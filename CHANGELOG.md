# Changelog

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0] — 2026-08-21

Byte order, for the half of this tool's audience that reads memory dumps.

### Added

- **Little-endian conversions.** Either half of a command name may carry an
  `le` or `be` prefix: `lehex2dec 0d0c0b0a` is `168496141`, `hex2lebin` writes
  little-endian binary, `char2lehex` packs a string back to front. `be` is the
  default written down and changes nothing. Fourteen `le` names are installed;
  every other combination still resolves through `f2t`, e.g. `f2t leb642hex`.
- **`--word BYTES`** — group the bytes into fixed-size words and reverse within
  each, leaving the order of the words alone, for a hexdump taken off a 32-bit
  little-endian machine. It also supplies the width that `dec` and `oct` lack.
  Counts *bytes of the value*, where `--width` counts *digits of the output*.
- A `BYTE ORDER` section in the man page and the README.

### Changed

- `from2to list` and `from2to install` now cover 65 and 66 names respectively,
  up from 51 and 52.
- A conversion whose byte width cannot be worked out is now an error naming the
  fix, rather than a guess: `ledec2hex 258` asks for `--word`. 255 is one byte
  and 256 is two, so inferring a width from the value would make the word size
  depend on the value.
- A little-endian conversion whose source is `char`, `b64` or `b32` reads to end
  of input instead of streaming. A swap over a run of bytes has no token
  boundaries to work with, so processing it a line at a time would reverse each
  line on its own. Nothing that streamed before byte order existed has stopped.

## [0.2.0] — 2026-08-13

First release published to crates.io.

### Added

- **`whatbase`** — identify the format of a piece of text instead of converting
  it, for when you do not know what you are holding. Also available as
  `from2to detect` before the commands are installed. `-d` decodes in one step,
  `-q` prints just the format name so it composes with the converter, `-a` shows
  every candidate.
- **`from2to install --method`** — choose `symlink`, `hardlink` or `copy`
  explicitly instead of taking the platform default.
- A man page, `man/from2to.1`, with a test that fails if it drifts behind the
  argument definitions.
- `rust-version = "1.85"`, declared and held honest by CI.

### Changed

- **Unix installs now default to symlinks** rather than hardlinks. This is the
  visible fix for a long-standing annoyance: `cargo install --force` writes a
  *new* binary, and hardlinked shims were left pointing at the old one, so every
  upgrade needed `from2to install` run again. Symlinks follow the binary, so it
  no longer does. Windows still defaults to hardlinks, because creating a
  symlink there needs Developer Mode or an elevated prompt.
- Symlinks beside the binary now point at a bare file name instead of an
  absolute path, so the directory can be moved or renamed as a unit.
- **The numeric shorthands are no longer installed as commands.** A file called
  `822` or `10216` on your `PATH` is a poor citizen, and it is the one name shape
  a package manager would rightly object to. Nothing else changed about them:
  `f2t 1628 ff` still prints `377`. A bare `from2to install` now creates 52
  commands rather than 64.
- `install` checks the target directory is writable up front, rather than
  failing partway through creating fifty files.
- clap's default features are off, dropping 9 of the 16 crates this used to
  build — `anstream`, `anstyle-parse`, `anstyle-query`, `anstyle-wincon`,
  `colorchoice`, `is_terminal_polyfill`, `once_cell_polyfill`, `utf8parse` and
  `windows-sys`.

### Fixed

- **`uninstall` removed nothing when the shims were symlinks.** It skips
  anything that resolves to the running binary, so that it can never delete
  itself — but a symlink resolves there too, so every shim looked like the
  binary. Only the real file counts now.
- A shim left dangling by a binary that moved away is recognized as ours and
  replaced, instead of being reported as someone else's file needing `--force`.

## [0.1.0] — 2026-08-11

Initial release: about sixty `<from>2<to>` commands over bases 2–36, `char`,
base64 and base32, sharing one binary through links.

[Unreleased]: https://github.com/kayfgit/from2to/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/kayfgit/from2to/releases/tag/v0.3.0
[0.2.0]: https://github.com/kayfgit/from2to/releases/tag/v0.2.0
[0.1.0]: https://github.com/kayfgit/from2to/releases/tag/v0.1.0
