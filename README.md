# from2to

[![crates.io](https://img.shields.io/crates/v/from2to.svg)](https://crates.io/crates/from2to)
[![docs.rs](https://img.shields.io/docsrs/from2to)](https://docs.rs/from2to)
[![MIT](https://img.shields.io/crates/l/from2to.svg)](LICENSE)

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
installs ~50 short commands — `hex2bin`, `dec2char`, `char2hex`, `hex2b64` —
each of which takes its input from the command line or from stdin.

And when you do not know what you are looking at yet, `whatbase` works it out:

```console
$ whatbase -d bDNhcm5fdGgzX3IwcDM1
l3arn_th3_r0p35
```

## Install

Requires a [Rust toolchain](https://rustup.rs) (1.85 or newer).

```console
$ cargo install from2to
$ from2to install
```

The first command gives you one binary, `from2to`. The second creates the
per-conversion commands next to it and prints where they went:

```console
installed 52 commands via symlink in /home/you/.cargo/bin
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

It is a separate step because writing fifty files into a directory on your
`PATH` should be something you ask for, not something a package manager does to
you behind your back.

### Installing somewhere else

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
| Number bases | `bin` `oct` `dec` `hex`, in every ordered pair: `hex2bin`, `dec2hex`, `bin2oct`, … |
| Text | `char` (alias `ascii`): `dec2char`, `hex2char`, `char2hex`, `dec2ascii`, … |
| Encodings | `b64` `b32`: `hex2b64`, `b642char`, `char2b32`, `b642hex`, … |
| Generic | `f2t <from> <to>` for any base 2–36 |
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
candidates. Short inputs are genuinely ambiguous and it will say so rather than
pick a winner it cannot justify:

```console
$ whatbase -a ff
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
-k, --keep-going  skip tokens that fail instead of stopping
-q, --quiet       suppress warnings
-n, --no-newline  no trailing newline
    --strict      reject prefixes, _ spacers and unpadded base64
```

A bad digit is an error and exits 1. `-k` downgrades it to a warning on stderr,
skips the token, and exits 0 — you asked it to keep going.

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

MIT — see [LICENSE](LICENSE).
