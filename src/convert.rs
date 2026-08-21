//! Decoding, converting and encoding.
//!
//! Three paths matter, in order of how often they run:
//!
//! 1. Both sides are powers of two (`hex2bin`, `hex2b64`, `bin2oct`): pure bit
//!    regrouping. No bignum, O(n), and leading zeros survive for free.
//! 2. The value fits in `u128`: the common case for a stream of small numbers,
//!    which is what `dec2char` sees.
//! 3. Anything else: `BigUint`.

use crate::endian::{Endian, swap_words};
use crate::format::Format;
use data_encoding::{BASE32, BASE32HEX, BASE32HEX_NOPAD, BASE32_NOPAD, BASE64, BASE64URL,
                    BASE64URL_NOPAD, BASE64_NOPAD, Encoding};
use num_bigint::BigUint;
use std::fmt;

const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";

fn digit_char(d: u8) -> char {
    DIGITS[d as usize] as char
}

// ---------------------------------------------------------------- errors

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConvError {
    BadDigit { token: String, ch: char, base: u32 },
    EmptyToken,
    NotACodepoint { token: String },
    NotAByte { token: String },
    BadEncoding { format: String, detail: String },
    /// A byte order was asked for where no byte width can be worked out.
    NoByteWidth { format: String },
    /// A byte order was asked for on code points, which are not bytes.
    EndianNeedsBytes,
    /// A token's digits do not add up to a whole number of bytes.
    NotWholeBytes { token: String, base: u32 },
    /// A run of bytes is not a whole number of `--word` sized words.
    RaggedWord { len: usize, word: usize },
    /// A value needs more room than the word size allows.
    TooWide { token: String, want: usize },
}

impl fmt::Display for ConvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConvError::BadDigit { token, ch, base } => {
                write!(f, "'{token}' is not valid base {base} (bad digit '{ch}')")
            }
            ConvError::EmptyToken => write!(f, "empty input token"),
            ConvError::NotACodepoint { token } => {
                write!(f, "'{token}' is not a Unicode code point")
            }
            ConvError::NotAByte { token } => write!(f, "'{token}' does not fit in a byte"),
            ConvError::BadEncoding { format, detail } => {
                write!(f, "invalid {format} input: {detail}")
            }
            ConvError::NoByteWidth { format } => {
                write!(f, "{format} carries no byte width of its own; say how wide the word is: --word 4")
            }
            ConvError::EndianNeedsBytes => {
                write!(f, "byte order applies to bytes, but char is being read as code points here; add --bytes")
            }
            ConvError::NotWholeBytes { token, base } => {
                write!(f, "'{token}' is not a whole number of bytes in base {base}; pad it or add --word")
            }
            ConvError::RaggedWord { len, word } => {
                write!(f, "{len} bytes is not a whole number of {word}-byte words")
            }
            ConvError::TooWide { token, want } => {
                write!(f, "'{token}' does not fit in {want} bytes")
            }
        }
    }
}

impl std::error::Error for ConvError {}

// ---------------------------------------------------------------- magnitude

/// An arbitrary-precision unsigned integer that stays on the stack while it can.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Mag {
    Small(u128),
    Big(BigUint),
}

impl Mag {
    fn mul_add(&mut self, base: u32, digit: u8) {
        match self {
            Mag::Small(v) => {
                match v
                    .checked_mul(base as u128)
                    .and_then(|x| x.checked_add(digit as u128))
                {
                    Some(next) => *v = next,
                    None => {
                        let mut b = BigUint::from(*v);
                        b *= base;
                        b += digit;
                        *self = Mag::Big(b);
                    }
                }
            }
            Mag::Big(b) => {
                *b *= base;
                *b += digit;
            }
        }
    }

    fn to_str_radix(&self, base: u32) -> String {
        match self {
            Mag::Small(v) => small_to_radix(*v, base),
            Mag::Big(b) => b.to_str_radix(base),
        }
    }

    /// Big-endian bytes, minimal length (a single `0` for zero).
    fn to_bytes_be(&self) -> Vec<u8> {
        match self {
            Mag::Small(0) => vec![0],
            Mag::Small(v) => {
                let bytes = v.to_be_bytes();
                let first = bytes.iter().position(|&b| b != 0).unwrap();
                bytes[first..].to_vec()
            }
            Mag::Big(b) => b.to_bytes_be(),
        }
    }

    fn to_u32(&self) -> Option<u32> {
        match self {
            Mag::Small(v) => u32::try_from(*v).ok(),
            // Anything that overflowed u128 is far past u32.
            Mag::Big(_) => None,
        }
    }
}

fn small_to_radix(mut v: u128, base: u32) -> String {
    if v == 0 {
        return "0".to_string();
    }
    let base = base as u128;
    let mut buf = Vec::new();
    while v > 0 {
        buf.push(digit_char((v % base) as u8));
        v /= base;
    }
    buf.iter().rev().collect()
}

// ---------------------------------------------------------------- numbers

/// A number as it was written: the digit values, plus the base they were in.
///
/// Keeping the digits rather than collapsing straight to a value is what makes
/// the power-of-two path possible and what preserves leading zeros.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Num {
    digits: Vec<u8>,
    base: u32,
    /// The original text, kept only for error messages.
    text: String,
}

impl Num {
    fn to_mag(&self) -> Mag {
        let mut m = Mag::Small(0);
        for &d in &self.digits {
            m.mul_add(self.base, d);
        }
        m
    }

    fn bits_per_digit(&self) -> Option<u32> {
        self.base.is_power_of_two().then(|| self.base.trailing_zeros())
    }
}

