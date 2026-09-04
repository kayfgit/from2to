# Changelog

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.0] — 2026-09-03

Arithmetic in the base you are already reading.

### Added

- **Arithmetic.** A positional value may now be an expression, so the math you
  were doing between two conversions is one command:

  ```console
  $ hex2hex '4438+1a'
  4452
  $ hex2hex '(4500-4438)/2'
  64
  ```

  Bare literals are read in the source base and the answer comes out in the
  target one, so `hex2dec '4438+1a'` is `17490`. The operators are C's — `+ - *
  / %`, unary `-` and `~`, `& | ^ << >>` and `( )` — and so is their
  precedence, shifts binding looser than `+` included, because the point is to
  agree with the disassembly rather than to have better opinions than it.

  **No flag turns this on.** Every operator character was a hard digit error
  before, so nothing that worked has changed meaning: `hex2dec ff 10` is still
  two numbers, `char2hex '1+1'` is still `312b31`, and a lone `<` is still a
  bad digit. A newline or a comma separates values; a space does not.
  `--strict` turns it off, alongside the other leniencies.

  **Subtraction is spelled with a space.** A `-` with digits hard up against it
  on both sides is punctuation inside one value, so `hex2hex '4500 - 4438'` is
  `c8` and `hex2hex '4500-4438'` is an error naming the fix. The rule is what
  keeps a hex-shaped identifier from reading as arithmetic: every character in
  `550e8400-e29b-41d4-a716-446655440000` and `00-11-22-33-44-55` is a hex
  digit, so the dash is the only thing separating a UUID and a MAC address
  from a chain of subtractions. Both were a clean digit error before
  arithmetic existed and both still are — answering them with a plausible
  number at exit 0 is the one outcome this tool refuses everywhere else. Only
  `-` needs the rule; nothing is written with a `+` in the middle of it, so
  `hex2hex '4438+1a'` needs no spaces.

  A prefix still never overrides the source base: `0b1` is `0xb1` in hex both
  on its own and inside an expression. In hex `0d26` is already the perfectly
  good number `0x0d26`, and quietly picking one of two readings of an address
  is exactly what this tool refuses to do elsewhere.

- **`--word` is also a register width.** Without it the arithmetic is signed
  and arbitrarily large, so `hex2hex '4438-4500'` is `-c8`. With it, the whole
  evaluation happens in an unsigned N-byte register, wrapping after every
  operation, which is what the 16-bit machine in front of you actually does:

  ```console
  $ hex2hex --word 2 '4438-4500'
  ff38
  $ hex2hex --word 2 '~0'
  ffff
  ```

  In a register `>>` is a logical shift and the result is padded out to the
  width. It is the same claim as the byte-order reading — "this value is N
  bytes wide" — so the two compose: `ledec2hex --word 4 '1+1'` is `2000000`.

- **`hex2hex`, `dec2dec`, `bin2bin` and `oct2oct` are installed.** The identity
  pairs were excluded as no-ops, and arithmetic retires that reason —
  `hex2hex` is the hex calculator. `lehex2hex` set the precedent: an identity
  pair earns a name when it does something. `char2char` and `b642b64` stay
  excluded, since they still do not. A bare `from2to install` now creates 84
  commands rather than 80, and `from2to list` covers 83 names rather than 79.

### Changed

- A negative result is an error rather than a magnitude wherever there is no
  way to say it — into `char`, `b64`, `b32`, `esc`, or a byte-order swap. The
  message names `--word` as the fix, since wrapping to a register is usually
  what was meant.


## [0.4.0] — 2026-08-21

What the tool says, and a format for what payloads are written in.

### Changed

- **`whatbase` answers before it explains.** The default report was six to nine
  lines of reasoning with the actual answer buried in it; it is now the format
  and what the input says, and nothing else:

  ```
  $ whatbase bDNhcm5fdGgzX3IwcDM1
  b64     l3arn_th3_r0p35
  ```

  A second line appears only when it is earned — a twin that decodes
  identically, or a rival close enough that choosing between them is a real
  decision — so silence is itself a claim of confidence. `whatbase 123456`
  still warns that octal reads it as `42798`, because that one is genuinely too
  close to call.
- A positional base whose bytes are not printable now shows its **value**
  rather than a note saying there is nothing to see: `whatbase deadbeef` is
  `hex 3735928559`. A single decoded character loses to the number, since one
  printable byte is usually a coincidence.
- `-a`/`--all` lists every reading in the same terse two-column form.

### Added

- **Escaped hex, `esc`** (aliases `escape`, `fhex`, `xhex`) — the `\x48\x65`
  notation, in both directions: `char2esc`, `esc2char`, `hex2esc` and the rest
  of the cross product. Every byte is escaped on output so the result
  round-trips; on input the short escapes `\n \r \t \0 \\` are understood and
  literal text passes through, because `esc2hex 'GET /\r\n'` is what you
  actually copy out of an exploit script. `--strict` accepts only `\xNN`.
  It composes with byte order, so `dec2leesc --word 4 1234` is `p32(1234)`.
- `whatbase` recognizes `esc` on sight. A real `\xNN` is required before it is
  even a candidate — `\t` and `\n` alone would make every Windows path read as
  escaped bytes.
- **`-w`/`--why`** restores the previous report in full: confidence scores, the
  evidence behind the winning reading, and the command that finishes the job.

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

[Unreleased]: https://github.com/kayfgit/from2to/compare/v0.5.0...HEAD
[0.5.0]: https://github.com/kayfgit/from2to/releases/tag/v0.5.0
[0.4.0]: https://github.com/kayfgit/from2to/releases/tag/v0.4.0
[0.3.0]: https://github.com/kayfgit/from2to/releases/tag/v0.3.0
[0.2.0]: https://github.com/kayfgit/from2to/releases/tag/v0.2.0
[0.1.0]: https://github.com/kayfgit/from2to/releases/tag/v0.1.0
