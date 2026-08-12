# from2to

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
installs ~60 short commands — `hex2bin`, `dec2char`, `char2hex`, `hex2b64` —
each of which takes its input from the command line or from stdin.

## Install

Requires a [Rust toolchain](https://rustup.rs). Then, in a shell:

```console
$ git clone https://github.com/kayfgit/from2to
$ cd from2to
$ cargo install --path . && from2to install
```

That is the whole thing. Open a new shell afterwards so the new commands are
picked up, then check it worked:

```console
$ hex2bin ff
11111111
```

### What those commands actually do

1. **`cd from2to`** — the next command reads `Cargo.toml` from the current
   directory, so you have to be inside the clone. This is the step people miss.
2. **`cargo install --path .`** — builds the release binary and copies it to
   Cargo's binary directory (`~/.cargo/bin`, or `%USERPROFILE%\.cargo\bin` on
   Windows). That directory is already on your `PATH` if you installed Rust
   through rustup. At this point you have exactly one command: `from2to`.
3. **`from2to install`** — creates the ~60 per-conversion commands
   (`hex2bin`, `dec2char`, …) next to the binary. They are hardlinks to that one
   executable, so the whole set costs no extra disk space; it falls back to
   symlinks and then to copies where hardlinks are not available. It prints
   where it put them:

   ```console
   installed 63 commands via hardlink in /home/you/.cargo/bin
   ```

### Installing somewhere else

`from2to install --dir <path>` puts the commands wherever you like — useful if
you would rather keep `~/.cargo/bin` tidy. Putting that directory on your
`PATH` is then up to you. `--dry-run` lists what it would create without
touching anything.

### Updating and removing

After changing the code, run **both** commands again:

```console
$ cargo install --path . --force && from2to install
```

`cargo install` writes a *new* binary, which leaves the existing commands
pointing at the old one. The second command refreshes them in place and needs
no flags.

`from2to uninstall` removes them again. Both commands identify their own work
by a marker compiled into the binary, so they recognize any version they
installed while leaving an unrelated `hex2bin` of your own strictly alone —
that one needs `--force` before install will touch it.

## Commands

| Group | Names |
|---|---|
| Number bases | `bin` `oct` `dec` `hex`, in every ordered pair: `hex2bin`, `dec2hex`, `bin2oct`, … |
| Text | `char` (alias `ascii`): `dec2char`, `hex2char`, `char2hex`, `dec2ascii`, … |
| Encodings | `b64` `b32`: `hex2b64`, `b642char`, `char2b32`, `b642hex`, … |
| Shorthands | `1628` = hex→oct, `10216` = dec→hex, `2216` = bin→hex, … |
| Generic | `f2t <from> <to>` for any base 2–36 |

`from2to list` prints all of them.

### The numeric shorthands

`<from>2<to>`, using `2` as the word "to": `1628` is 16→8. These exist only for
bases **2, 8, 10 and 16**, and that limit is deliberate. With `2` serving as
both a base and the separator, those sixteen names each have exactly one valid
reading — but `216` would be both 2→16 and 21→6. The restriction is what keeps
them unambiguous, so the set cannot be extended. Use `f2t 21 6` for the rest.

One shell caveat: **PowerShell** parses a bare `1628` as the number 1628, not as
a command, so `1628 ff` is a syntax error there. Write `1628.exe ff`, `& 1628 ff`
or `f2t 1628 ff`. bash and cmd.exe run them bare with no ceremony.

### Generic form

```console
$ f2t 36 10 zz          # any base 2-36
1295
$ f2t hex bin ff        # word names work too
11111111
$ f2t hex2bin ff        # so does a whole command name
11111111
```

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

## License

MIT — see [LICENSE](LICENSE).