/// Regroup a digit sequence from one power-of-two base into another, producing
/// exactly `out_len` digits.
///
/// Shorter output than the input's bit length is only ever requested when the
/// dropped leading bits are known to be zero.
fn regroup_to(digits: &[u8], from_bits: u32, to_bits: u32, out_len: usize) -> Vec<u8> {
    let total_in = digits.len() * from_bits as usize;
    let out_bits = out_len * to_bits as usize;
    let pad = out_bits.saturating_sub(total_in);
    let skip = total_in.saturating_sub(out_bits);

    let bits = std::iter::repeat_n(0u8, pad)
        .chain(
            digits
                .iter()
                .flat_map(|&d| (0..from_bits).rev().map(move |i| (d >> i) & 1)),
        )
        .skip(skip);

    let mut out = Vec::with_capacity(out_len);
    let mut cur = 0u8;
    let mut have = 0u32;
    for bit in bits {
        cur = (cur << 1) | bit;
        have += 1;
        if have == to_bits {
            out.push(cur);
            cur = 0;
            have = 0;
        }
    }
    out
}

/// How many bytes a number should occupy, given how it was written.
///
/// For a power-of-two base the written digit count carries real width
/// information (`00706963` is four bytes, not three), so honour it — but never
/// go below what the value actually needs.
fn byte_width(n: &Num, minimal: usize) -> usize {
    match n.bits_per_digit() {
        Some(bits) => minimal.max(n.digits.len() * bits as usize / 8),
        None => minimal,
    }
}

// ---------------------------------------------------------------- options

#[derive(Debug, Clone, Default)]
pub struct Options {
    pub width: Option<usize>,
    pub no_width: bool,
    pub group: Option<usize>,
    pub upper: bool,
    pub prefix: bool,
    pub delim: Option<String>,
    /// `Some(true)` forces byte semantics, `Some(false)` forces code points.
    pub bytes_mode: Option<bool>,
    pub strict: bool,
    /// Skip tokens that fail to convert instead of aborting.
    pub keep_going: bool,
    /// Byte order of the input and of the output. `Big` on both sides is the
    /// identity, and is checked for explicitly so that every conversion that
    /// existed before byte order did still takes the same code path.
    pub from_endian: Endian,
    pub to_endian: Endian,
    /// Bytes per word for the byte-order swap. `None` means "the whole value".
    pub word: Option<usize>,
}

// ---------------------------------------------------------------- decoding

/// Strip the leniency affordances: a base-appropriate prefix and `_` spacers.
///
/// Shared with `detect`, which has to accept exactly what this accepts or it
/// would rule out inputs the converter would have handled.
pub(crate) fn clean_token<'a>(tok: &'a str, base: u32, strict: bool) -> std::borrow::Cow<'a, str> {
    if strict {
        return std::borrow::Cow::Borrowed(tok);
    }
    let prefix = match base {
        2 => "0b",
        8 => "0o",
        10 => "0d",
        16 => "0x",
        _ => "",
    };
    let mut s = tok;
    if !prefix.is_empty()
        && s.len() > prefix.len()
        && s.get(..2).is_some_and(|p| p.eq_ignore_ascii_case(prefix))
    {
        s = &s[2..];
    }
    if s.contains('_') {
        std::borrow::Cow::Owned(s.replace('_', ""))
    } else {
        std::borrow::Cow::Borrowed(s)
    }
}

fn decode_num(tok: &str, base: u32, strict: bool) -> Result<Num, ConvError> {
    let cleaned = clean_token(tok, base, strict);
    if cleaned.is_empty() {
        return Err(ConvError::EmptyToken);
    }
    let mut digits = Vec::with_capacity(cleaned.len());
    for ch in cleaned.chars() {
        match ch.to_digit(base) {
            Some(d) => digits.push(d as u8),
            None => {
                return Err(ConvError::BadDigit {
                    token: tok.to_string(),
                    ch,
                    base,
                });
            }
        }
    }
    Ok(Num {
        digits,
        base,
        text: tok.to_string(),
    })
}

fn encoding_for(f: Format, padded: bool) -> Encoding {
    match (f, padded) {
        (Format::B64, true) => BASE64,
        (Format::B64, false) => BASE64_NOPAD,
        (Format::B64Url, true) => BASE64URL,
        (Format::B64Url, false) => BASE64URL_NOPAD,
        (Format::B32, true) => BASE32,
        (Format::B32, false) => BASE32_NOPAD,
        (Format::B32Hex, true) => BASE32HEX,
        (Format::B32Hex, false) => BASE32HEX_NOPAD,
        _ => unreachable!("not an encoding format"),
    }
}

/// Decode base64/base32 text, tolerating missing padding and (for base32)
/// lowercase input, which real-world CTF strings routinely have.
fn decode_encoding(text: &str, f: Format, strict: bool) -> Result<Vec<u8>, ConvError> {
    let mut s: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    if !strict && matches!(f, Format::B32 | Format::B32Hex) {
        s = s.to_ascii_uppercase();
    }
    if let Ok(bytes) = encoding_for(f, true).decode(s.as_bytes()) {
        return Ok(bytes);
    }
    if !strict {
        let trimmed = s.trim_end_matches('=');
        if let Ok(bytes) = encoding_for(f, false).decode(trimmed.as_bytes()) {
            return Ok(bytes);
        }
    }
    // Re-run the strict decode purely to get a useful error message.
    let err = encoding_for(f, true).decode(s.as_bytes()).unwrap_err();
    Err(ConvError::BadEncoding {
        format: f.name().to_string(),
        detail: err.to_string(),
    })
}

// ---------------------------------------------------------------- converter

/// A configured conversion. Built once, then fed input.
pub struct Converter {
    pub from: Format,
    pub to: Format,
    pub opts: Options,
}

