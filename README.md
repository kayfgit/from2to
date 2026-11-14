# from2to

[![CI](https://github.com/kayfgit/from2to/actions/workflows/ci.yml/badge.svg)](https://github.com/kayfgit/from2to/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/from2to.svg)](https://crates.io/crates/from2to)
[![docs.rs](https://img.shields.io/docsrs/from2to)](https://docs.rs/from2to)
[![License](https://img.shields.io/crates/l/from2to.svg)](LICENSE)

<img width="800" height="400" alt="from2to" src="https://github.com/user-attachments/assets/e93e4775-87fa-48f2-ba11-3e4f4a351fe5" />

Conversion that fits in a pipe. Every command is named for what it does:
`<from>2<to>`.

```console
$ hex2bin ff
11111111

$ nc example.com 4000 | dec2char
flag{d3c0d3d_1n_0n3_p1p3}
```

No Python REPL, no `awk`, no looking up `printf` format strings. One binary
installs ~83 short commands — `hex2bin`, `dec2char`, `char2hex`, `hex2b64` —
each of which takes its input from the command line or from stdin.

And when you do not know what you are looking at yet, `whatbase` works it out:

```console
$ whatbase -d bDNhcm5fdGgzX3IwcDM1
l3arn_th3_r0p35
```

## Install

Every route gives you one binary, `from2to`; `from2to install` then creates the
per-conversion commands. See [why the second step exists](#why-the-second-step-exists).

**A prebuilt binary** — no toolchain needed. Grab the archive for your platform
from [the latest release](https://github.com/kayfgit/from2to/releases/latest),
check it against `SHA256SUMS`, and put `from2to` somewhere on your `PATH`. The
Linux builds are static musl, so they run in stripped-down containers and
rescue shells too.

**Debian, Ubuntu, Kali, Parrot** — `.deb` for `amd64` and `arm64`:

```console
$ sudo dpkg -i from2to_0.5.0_amd64.deb
$ from2to install
```

**Fedora, RHEL** — `.rpm` from the same release page, via `sudo rpm -i`.

**Arch, BlackArch** — from the AUR:

```console
$ yay -S from2to-bin
$ from2to install
```

**Windows** — with [Scoop](https://scoop.sh):

```console
> scoop bucket add from2to https://github.com/kayfgit/from2to
> scoop install from2to
> from2to install --dir "$env:USERPROFILE\scoop\shims"
```

The `--dir` matters here: Scoop puts the binary in its own app directory, which
is not the one on your `PATH`. `from2to install` tells you when the directory it
picked is not on `PATH`, so you will not be left guessing.

**From source**, with a [Rust toolchain](https://rustup.rs) (1.85 or newer):

```console
$ cargo install from2to
$ from2to install
```

The second command creates the per-conversion commands and prints where they
went:

```console
installed 84 commands via symlink in /home/you/.cargo/bin
```

Open a new shell so they are picked up, then check it worked:

```console
$ hex2bin ff
11111111
```

Building from a clone works the same way — `cargo install --path .` from inside
the checkout, since that command reads `Cargo.toml` from the current directory.

### Why the second step exists

The commands are links to the one executable, so the whole set costs no extra
disk space. On Unix they are symlinks; on Windows, hardlinks, because creating
a symlink there needs Developer Mode or an elevated prompt. `--method
symlink|hardlink|copy` overrides that, and it falls back through the list if
your filesystem will not do the first choice.

It is a separate step because writing sixty-odd files into a directory on your
`PATH` should be something you ask for, not something a package manager does to
you behind your back.

### Installing somewhere else

By default the commands go next to the binary. When that directory is not
yours to write to — which is the case for the `.deb`, `.rpm` and AUR packages,
where the binary lands in `/usr/bin` — they go to `$XDG_BIN_HOME`, or
`~/.local/bin` if that is unset. Either way the chosen directory is printed,
and you are told if it is not on your `PATH`.

`from2to install --dir <path>` puts the commands wherever you like — useful if
you would rather keep `~/.cargo/bin` tidy. Putting that directory on your `PATH`
is then up to you. `--dry-run` lists what it would create without touching
anything.

### Updating and removing

```console
$ cargo install from2to --force
```

On Unix that is the whole update: the commands are symlinks, so they follow the
new binary and keep working. **On Windows** they are hardlinks pointing at the
old executable, so run `from2to install` again afterwards to refresh them — it
needs no flags.

`from2to uninstall` removes them again. Both directions identify their own work
by a marker compiled into the binary, so they recognize any version they
installed while leaving an unrelated `hex2bin` of your own strictly alone —
that one needs `--force` before install will touch it.

## Commands

| Group | Names |
|---|---|
| Number bases | `bin` `oct` `dec` `hex`, in every pair: `hex2bin`, `dec2hex`, `bin2oct`, … |
| Arithmetic | any number base to itself: `hex2hex '4438+1a'`, `dec2dec`, … |
| Text | `char` (alias `ascii`): `dec2char`, `hex2char`, `char2hex`, `dec2ascii`, … |
| Encodings | `b64` `b32`: `hex2b64`, `b642char`, `char2b32`, `b642hex`, … |
| Escaped hex | `esc` (alias `fhex`): `char2esc`, `esc2char`, `hex2esc`, … |
| Generic | `f2t <from> <to>` for any base 2–36 |
| Byte order | `le`/`be` on either half: `lehex2dec`, `hex2lebin`, `char2lehex`, … |
| Detection | `whatbase <text>` — when you do not know the format yet |
| Shorthands | `f2t 1628` = hex→oct, `f2t 10216` = dec→hex, … (not installed as commands) |

`from2to list` prints all of them.

### The numeric shorthands

`<from>2<to>`, using `2` as the word "to": `1628` is 16→8. These exist only for
bases **2, 8, 10 and 16**, and that limit is deliberate. With `2` serving as
both a base and the separator, those sixteen names each have exactly one valid
reading — but `216` would be both 2→16 and 21→6. The restriction is what keeps
them unambiguous, so the set cannot be extended. Use `f2t 21 6` for the rest.

**They are not installed as commands.** A file called `822` or `10216` sitting
in a directory on your `PATH` is a poor citizen, and it is the one name shape a
package manager would rightly object to. They are only a spelling `f2t`
understands:

```console
$ f2t 1628 ff
377
```

That also settles an old wart: **PowerShell** parses a bare `1628` as the number
1628 rather than a command, so it could never have run them without ceremony.
`f2t 1628 ff` works everywhere.

### Generic form

```console
$ f2t 36 10 zz          # any base 2-36
1295
$ f2t hex bin ff        # word names work too
11111111
$ f2t hex2bin ff        # so does a whole command name
11111111
```

## When you don't know the format: `whatbase`

Every command above needs you to already know what you are holding. `whatbase`
is the other direction — paste the blob and be told what it plausibly is:

```console
$ whatbase bDNhcm5fdGgzX3IwcDM1
b64     l3arn_th3_r0p35
        also reads as b64url (identical here)
```

The answer, and what it says. A positional base that does not spell anything
gives you the number instead, which is almost always what you wanted next:

```console
$ whatbase deadbeef
hex     3735928559
```

**A second line only appears when it is earned** — so silence is itself a
claim of confidence. Two things earn it: a twin that decodes identically, as
above, or a rival close enough that choosing between them is a real decision:

```console
$ whatbase 123456
dec     123456
        also reads as oct (42798)
```

That is the case worth being loud about. `123456` is a perfectly good octal
number meaning something completely different, and the scores are 53 to 49 —
too close to quietly pick one. Meanwhile `deadbeef` scores hex 59 to char 35
and is not in any doubt, so nothing is added.

`--why` shows how it decided — the scores, the evidence, and the command that
finishes the job:

```console
$ whatbase --why bDNhcm5fdGgzX3IwcDM1
b64       96%   l3arn_th3_r0p35
         all 20 characters are in the base64 alphabet
         length 20 is a multiple of 4
         decodes to printable text
         b64url reads the same here — no + / - _ to tell them apart

  try: b642char bDNhcm5fdGgzX3IwcDM1

  could also be: char 26%
```

`f2t detect` is the same thing spelled out, for when you have not installed the
shims.

| Flag | Effect |
|---|---|
| `-w`, `--why` | show the scores, the evidence and the command to run |
| `-d`, `--decode` | skip the report, print what the best guess decodes to |
| `-q`, `--quiet` | print just the format name, for scripts |
| `-a`, `--all` | every candidate, including the implausible ones |
| `--to <fmt>` | with `--decode`, decode into something other than `char` |

So the one-liner for a challenge string is:

```console
$ whatbase -d bDNhcm5fdGgzX3IwcDM1
l3arn_th3_r0p35
```

and the name it prints is one the converter accepts, so the two compose:

```console
$ f2t "$(whatbase -q "$blob")" char "$blob"
```

### How it decides

Three questions, none of which is enough alone:

1. **Does the alphabet admit it?** A hard gate — one `z` rules out hex.
2. **Is it the *narrowest* alphabet that admits it?** `01010101` sits inside
   base64's alphabet perfectly happily, but nobody writes base64 that way. This
   is what makes the answer `bin`, and what stops `1234567890` reading as hex.
3. **Does the payload look like anything?** The question that separates base64
   from base32hex on a string both of them accept: only one decodes to words.

Question 3 counts asymmetrically, on purpose. For base64 and base32, a decode
that produces noise is real evidence against the guess — those encodings exist
to carry something. For a positional base it is not: `1234567890` is a perfectly
good decimal number and owes nobody a printable decoding.

The score is a confidence, and only means anything next to the other
candidates — which is exactly why it is not in the default output. `59%` on its
own tells you nothing; `59` against `25` tells you the answer is not in doubt.
`--why` shows the comparison, and `-a` adds every reading including the
implausible ones:

```console
$ whatbase --why -a ff
hex       59%
         1 token of nothing but base-16 digits
         2 digits — a whole number of bytes
         the decoding is not obviously text

  try: hex2char ff

char      25%
b32       23%
b64       20%
b32hex    20%
```

`-a` on its own gives the same list, terse — one line per reading:

```console
$ whatbase -a 123456
dec     123456
oct     42798
hex     1193046
char    (already text)
b64
```

Two deliberate limits. `whatbase` never guesses at ciphers — a rot13 string is
text, and it will tell you it is text. And an interior *space* rules out the
encodings entirely, since it is the strongest hint in the string that you are
looking at words; line breaks do not, so wrapped base64 still works.

## Input

Values come from the arguments, or from stdin when there are none:

```console
$ hex2dec ff 10
255
16

$ printf '255\n16\n' | hex2dec
597
22
```

Numbers may be separated by whitespace, newlines or commas, and may carry a
`0x` / `0b` / `0o` prefix or `_` spacers. `--strict` turns that leniency off.

For `char` input, whitespace is *data*, not a separator — except for one
trailing newline from a pipe, which is dropped so that `echo hi | char2hex`
does the obvious thing.

## Output

Output streams as it is produced, so a slow producer shows up live:

```console
$ nc example.com 4000 | dec2char   # prints as bytes arrive, not at EOF
```

The one exception is base64/base32 *output*, which has to see the whole input
before it can place its padding.

Separators are chosen to be the useful thing rather than a uniform thing:

| Conversion | Default | Why |
|---|---|---|
| `hex2dec` | newline per result | one line in, one line out |
| `dec2char` | nothing | the point is to reassemble a flag |
| `char2hex` | nothing | `68656c6c6f` reads like `xxd -p` |
| `char2dec` | space | `104 101 108 108 111` is unreadable otherwise |

`-d <s>` sets it explicitly, `-j` joins with nothing.

## Leading zeros

Between two power-of-two bases the digits you wrote carry real width
information, so it is preserved:

```console
$ hex2bin 0f
00001111
$ hex2bin -W 0f      # bare numeric
1111
$ hex2bin -w 12 0f
000000001111
```

The same rule keeps a leading zero byte alive through an encoding:

```console
$ hex2b64 0068656c6c6f
AGhlbGxv
$ hex2b64 68656c6c6f
aGVsbG8=
```

Base 10 has no such structure, so decimal results are always bare.

## Arithmetic

Reading disassembly means doing math on addresses, and that used to be two
conversions wrapped around a step you did in your head. Now it is one command,
because a value can be an expression:

```console
$ hex2hex '4438+1a'
4452
$ hex2hex '(4500 - 4438)/2'    # how many words is that jump?
64
$ dec2hex '1000*4'
fa0
```

Bare literals are read in the **source** base, and the answer comes out in the
target one — so the whole thing stays in hex, or crosses over, exactly as the
command name says:

```console
$ hex2dec '4438+1a'
17490
$ hex2bin 'ff & 0f'
1111
```

**Quote your expressions.** Almost every operator — `*` `&` `|` `<` `>` `(` `)`
— is also a shell metacharacter, and an unquoted `hex2hex '1000*4'` is a glob
error rather than a number. A leading unary minus needs `--`:

```console
$ hex2hex -- '-5+3'
-2
```

**Subtraction is spelled with a space.** A `-` with digits hard up against it
on both sides is punctuation inside one value, not an operator:

```console
$ hex2hex '4500 - 4438'
c8
$ hex2hex '4500-4438'
from2to: '4500-4438': a '-' between two digits is part of the value, not
subtraction (a UUID or a MAC address is written that way, and would otherwise
read as a sum); put spaces around it to subtract
```

The rule exists because hex-shaped identifiers are punctuated with dashes and
every other character in them is a hex digit, so the dash is the only thing
that tells them apart from a chain of subtractions:

```console
$ hex2dec 550e8400-e29b-41d4-a716-446655440000     # a UUID, not a sum
$ hex2dec 00-11-22-33-44-55                        # a MAC address, not a sum
```

Both are an error, as they were before arithmetic existed. Answering them with
a plausible number and an exit status of zero is the one thing this tool will
not do. Only `-` needs the rule — nothing is written with a `+` in the middle
of it, so `4438+1a` needs no spaces.

The operators are C's, and so is their precedence — loosest first:

| | Operators |
|---|---|
| loosest | `\|` |
| | `^` (exclusive or, **not** a power) |
| | `&` |
| | `<<` `>>` |
| | `+` `-` |
| | `*` `/` `%` |
| tightest | unary `-` `~`, then `( )` |

Shifts binding looser than `+` is a famous C trap — `1<<2+3` is 32, not 7 — and
it is reproduced deliberately. The point of this is to agree with the code you
are reading, not to have better opinions than it.

Division truncates toward zero, so `-7/2` is `-3`, which is what the debugger
next to you says.

A prefix never overrides the source base. `0b1` in hex is `0xb1` — `b` is a hex
digit — and it means the same thing on its own as it does inside an expression:

```console
$ hex2dec 0b1
177
$ hex2dec '0b1+0'
177
```

That is the whole rule, and it is worth stating plainly because the alternative
is worse. In hex, `0d26` is already the perfectly good number `0x0d26`; letting
a prefix win would mean the tool silently choosing one of two readings of your
address. To add decimal 26 to a hex address, write `1a`.

### Negatives, and `--word` as a register

Without `--word` the arithmetic is signed and arbitrarily large, so a negative
result simply prints with a sign:

```console
$ hex2hex '4438-4500'
-c8
$ hex2dec '1-5'
-4
```

With `--word N` the whole thing happens in an unsigned **N-byte register**
instead, wrapping after every operation — which is what the 16-bit machine you
are looking at actually does:

```console
$ hex2hex --word 2 '4438-4500'
ff38
$ hex2hex --word 2 '~0'
ffff
$ dec2hex --word 4 '0-1'
ffffffff
```

In a register `>>` is a logical shift and `-x` is the two's complement; without
one, `>>` sign-extends and `~x` is `-x-1`. The register width is also the
written width, so the result is padded out to say how wide it is:

```console
$ hex2hex 'ff & 0f'
f
$ hex2hex --word 2 'ff & 0f'
000f
```

This is the same `--word` that groups bytes for a byte-order swap, because it is
the same claim about the same value — "this is a four-byte word". The two
compose:

```console
$ ledec2hex --word 4 '1+1'
2000000
```

A negative reaching something with no way to say so is an error, not a
magnitude quietly standing in:

```console
$ hex2char '1-5'
hex2char: '1-5' is negative, and char cannot express that; add --word N to wrap it to N bytes
```

### What is and is not an expression

An operator character is the whole trigger, and every one of them was a hard
error before this existed — so nothing that worked has changed meaning. A
newline or a comma still separates values; a space does not:

```console
$ hex2dec ff 10          # no operator: two numbers, as always
255
16
$ hex2hex '1+1, 2+2'     # two expressions
2
4
```

Only a **positional source** has expressions. `char2hex '1+1'` is still the hex
of those three characters, because `char` has no digits to compute with. And
`--strict` turns expression math off entirely, alongside the other leniencies,
for when the input is meant to be pure data.

Expressions are per line from a pipe, so `printf '1+1\n2+2\n' | hex2hex` gives
two answers.

## Byte order

Either half of a name can carry an `le` or `be` prefix. This is the part that
matters for security work: a value pulled out of a memory dump, a ROP gadget
address, a struct field read off the wire.

```console
$ lehex2dec 0d0c0b0a        # 0x0a0b0c0d
168496141
$ lehex2hex 0d0c0b0a        # the pure byte swap
0a0b0c0d
$ hex2lebin ff00
0000000011111111
$ lehex2char 6f6c6c6568
hello
```

`be` is the default written down — it changes nothing, and exists so a script
can be explicit rather than rely on a default it cannot see. Only the `le`
names are installed as commands, but every combination resolves through `f2t`,
so `f2t leb642hex` works whether or not you ran `from2to install`.

A byte order only means something when the **byte width** is known, and that
width comes from how the source wrote the value:

| Source | Width | |
|---|---|---|
| `char`, `b64`, `b32` | the run of bytes is its own width | reads to end of input |
| `hex`, `bin` | byte-aligned, so the digits you wrote are a byte count | `lehex2dec ff` is `255` — one byte has no byte order |
| `dec`, `oct`, everything else | none of their own | an error until `--word` says how wide |

That last row is deliberate. 255 is one byte and 256 is two, so inferring a
width from the value would make the word size depend on the value — which is
how a swap silently hands you the wrong address:

```console
$ ledec2hex 258
ledec2hex: dec carries no byte width of its own; say how wide the word is: --word 4
```

Numbers swap **per token**: `lehex2hex 0102 0304` is two results, not a
re-reading of four bytes. An odd hex digit count is an error rather than a
guess, since there is no byte boundary to swap on — `-k` downgrades that to a
warning like any other bad token.

### `--word`

Groups the bytes into fixed-size words and reverses within each, leaving the
order of the words alone. This is a hexdump off a 32-bit little-endian machine:

```console
$ lehex2hex --word 4 0d0c0b0a78563412
0a0b0c0d12345678
$ dec2lehex --word 4 1234
d2040000
```

`--word` counts *bytes of the value*; `--width` counts *digits of the output*.
They are independent and may both be given.

Two things worth knowing in advance. `-W` strips the leading zeros of the
rendered result, which after a swap are the zeros that used to be trailing, so
`lehex2hex -W 0100` is `1` rather than `0001`. And reversing UTF-8 bytes is not
reversing characters — `char2lehex é` gives `a9c3`, which is no longer valid
UTF-8.

## Escaped hex: `esc`

The `\x48\x65` notation, in both directions. This is the format payloads are
written in, and the one you paste back out of them:

```console
$ char2esc hello
\x68\x65\x6c\x6c\x6f

$ esc2char '\x66\x6c\x61\x67'
flag
```

Every byte is escaped on the way out, never "printable as-is, the rest
escaped". Uniform output round-trips and never leaves you wondering whether a
backslash was data.

Reading is deliberately more forgiving than writing, because what you copy out
of an exploit script is **mixed** — literal text with escapes threaded through
it, and every byte of it meant:

```console
$ esc2hex 'GET /\r\n'
474554202f0d0a
```

The short escapes `\n \r \t \0 \\` are understood on the way in. `--strict`
accepts nothing but `\xNN`.

It composes with byte order, which is where it earns its place — this is
`p32(1234)` from pwntools, and it agrees with `struct.pack('<I', 1234)` byte
for byte:

```console
$ dec2leesc --word 4 1234
\xd2\x04\x00\x00
```

`whatbase` recognizes it on sight, since nothing else looks like it:

```console
$ whatbase '\x68\x65\x6c\x6c\x6f'
esc     hello
```

A backslash alone is not enough, though. `\t` and `\n` are a tab and a newline
in most languages, which would make every Windows path (`C:\temp\new`) read as
escaped bytes — so a real `\xNN` is required before `esc` is even a candidate.

## char: bytes or code points?

`hex2char` reads bytes, because hex digits come in byte-sized pairs.
`dec2char` reads Unicode code points, because decimal digits do not:

```console
$ hex2char 68656c6c6f
hello
$ dec2char 128512
😀
```

`-b` forces bytes, `-c` forces code points.

## Flags

```
-w, --width N     pad each result to N digits
-W, --no-width    drop the natural digit width
-g, --group N     group output digits in runs of N
-u, --upper       uppercase the output
-x, --prefix      prefix with 0x / 0b / 0o
-d, --delim S     separator between results (\n, \t escapes work)
-j, --join        no separator
-b, --bytes       treat char as raw bytes
-c, --codepoint   treat char as Unicode code points
    --word BYTES  bytes per word, for the le/be names
-k, --keep-going  skip tokens that fail instead of stopping
-q, --quiet       suppress warnings
-n, --no-newline  no trailing newline
    --strict      reject prefixes, _ spacers and unpadded base64
```

A bad digit is an error and exits 1. `-k` downgrades it to a warning on stderr,
skips the token, and exits 0 — you asked it to keep going.

`--strict` also turns off [expression math](#arithmetic), and `--word` doubles
as the register width there.

## Arbitrary size

Numbers are not limited to 64 or 128 bits:

```console
$ dec2hex 340282366920938463463374607431768211456
100000000000000000000000000000000
```

Conversions between power-of-two bases never build a bignum at all — they are
bit regrouping — so a multi-megabyte hex blob converts in one linear pass.

## Building

```console
$ cargo test
$ cargo build --release
```

Minimum supported Rust version is **1.85**, the floor for edition 2024. CI holds
that honest with a job pinned to it, so a newer language feature slipping in is
a build failure rather than a surprise for someone on an older toolchain.

A man page lives at [`man/from2to.1`](man/from2to.1); a test fails if it drifts
behind the flags. `CHANGELOG.md` records what changed between releases.

## License

Copyright (C) 2026 Kayf

from2to is free software: you can redistribute it and/or modify it under the
terms of the GNU General Public License as published by the Free Software
Foundation, either version 3 of the License, or (at your option) any later
version. See [LICENSE](LICENSE).
