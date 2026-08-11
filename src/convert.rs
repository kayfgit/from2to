//! Decoding, converting and encoding.
//!
//! Three paths matter, in order of how often they run:
//!
//! 1. Both sides are powers of two (`hex2bin`, `hex2b64`, `bin2oct`): pure bit
//!    regrouping. No bignum, O(n), and leading zeros survive for free.
//! 2. The value fits in `u128`: the common case for a stream of small numbers,
//!    which is what `dec2char` sees.
//! 3. Anything else: `BigUint`.

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
}

// ---------------------------------------------------------------- decoding

/// Strip the leniency affordances: a base-appropriate prefix and `_` spacers.
fn clean_token<'a>(tok: &'a str, base: u32, strict: bool) -> std::borrow::Cow<'a, str> {
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
        self.to.is_encoding()
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
        !self.from.is_encoding() && !self.to.is_encoding()
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

    fn num_to_bytes(&self, n: &Num) -> Vec<u8> {
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

    // ---- the two directions

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
            Stream::Nums(nums) => nums.iter().flat_map(|n| self.num_to_bytes(n)).collect(),
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
        let stream = self.decode(input, warnings)?;
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
        assert_eq!(conv("dec", "char", "112\n105\n99\n111"), "pico");
        assert_eq!(conv("dec", "char", "128512"), "\u{1F600}");
    }

    #[test]
    fn hex_to_char_reads_bytes() {
        assert_eq!(conv("hex", "char", "706963"), "pic");
        assert_eq!(conv("hex", "char", "70 69 63"), "pic");
        assert_eq!(conv("bin", "char", "0100000101000010"), "AB");
    }

    #[test]
    fn char_to_numbers() {
        assert_eq!(conv("char", "hex", "picoCTF"), "7069636f435446");
        assert_eq!(conv("char", "dec", "pico"), "112 105 99 111");
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
        assert_eq!(conv("hex", "b64", "706963"), "cGlj");
        assert_eq!(conv("b64", "char", "cGljb0NURg=="), "picoCTF");
        assert_eq!(conv("char", "b64", "picoCTF"), "cGljb0NURg==");
        assert_eq!(conv("b64", "hex", "cGlj"), "706963");
    }

    #[test]
    fn base64_tolerates_missing_padding() {
        assert_eq!(conv("b64", "char", "cGljb0NURg"), "picoCTF");
    }

    #[test]
    fn base32_round_trip_and_case_leniency() {
        let encoded = conv("char", "b32", "pico");
        assert_eq!(conv("b32", "char", &encoded), "pico");
        assert_eq!(conv("b32", "char", &encoded.to_ascii_lowercase()), "pico");
    }

    #[test]
    fn leading_zero_bytes_survive_into_base64() {
        // 8 hex digits is 4 bytes, so the zero byte must not be dropped.
        assert_eq!(conv("hex", "b64", "00706963"), "AHBpYw==");
        assert_eq!(conv("hex", "b64", "706963"), "cGlj");
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