impl Converter {
    pub fn new(from: Format, to: Format, opts: Options) -> Self {
        Converter { from, to, opts }
    }

    /// True when output cannot be emitted incrementally.
    ///
    /// base64/base32 output has to see the whole byte stream before it can
    /// place its padding, so those conversions buffer to EOF. Everything else
    /// streams, which is what makes `nc host port | dec2char` print live.
    pub fn buffers_output(&self) -> bool {
        self.to.is_encoding() || self.swap_needs_whole_input()
    }

    /// True when a byte-order swap has to see the whole input at once.
    ///
    /// A swap over a *run* of bytes has no token boundaries to work with, so
    /// reading it a line at a time would reverse each line independently —
    /// a wrong answer, not merely a slow one. A swap inside a positional token
    /// is safe, because a token is complete before it is converted.
    ///
    /// What decides it is the **source**, not whichever side wears the `le`:
    /// the source is what determines whether `reorder` sees one byte run or a
    /// list of tokens. `char2lehex` swaps a run even though its `le` is on the
    /// hex side, so it has to buffer.
    fn swap_needs_whole_input(&self) -> bool {
        (self.opts.from_endian.is_little() || self.opts.to_endian.is_little())
            && self.from.is_byteish()
    }

    /// Reject a configuration that cannot mean anything, before reading input.
    ///
    /// A byte order needs a byte width. A run of bytes is its own width, and a
    /// byte-aligned base writes a known number of digits per byte — but base
    /// 10 and base 8 carry no width at all, and inferring one from the value
    /// would make the word size depend on the value. That is how a swap
    /// silently produces the wrong address, so it is an error instead.
    pub fn check(&self) -> Result<(), ConvError> {
        if !self.opts.from_endian.is_little() && !self.opts.to_endian.is_little() {
            return Ok(());
        }
        if self.opts.word.is_some_and(|w| w > 0) {
            return Ok(());
        }
        if self.from.is_byteish() || self.from.byte_aligned() {
            return Ok(());
        }
        Err(ConvError::NoByteWidth { format: self.from.to_string() })
    }

    /// Whether input arrives as tokens (positional) or as an opaque run of
    /// text/bytes (`char`, base64, base32).
    pub fn tokenizes_input(&self) -> bool {
        self.from.is_positional()
    }

    /// Should a `char` endpoint be treated as bytes rather than code points?
    ///
    /// Powers of two carry byte structure (`hex2char 706963` is "pic"), while
    /// base 10 does not, so a decimal token is a code point (`dec2char 128512`
    /// is an emoji). `--bytes` / `--codepoint` override this.
    fn char_is_bytes(&self) -> bool {
        let other = if self.from == Format::Char { self.to } else { self.from };
        self.opts.bytes_mode.unwrap_or_else(|| {
            other.is_byteish() || other.bits_per_digit().is_some()
        })
    }

    /// The separator between results, when the caller hasn't chosen one.
    pub fn default_delim(&self) -> &'static str {
        if !self.to.is_positional() {
            // char/b64/b32 output is a single run: joining is the whole point.
            ""
        } else if self.from.is_byteish() {
            // Byte-sourced numbers: xxd-style for hex/bin/oct, spaced for base 10.
            if self.to.bits_per_digit().is_some() { "" } else { " " }
        } else {
            "\n"
        }
    }

    /// The separator actually in use, honouring `--delim` / `--join`.
    pub fn delim(&self) -> &str {
        self.opts.delim.as_deref().unwrap_or_else(|| self.default_delim())
    }

    /// True when input can be processed a line at a time and written straight
    /// out. base64 and base32 need the whole stream at either end.
    pub fn can_stream(&self) -> bool {
        !self.from.is_encoding() && !self.to.is_encoding() && !self.swap_needs_whole_input()
    }

    // ---- number formatting

    fn finish_digits(&self, mut s: String, natural_width: bool) -> String {
        let explicit = self.opts.width.is_some();
        if (self.opts.no_width || explicit) && natural_width {
            let trimmed = s.trim_start_matches('0');
            s = if trimmed.is_empty() { "0".to_string() } else { trimmed.to_string() };
        }
        if let Some(w) = self.opts.width.filter(|w| s.len() < *w) {
            s = "0".repeat(w - s.len()) + &s;
        }
        if let Some(g) = self.opts.group.filter(|g| *g > 0) {
            s = group_digits(&s, g);
        }
        if self.opts.upper {
            s = s.to_ascii_uppercase();
        }
        if self.opts.prefix {
            let p = match self.to.radix() {
                Some(2) => "0b",
                Some(8) => "0o",
                Some(16) => "0x",
                _ => "",
            };
            s = format!("{p}{s}");
        }
        s
    }

    /// Render one number in the target base.
    fn num_to_string(&self, n: &Num, to_base: u32) -> String {
        let to_bits = to_base.is_power_of_two().then(|| to_base.trailing_zeros());
        match (n.bits_per_digit(), to_bits) {
            // Path 1: bit regrouping, no bignum.
            (Some(fb), Some(tb)) => {
                let total_bits = n.digits.len() * fb as usize;
                let out_len = total_bits.div_ceil(tb as usize).max(1);
                let out = regroup_to(&n.digits, fb, tb, out_len);
                let s: String = out.into_iter().map(digit_char).collect();
                self.finish_digits(s, true)
            }
            // Paths 2 and 3: via the magnitude, which picks u128 or BigUint.
            _ => self.finish_digits(n.to_mag().to_str_radix(to_base), false),
        }
    }

    /// Render one byte in the target base, padded to the base's natural byte width.
    fn byte_to_string(&self, b: u8, to_base: u32) -> String {
        match to_base.is_power_of_two().then(|| to_base.trailing_zeros()) {
            Some(tb) => {
                let out_len = 8usize.div_ceil(tb as usize);
                let s: String = regroup_to(&[b], 8, tb, out_len)
                    .into_iter()
                    .map(digit_char)
                    .collect();
                self.finish_digits(s, true)
            }
            None => self.finish_digits(small_to_radix(b as u128, to_base), false),
        }
    }

}

// ------------------------------------------------------------ bytes of a number

/// The bytes of a number, at the width its digits imply.
///
/// Free rather than a method: the body never looked at `self`, and the
/// byte-order pass needs it from outside the impl.
fn num_to_bytes(n: &Num) -> Vec<u8> {
    match n.bits_per_digit() {
        Some(fb) => {
            let minimal = {
                let significant = n.digits.iter().position(|&d| d != 0).unwrap_or(n.digits.len());
                let bits = (n.digits.len() - significant) * fb as usize;
                bits.div_ceil(8).max(1)
            };
            let want = byte_width(n, minimal);
            regroup_to(&n.digits, fb, 8, want)
        }
        None => {
            let bytes = n.to_mag().to_bytes_be();
            let want = byte_width(n, bytes.len());
            let mut out = vec![0u8; want - bytes.len()];
            out.extend_from_slice(&bytes);
            out
        }
    }
}

/// The bytes of `n` at exactly `want` bytes: zero-extended on the left, and an
/// error rather than a silent truncation when the value needs more room.
fn num_to_bytes_at(n: &Num, want: usize) -> Result<Vec<u8>, ConvError> {
    let b = num_to_bytes(n);
    if b.len() > want {
        let cut = b.len() - want;
        if b[..cut].iter().any(|&x| x != 0) {
            return Err(ConvError::TooWide { token: n.text.clone(), want });
        }
        return Ok(b[cut..].to_vec());
    }
    let mut out = vec![0u8; want - b.len()];
    out.extend_from_slice(&b);
    Ok(out)
}

/// Rebuild a number from bytes: pure regrouping for a power-of-two base, and
/// through the magnitude otherwise.
///
/// The digit count comes from the byte count, which is what carries a swapped
/// value's width back into the pipeline so the rest of it never learns that a
/// swap happened.
fn num_from_bytes(bytes: &[u8], base: u32, text: String) -> Num {
    match base.is_power_of_two().then(|| base.trailing_zeros()) {
        Some(tb) => {
            let out_len = (bytes.len() * 8).div_ceil(tb as usize).max(1);
            Num { digits: regroup_to(bytes, 8, tb, out_len), base, text }
        }
        None => {
            let mut m = Mag::Small(0);
            for &b in bytes {
                m.mul_add(256, b);
            }
            let digits = m
                .to_str_radix(base)
                .chars()
                .map(|c| c.to_digit(base).unwrap() as u8)
                .collect();
            Num { digits, base, text }
        }
    }
}

impl Converter {
    // ---- the two directions

    /// How many bytes of this token the swap should cover.
    fn byte_span(&self, n: &Num) -> Result<usize, ConvError> {
        if let Some(w) = self.opts.word.filter(|w| *w > 0) {
            // An explicit word size settles the width outright, and a short
            // value is zero-extended to fill it.
            return Ok(num_to_bytes(n).len().div_ceil(w).max(1) * w);
        }
        match n.bits_per_digit() {
            Some(bits) if 8 % bits == 0 => {
                let total = n.digits.len() * bits as usize;
                if total % 8 != 0 {
                    // An odd hex digit count has no byte boundary to swap on,
                    // and rounding it either way is a guess.
                    return Err(ConvError::NotWholeBytes {
                        token: n.text.clone(),
                        base: n.base,
                    });
                }
                Ok(total / 8)
            }
            // No width of its own. `check` refuses this configuration up
            // front, but the engine must not depend on having been asked:
            // inferring a width from the value here is exactly the silent
            // wrong answer the error exists to prevent.
            _ => Err(ConvError::NoByteWidth { format: Format::Positional(n.base).to_string() }),
        }
    }

    /// Put the value into, or take it out of, little-endian byte order.
    ///
    /// The swap is defined on bytes, so every arm has to say where its bytes
    /// and its width come from. Numbers swap per token; a run of bytes swaps
    /// as one run, or per `--word` chunk.
    fn reorder(&self, s: Stream, warnings: &mut Vec<ConvError>) -> Result<Stream, ConvError> {
        let word = self.opts.word.filter(|w| *w > 0);
        match s {
            Stream::Bytes(mut b) => {
                if swap_words(&mut b, word).is_err() {
                    let e = ConvError::RaggedWord {
                        len: b.len(),
                        word: word.unwrap_or(1),
                    };
                    if !self.opts.keep_going {
                        return Err(e);
                    }
                    warnings.push(e);
                }
                Ok(Stream::Bytes(b))
            }
            Stream::Nums(nums) => {
                let mut out = Vec::with_capacity(nums.len());
                for n in nums {
                    let bytes = self
                        .byte_span(&n)
                        .and_then(|want| num_to_bytes_at(&n, want));
                    let mut bytes = match bytes {
                        Ok(b) => b,
                        Err(e) if self.opts.keep_going => {
                            warnings.push(e);
                            continue;
                        }
                        Err(e) => return Err(e),
                    };
                    // Padded to a multiple of the word size above, so the
                    // ragged case cannot arise here.
                    let _ = swap_words(&mut bytes, word);
                    out.push(num_from_bytes(&bytes, n.base, n.text));
                }
                Ok(Stream::Nums(out))
            }
            // Reversing code points is `rev`, not endianness.
            Stream::Codepoints(_) => Err(ConvError::EndianNeedsBytes),
        }
    }

    /// Decode a chunk of source text into the intermediate form.
    fn decode(&self, input: &str, warnings: &mut Vec<ConvError>) -> Result<Stream, ConvError> {
        match self.from {
            Format::Positional(base) => {
                let mut nums = Vec::new();
                for tok in input.split(|c: char| c.is_whitespace() || c == ',') {
                    if tok.is_empty() {
                        continue;
                    }
                    match decode_num(tok, base, self.opts.strict) {
                        Ok(n) => nums.push(n),
                        Err(e) if self.opts.keep_going => warnings.push(e),
                        Err(e) => return Err(e),
                    }
                }
                Ok(Stream::Nums(nums))
            }
            Format::Char => {
                if self.char_is_bytes() {
                    Ok(Stream::Bytes(input.as_bytes().to_vec()))
                } else {
                    Ok(Stream::Codepoints(input.chars().collect()))
                }
            }
            enc => Ok(Stream::Bytes(decode_encoding(input, enc, self.opts.strict)?)),
        }
    }

    /// Flatten the intermediate form to bytes, for byte-shaped targets.
    fn to_bytes(&self, s: &Stream) -> Result<Vec<u8>, ConvError> {
        Ok(match s {
            Stream::Bytes(b) => b.clone(),
            Stream::Nums(nums) => nums.iter().flat_map(num_to_bytes).collect(),
            Stream::Codepoints(chars) => chars.iter().collect::<String>().into_bytes(),
        })
    }

    /// Convert one chunk. Trailing bytes that form an incomplete UTF-8 sequence
    /// are held in `carry` for the next chunk; recoverable problems are pushed
    /// to `warnings` when `keep_going` is set.
    pub fn convert_chunk(
        &self,
        input: &str,
        carry: &mut Vec<u8>,
        warnings: &mut Vec<ConvError>,
    ) -> Result<String, ConvError> {
        let mut stream = self.decode(input, warnings)?;

        // Normalize the source's byte order in, and the target's out. Between
        // these two lines everything works in reading order and knows nothing
        // about endianness. When both ends are big-endian — which is every
        // conversion that existed before byte order did — neither pass runs.
        if self.opts.from_endian.is_little() {
            stream = self.reorder(stream, warnings)?;
        }
        if self.opts.to_endian.is_little() {
            stream = self.reorder(stream, warnings)?;
        }

        let delim = self.delim().to_string();

        match self.to {
            Format::Positional(base) => {
                let parts: Vec<String> = match &stream {
                    Stream::Nums(nums) => nums.iter().map(|n| self.num_to_string(n, base)).collect(),
                    Stream::Bytes(bytes) => {
                        bytes.iter().map(|&b| self.byte_to_string(b, base)).collect()
                    }
                    Stream::Codepoints(chars) => chars
                        .iter()
                        .map(|c| self.finish_digits(small_to_radix(*c as u128, base), false))
                        .collect(),
                };
                Ok(parts.join(&delim))
            }
            Format::Char => {
                if self.char_is_bytes() {
                    let mut bytes = std::mem::take(carry);
                    bytes.extend(self.to_bytes(&stream)?);
                    Ok(decode_utf8_incremental(bytes, carry))
                } else {
                    let mut out = String::new();
                    match &stream {
                        Stream::Nums(nums) => {
                            for n in nums {
                                match n.to_mag().to_u32().and_then(char::from_u32) {
                                    Some(cp) => out.push(cp),
                                    None if self.opts.keep_going => {
                                        warnings.push(ConvError::NotACodepoint {
                                            token: n.text.clone(),
                                        });
                                    }
                                    None => {
                                        return Err(ConvError::NotACodepoint {
                                            token: n.text.clone(),
                                        });
                                    }
                                }
                            }
                        }
                        Stream::Bytes(b) => out.push_str(&String::from_utf8_lossy(b)),
                        Stream::Codepoints(c) => out.extend(c.iter()),
                    }
                    Ok(out)
                }
            }
            enc => {
                let bytes = self.to_bytes(&stream)?;
                Ok(encoding_for(enc, true).encode(&bytes))
            }
        }
    }

    /// Convert a complete input in one shot, discarding warnings.
    pub fn convert(&self, input: &str) -> Result<String, ConvError> {
        self.convert_all(input).map(|(s, _)| s)
    }

    /// Convert a complete input in one shot, keeping any warnings.
    pub fn convert_all(&self, input: &str) -> Result<(String, Vec<ConvError>), ConvError> {
        let mut carry = Vec::new();
        let mut warnings = Vec::new();
        let mut out = self.convert_chunk(input, &mut carry, &mut warnings)?;
        if !carry.is_empty() {
            out.push_str(&String::from_utf8_lossy(&carry));
        }
        Ok((out, warnings))
    }
}

/// The intermediate form between decoding and encoding.
enum Stream {
    Nums(Vec<Num>),
    Bytes(Vec<u8>),
    Codepoints(Vec<char>),
}

fn group_digits(s: &str, g: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + s.len() / g);
    let first = chars.len() % g;
    let mut i = 0;
    if first > 0 {
        out.extend(&chars[..first]);
        i = first;
    }
    while i < chars.len() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.extend(&chars[i..i + g]);
        i += g;
    }
    out
}

/// Decode bytes to text, holding back a trailing partial UTF-8 sequence so a
/// multi-byte character split across two flushes still comes out right.
fn decode_utf8_incremental(bytes: Vec<u8>, carry: &mut Vec<u8>) -> String {
    carry.clear();
    let mut out = String::new();
    let mut rest = &bytes[..];
    loop {
        match std::str::from_utf8(rest) {
            Ok(s) => {
                out.push_str(s);
                return out;
            }
            Err(e) => {
                let valid = e.valid_up_to();
                out.push_str(unsafe { std::str::from_utf8_unchecked(&rest[..valid]) });
                match e.error_len() {
                    // A genuinely invalid byte: substitute and carry on.
                    Some(n) => {
                        out.push(char::REPLACEMENT_CHARACTER);
                        rest = &rest[valid + n..];
                    }
                    // Truncated at the end: keep it for the next chunk.
                    None => {
                        carry.extend_from_slice(&rest[valid..]);
                        return out;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conv(from: &str, to: &str, input: &str) -> String {
        with_opts(from, to, input, Options::default())
    }

    fn with_opts(from: &str, to: &str, input: &str, opts: Options) -> String {
        let c = Converter::new(
            Format::parse(from).unwrap(),
            Format::parse(to).unwrap(),
            opts,
        );
        c.convert(input).unwrap()
    }

    // ---- byte order

    /// Convert through a command name, the way a shim would.
    fn le(name: &str, input: &str) -> Result<String, ConvError> {
        le_opts(name, input, Options::default())
    }

    fn le_opts(name: &str, input: &str, mut opts: Options) -> Result<String, ConvError> {
        let (from, to) = crate::names::resolve(name).unwrap();
        opts.from_endian = from.endian;
        opts.to_endian = to.endian;
        let c = Converter::new(from.format, to.format, opts);
        c.check()?;
        c.convert(input)
    }

    fn worded(name: &str, input: &str, word: usize) -> Result<String, ConvError> {
        le_opts(name, input, Options { word: Some(word), ..Default::default() })
    }

    #[test]
    fn little_endian_reverses_a_written_word() {
        // 0x0d0c0b0a read back to front is 0x0a0b0c0d.
        assert_eq!(le("lehex2dec", "0d0c0b0a").unwrap(), "168496141");
        assert_eq!(le("lehex2hex", "0d0c0b0a").unwrap(), "0a0b0c0d");
        assert_eq!(le("lehex2char", "6f6c6c6568").unwrap(), "hello");
        assert_eq!(le("hex2lebin", "ff00").unwrap(), "0000000011111111");
    }

    /// A single byte has no byte order, so the swap is a no-op rather than an
    /// error — `lehex2dec ff` must agree with `hex2dec ff`.
    #[test]
    fn one_byte_has_no_byte_order() {
        assert_eq!(le("lehex2dec", "ff").unwrap(), conv("hex", "dec", "ff"));
        assert_eq!(le("lehex2hex", "ff").unwrap(), "ff");
    }

    /// `be` is the default written down, so it must change nothing at all.
    #[test]
    fn big_endian_is_the_identity() {
        assert_eq!(le("behex2dec", "0d0c0b0a").unwrap(), conv("hex", "dec", "0d0c0b0a"));
    }

    /// Two swaps at the same width cancel.
    #[test]
    fn swapping_twice_is_the_identity() {
        assert_eq!(le("lehex2lehex", "0d0c0b0a").unwrap(), "0d0c0b0a");
    }

    /// Each token is its own value; the swap never runs across whitespace.
    #[test]
    fn little_endian_applies_per_token() {
        assert_eq!(le("lehex2hex", "0102 0304").unwrap(), "0201\n0403");
    }

    /// An odd hex digit count has no byte boundary to swap on, and rounding it
    /// either way would be a guess.
    #[test]
    fn a_half_byte_is_refused() {
        assert!(matches!(
            le("lehex2dec", "fff"),
            Err(ConvError::NotWholeBytes { .. })
        ));
        // ...but `-k` downgrades it, like every other token-level failure.
        let opts = Options { keep_going: true, ..Default::default() };
        assert_eq!(le_opts("lehex2dec", "fff 0d0c0b0a", opts).unwrap(), "168496141");
    }

    /// Base 10 has no width of its own: 255 is a byte and 256 is two, so the
    /// word size would otherwise depend on the value.
    #[test]
    fn a_base_without_a_byte_width_is_refused() {
        assert!(matches!(le("ledec2hex", "258"), Err(ConvError::NoByteWidth { .. })));
        assert!(matches!(le("dec2lehex", "1234"), Err(ConvError::NoByteWidth { .. })));
        // Base 8 is a power of two but 3 bits do not divide 8.
        assert!(matches!(le("leoct2hex", "777"), Err(ConvError::NoByteWidth { .. })));
        // A word size settles it.
        assert_eq!(worded("dec2lehex", "1234", 4).unwrap(), "d2040000");
        assert_eq!(worded("ledec2hex", "1234", 4).unwrap(), "d2040000");
    }

    /// `check` is a courtesy to the CLI, not the engine's only guard: a
    /// library caller that skips it must still not get a guessed width.
    #[test]
    fn the_engine_refuses_a_missing_width_even_without_check() {
        let opts = Options { from_endian: Endian::Little, ..Default::default() };
        let c = Converter::new(Format::DEC, Format::HEX, opts);
        assert!(matches!(c.convert("258"), Err(ConvError::NoByteWidth { .. })));
    }

    #[test]
    fn a_word_size_pins_the_width() {
        // `ff` fills a 4-byte word as 000000ff, which reversed is ff000000.
        assert_eq!(worded("lehex2dec", "ff", 4).unwrap(), "4278190080");
        // Words are swapped independently; their order is left alone.
        assert_eq!(worded("lehex2hex", "0d0c0b0a78563412", 4).unwrap(), "0a0b0c0d12345678");
        assert_eq!(worded("lehex2hex", "0d0c0b0a", 2).unwrap(), "0c0d0a0b");
    }

    /// Zero-padding a run of bytes would change its length, which is data
    /// corruption — so it is an error unless the caller asked to keep going.
    #[test]
    fn a_ragged_byte_run_is_refused() {
        assert!(matches!(
            worded("char2lehex", "hello", 4),
            Err(ConvError::RaggedWord { len: 5, word: 4 })
        ));
    }

    /// Reversing code points is `rev`, not endianness.
    #[test]
    fn code_points_have_no_byte_order() {
        assert!(matches!(
            le_opts(
                "char2ledec",
                "hi",
                Options { bytes_mode: Some(false), ..Default::default() }
            ),
            Err(ConvError::EndianNeedsBytes)
        ));
        // As bytes it is perfectly well defined.
        let opts = Options { bytes_mode: Some(true), ..Default::default() };
        assert_eq!(le_opts("char2ledec", "hi", opts).unwrap(), "105 104");
    }

    /// A base64 blob is one value, so the whole decoded run swaps together.
    #[test]
    fn an_encoding_blob_is_one_value() {
        assert_eq!(le("leb642hex", "AQAAAA==").unwrap(), "00000001");
    }

    /// A swap over a *run* of bytes cannot be done a chunk at a time, and what
    /// decides that is the source — not whichever side wears the `le`.
    #[test]
    fn a_swapped_byte_run_cannot_stream() {
        let streams = |name: &str| {
            let (from, to) = crate::names::resolve(name).unwrap();
            let opts = Options {
                from_endian: from.endian,
                to_endian: to.endian,
                ..Default::default()
            };
            Converter::new(from.format, to.format, opts).can_stream()
        };
        // char source: the run has no token boundaries to work with.
        assert!(!streams("char2lehex"));
        assert!(!streams("lechar2hex"));
        // Positional source: a token is complete before it is converted.
        assert!(streams("lehex2dec"));
        assert!(streams("hex2lechar"));
        // Nothing that streamed before byte order existed stops streaming.
        assert!(streams("char2hex"));
        assert!(streams("hex2char"));
    }

    /// The swap must stay linear; it is two extra passes, not a quadratic one.
    #[test]
    fn a_long_swapped_blob_is_not_quadratic() {
        let input = "0d0c0b0a".repeat(20_000);
        let out = le("lehex2hex", &input).unwrap();
        assert_eq!(out.len(), input.len());
    }

    // ---- path 1: power-of-two regrouping

    #[test]
    fn hex_to_bin_keeps_digit_width() {
        assert_eq!(conv("hex", "bin", "0f"), "00001111");
        assert_eq!(conv("hex", "bin", "f"), "1111");
        assert_eq!(conv("hex", "bin", "41 42"), "01000001\n01000010");
    }

    #[test]
    fn hex_to_oct_rounds_bit_width_up() {
        assert_eq!(conv("hex", "oct", "ff"), "377");
        assert_eq!(conv("hex", "oct", "1628"), "013050");
    }

    #[test]
    fn bin_to_hex_packs_nibbles() {
        assert_eq!(conv("bin", "hex", "0100000101000010"), "4142");
        assert_eq!(conv("bin", "hex", "1111"), "f");
    }

    #[test]
    fn no_width_strips_leading_zeros() {
        let opts = Options { no_width: true, ..Default::default() };
        assert_eq!(with_opts("hex", "bin", "0f", opts), "1111");
    }

    #[test]
    fn explicit_width_overrides_natural_width() {
        let opts = Options { width: Some(4), ..Default::default() };
        assert_eq!(with_opts("hex", "bin", "0f", opts), "1111");
        let opts = Options { width: Some(12), ..Default::default() };
        assert_eq!(with_opts("hex", "bin", "0f", opts), "000000001111");
    }

    // ---- paths 2 and 3: magnitude

    #[test]
    fn decimal_conversions() {
        assert_eq!(conv("dec", "hex", "255"), "ff");
        assert_eq!(conv("hex", "dec", "ff"), "255");
        assert_eq!(conv("dec", "bin", "5"), "101");
        assert_eq!(conv("36", "dec", "zz"), "1295");
        assert_eq!(conv("dec", "36", "1295"), "zz");
    }

    #[test]
    fn arbitrary_precision_beyond_u128() {
        // 2^128, which cannot round-trip through the u128 fast path.
        let big = "340282366920938463463374607431768211456";
        assert_eq!(conv("dec", "hex", big), "100000000000000000000000000000000");
        assert_eq!(conv("hex", "dec", "100000000000000000000000000000000"), big);
    }

    #[test]
    fn very_long_hex_blob_is_not_quadratic() {
        // Straight through the regrouping path; no bignum should be built.
        let blob = "ab".repeat(5000);
        let out = conv("hex", "bin", &blob);
        assert_eq!(out.len(), 40000);
        assert!(out.starts_with("10101011"));
    }

    // ---- char semantics

    #[test]
    fn dec_to_char_reads_code_points() {
        assert_eq!(conv("dec", "char", "104\n101\n108\n108\n111"), "hello");
        assert_eq!(conv("dec", "char", "128512"), "\u{1F600}");
    }

    #[test]
    fn hex_to_char_reads_bytes() {
        assert_eq!(conv("hex", "char", "68656c6c6f"), "hello");
        assert_eq!(conv("hex", "char", "68 65 6c 6c 6f"), "hello");
        assert_eq!(conv("bin", "char", "0100000101000010"), "AB");
    }

    #[test]
    fn char_to_numbers() {
        assert_eq!(conv("char", "hex", "hello"), "68656c6c6f");
        assert_eq!(conv("char", "dec", "hello"), "104 101 108 108 111");
        assert_eq!(conv("char", "bin", "A"), "01000001");
        assert_eq!(conv("char", "oct", "A"), "101");
    }

    #[test]
    fn bytes_mode_can_be_forced_either_way() {
        let opts = Options { bytes_mode: Some(false), ..Default::default() };
        assert_eq!(with_opts("hex", "char", "1F600", opts), "\u{1F600}");
        let opts = Options { bytes_mode: Some(true), ..Default::default() };
        assert_eq!(conv("dec", "char", "112"), "p");
        assert_eq!(with_opts("dec", "char", "112", opts), "p");
    }

    // ---- encodings

    #[test]
    fn base64_round_trip() {
        assert_eq!(conv("hex", "b64", "68656c6c6f"), "aGVsbG8=");
        assert_eq!(conv("b64", "char", "aGVsbG8="), "hello");
        assert_eq!(conv("char", "b64", "hello"), "aGVsbG8=");
        assert_eq!(conv("b64", "hex", "aGVsbG8="), "68656c6c6f");
    }

    #[test]
    fn base64_tolerates_missing_padding() {
        assert_eq!(conv("b64", "char", "aGVsbG8"), "hello");
    }

    #[test]
    fn base32_round_trip_and_case_leniency() {
        let encoded = conv("char", "b32", "hello");
        assert_eq!(conv("b32", "char", &encoded), "hello");
        assert_eq!(conv("b32", "char", &encoded.to_ascii_lowercase()), "hello");
    }

    #[test]
    fn leading_zero_bytes_survive_into_base64() {
        // 12 hex digits is 6 bytes, so the zero byte must not be dropped.
        assert_eq!(conv("hex", "b64", "0068656c6c6f"), "AGhlbGxv");
        assert_eq!(conv("hex", "b64", "68656c6c6f"), "aGVsbG8=");
    }

    // ---- formatting options

    #[test]
    fn grouping_and_case_and_prefix() {
        let opts = Options { group: Some(4), ..Default::default() };
        assert_eq!(with_opts("hex", "bin", "ff", opts), "1111 1111");
        let opts = Options { upper: true, ..Default::default() };
        assert_eq!(with_opts("dec", "hex", "255", opts), "FF");
        let opts = Options { prefix: true, ..Default::default() };
        assert_eq!(with_opts("dec", "hex", "255", opts), "0xff");
    }

    #[test]
    fn grouping_handles_uneven_leading_group() {
        assert_eq!(group_digits("11111", 4), "1 1111");
        assert_eq!(group_digits("11111111", 4), "1111 1111");
    }

    #[test]
    fn custom_delimiter() {
        let opts = Options { delim: Some(",".into()), ..Default::default() };
        assert_eq!(with_opts("dec", "hex", "255 16", opts), "ff,10");
    }

    // ---- input leniency

    #[test]
    fn accepts_prefixes_and_underscores() {
        assert_eq!(conv("hex", "dec", "0xff"), "255");
        assert_eq!(conv("bin", "dec", "0b1010"), "10");
        assert_eq!(conv("dec", "hex", "1_000"), "3e8");
        // A bare `0b` in hex is the number 11, not a prefix.
        assert_eq!(conv("hex", "dec", "0b"), "11");
    }

    #[test]
    fn commas_and_newlines_both_separate() {
        assert_eq!(conv("dec", "hex", "255,16"), "ff\n10");
        assert_eq!(conv("dec", "hex", "255\n16"), "ff\n10");
    }

    #[test]
    fn strict_mode_rejects_prefixes() {
        let opts = Options { strict: true, ..Default::default() };
        let c = Converter::new(Format::HEX, Format::DEC, opts);
        assert!(c.convert("0xff").is_err());
    }

    // ---- errors

    #[test]
    fn bad_digits_are_reported_with_context() {
        let c = Converter::new(Format::HEX, Format::BIN, Options::default());
        let err = c.convert("zz").unwrap_err();
        assert_eq!(
            err.to_string(),
            "'zz' is not valid base 16 (bad digit 'z')"
        );
    }

    #[test]
    fn out_of_range_code_points_are_rejected() {
        let c = Converter::new(Format::DEC, Format::Char, Options::default());
        assert!(c.convert("999999999").is_err());
    }

    // ---- streaming

    #[test]
    fn utf8_split_across_chunks_is_reassembled() {
        let c = Converter::new(Format::HEX, Format::Char, Options::default());
        let mut carry = Vec::new();
        let mut warn = Vec::new();
        // U+00E9 is 0xC3 0xA9, fed one byte per chunk.
        let a = c.convert_chunk("c3", &mut carry, &mut warn).unwrap();
        assert_eq!(a, "");
        assert_eq!(carry, vec![0xc3]);
        let b = c.convert_chunk("a9", &mut carry, &mut warn).unwrap();
        assert_eq!(b, "é");
        assert!(carry.is_empty());
    }

    #[test]
    fn keep_going_skips_bad_tokens() {
        let opts = Options { keep_going: true, ..Default::default() };
        let c = Converter::new(Format::HEX, Format::DEC, opts);
        let (out, warnings) = c.convert_all("ff zz 10").unwrap();
        assert_eq!(out, "255\n16");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].to_string().contains("'zz'"));
    }

    #[test]
    fn buffering_is_limited_to_encoding_targets() {
        let c = Converter::new(Format::DEC, Format::Char, Options::default());
        assert!(!c.buffers_output());
        let c = Converter::new(Format::HEX, Format::B64, Options::default());
        assert!(c.buffers_output());
    }
}
