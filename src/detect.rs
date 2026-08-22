//! Working out what a piece of text already is.
//!
//! Every other command in this program needs you to know the source format:
//! `hex2bin` only works if you knew it was hex. This is the other direction —
//! paste the blob from the challenge page and be told what it plausibly is.
//!
//! Three questions decide it, and no one of them is enough on its own:
//!
//! 1. **Does the alphabet admit it?** A hard gate. A `z` rules out hex.
//! 2. **Is it the *narrowest* alphabet that admits it?** `01010101` sits inside
//!    base64's alphabet quite happily, but nobody writes base64 that way.
//!    Parsimony is what makes that answer `bin` instead.
//! 3. **Does the payload look like anything?** The question that separates
//!    base64 from base32hex on a string both of them accept: only one of them
//!    decodes to words.
//!
//! Question 3 is deliberately asymmetric. For base64 and base32, a decode that
//! produces noise is real evidence against the guess — those encodings exist to
//! carry something. For a positional base it is not: `1234567890` is a perfectly
//! good decimal number and owes nobody a printable decoding.

use crate::convert::{Converter, Options};
use crate::format::Format;

/// The formats detection considers, in the order ties break.
const CANDIDATES: [Format; 10] = [
    // First: nothing else looks like `\\x48`, so a match here is decisive.
    Format::Esc,
    Format::HEX,
    Format::B64,
    Format::BIN,
    Format::DEC,
    Format::B32,
    Format::OCT,
    Format::B64Url,
    Format::B32Hex,
    Format::Char,
];

/// Candidates scoring below this are noise and are not reported.
const FLOOR: u32 = 8;

// How the three "does it fit the form" signals are weighted against each other.
const W_PARSIMONY: f64 = 0.45;
const W_STRUCTURE: f64 = 0.35;
const W_FAMILIARITY: f64 = 0.20;

/// One reading of the input, with the evidence behind it.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub format: Format,
    /// Confidence 0..=100. Only meaningful relative to the other candidates.
    pub score: u32,
    /// Why it scored what it did, strongest evidence first.
    pub reasons: Vec<String>,
    /// What the input decodes to, when that is worth showing.
    pub decoded: Option<String>,
    /// A caveat about another format that would read the same way.
    pub note: Option<String>,
    /// Another format that decodes this input identically, so nothing in the
    /// string itself could tell them apart.
    pub twin: Option<Format>,
    /// The plain value, for a positional base short enough to be worth showing.
    ///
    /// `ff` is hex, and the useful thing to say next is `255` — not that the
    /// byte 0xff fails to spell anything.
    pub value: Option<String>,
}

impl Candidate {
    /// The command that turns the input into text, if there is one to run.
    pub fn command(&self) -> Option<String> {
        (self.format != Format::Char).then(|| format!("{}2char", self.format.name()))
    }
}

// ---------------------------------------------------------------- alphabets

/// How many distinct symbols the format can write.
///
/// `Char` is given the printable-ASCII count: it is the widest alphabet here,
/// which is exactly why it is the reading of last resort.
fn alphabet_size(f: Format) -> u32 {
    match f {
        Format::Positional(r) => r,
        Format::B32 | Format::B32Hex => 32,
        Format::B64 | Format::B64Url => 64,
        Format::Char => 95,
        // Not really an alphabet: the syntax is what identifies it, so give it
        // hex's width and let structure do the work.
        Format::Esc => 16,
    }
}

/// How often this format is what someone actually pasted. A tie-breaker, not a
/// driver: it is the smallest of the three weights.
fn familiarity(f: Format, body: &str) -> f64 {
    match f {
        Format::HEX => 1.0,
        // Unmistakable when present — nothing else looks like `H`.
        Format::Esc => 1.0,
        Format::B64 => 0.95,
        Format::BIN => 0.9,
        Format::DEC => 0.9,
        Format::B32 => 0.8,
        Format::OCT => 0.6,
        // Unremarkable until its distinguishing characters actually turn up,
        // at which point it is the only reading left.
        Format::B64Url => {
            if body.contains(['-', '_']) {
                1.0
            } else {
                0.6
            }
        }
        Format::B32Hex => 0.5,
        Format::Positional(_) => 0.4,
        Format::Char => 0.5,
    }
}

/// Symbols per group, and the group lengths a valid unpadded encoding can end on.
fn encoding_shape(f: Format) -> (usize, &'static [usize]) {
    match f {
        Format::B64 | Format::B64Url => (4, &[0, 2, 3]),
        Format::B32 | Format::B32Hex => (8, &[0, 2, 4, 5, 7]),
        _ => unreachable!("not an encoding format"),
    }
}

fn in_alphabet(f: Format, c: char) -> bool {
    match f {
        Format::B64 => c.is_ascii_alphanumeric() || c == '+' || c == '/',
        Format::B64Url => c.is_ascii_alphanumeric() || c == '-' || c == '_',
        Format::B32 => c.is_ascii_alphabetic() || ('2'..='7').contains(&c),
        Format::B32Hex => c.is_ascii_digit() || ('a'..='v').contains(&c.to_ascii_lowercase()),
        _ => unreachable!("not an encoding format"),
    }
}

/// Could this text be the given encoding?
///
/// Stricter than [`Converter`] is about whitespace: the converter throws away
/// every space it sees, which is the right call once you have committed to a
/// format, but here an interior space is the strongest hint in the string that
/// you are looking at words rather than a blob. Line breaks still pass, since
/// wrapped base64 is everywhere.
fn encoding_admits(f: Format, body: &str) -> bool {
    if body.is_empty() || body.chars().any(char::is_whitespace) {
        return false;
    }
    let core = body.trim_end_matches('=');
    let pad = body.len() - core.len();
    if core.is_empty() || core.contains('=') {
        return false;
    }
    if !core.chars().all(|c| in_alphabet(f, c)) {
        return false;
    }
    let (unit, tails) = encoding_shape(f);
    let tail = core.len() % unit;
    if !tails.contains(&tail) {
        return false;
    }
    // Padding, when present, must be exactly what fills the last group.
    pad == 0 || pad == (unit - tail) % unit
}

/// Could this token be a number in the given base? Leniency matches the
/// converter's: an `0x`-style prefix and `_` spacers are allowed.
fn token_admits(tok: &str, base: u32) -> bool {
    let cleaned = crate::convert::clean_token(tok, base, false);
    !cleaned.is_empty() && cleaned.chars().all(|c| c.is_digit(base))
}

/// Escaped hex has to actually be escaped hex: at least one `\\xNN`, and
/// every backslash in the string accounted for. That second half is what
/// stops a stray backslash in ordinary prose from claiming the format.
fn esc_count(body: &str) -> usize {
    body.matches("\\x").count()
}

fn esc_admits(body: &str) -> bool {
    body.contains("\\x") && crate::convert::decode_esc_ok(body)
}

fn admits(f: Format, tokens: &[&str], body: &str) -> bool {
    match f {
        Format::Char => true,
        Format::Positional(r) => !tokens.is_empty() && tokens.iter().all(|t| token_admits(t, r)),
        Format::Esc => esc_admits(body),
        enc => encoding_admits(enc, body),
    }
}

// ---------------------------------------------------------------- payload

/// How much does this text look like something a person meant to read?
fn text_plausibility(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    let mut good = 0.0f64;
    let mut n = 0.0f64;
    for c in s.chars() {
        n += 1.0;
        good += match c {
            // What `from_utf8_lossy` leaves behind: bytes that were not text.
            '\u{FFFD}' => -1.0,
            ' '..='~' => 1.0,
            '\n' | '\t' | '\r' => 0.8,
            c if c.is_control() => -0.5,
            // Printable, but not what a flag usually looks like.
            _ => 0.5,
        };
    }
    let ratio = (good / n).clamp(0.0, 1.0);
    if ratio > 0.85 && flagish(s) { 1.0 } else { ratio }
}

/// Does this read like a CTF answer rather than an accident?
fn flagish(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    if lower.contains("flag") || lower.contains("ctf") || (s.contains('{') && s.contains('}')) {
        return true;
    }
    // `l3arn_th3_r0p35`: underscores joining alphanumerics, nothing else.
    s.contains('_')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// How much does this text look like prose, as opposed to a blob that merely
/// happens to be printable? The only thing keeping `char` from claiming every
/// base64 string it sees.
fn prose_likeness(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    let mut p: f64 = 0.0;
    if s.contains(' ') || s.contains(|c: char| ".,!?;:'\"".contains(c)) {
        p += 0.5;
    }
    let letters: Vec<char> = s.chars().filter(|c| c.is_alphabetic()).collect();
    if !letters.is_empty() {
        let vowels = letters
            .iter()
            .filter(|c| "aeiouAEIOU".contains(**c))
            .count() as f64;
        let ratio = vowels / letters.len() as f64;
        if (0.25..=0.55).contains(&ratio) {
            p += 0.3;
        }
    }
    // Mixed case *and* digits, with no separators, is the signature of an
    // encoded blob rather than of anything written by hand.
    let mixed = s.chars().any(|c| c.is_ascii_uppercase()) && s.chars().any(|c| c.is_ascii_lowercase());
    if !(mixed && s.chars().any(|c| c.is_ascii_digit())) {
        p += 0.2;
    }
    if flagish(s) {
        p += 0.4;
    }
    p.min(1.0)
}

/// Decode the input under one candidate and judge what came out.
///
/// Returns the plausibility of the payload and, when it is worth showing, the
/// decoded text itself.
/// A positional input rendered in base 10, when that is cheap and useful.
///
/// Base 10 restating itself looks redundant, but it is the half of the
/// comparison that makes the other half mean anything: `dec 123456` next to
/// `oct 42798` is the whole reason to warn about the ambiguity at all.
///
/// Capped at one short token on purpose. Converting a megabyte of hex to
/// decimal is a bignum operation quadratic in the input, and nobody asked for
/// the decimal expansion of a memory dump.
fn base_ten(f: Format, input: &str) -> Option<String> {
    const MAX_DIGITS: usize = 32;
    if !f.is_positional() {
        return None;
    }
    let tok = input.trim();
    if tok.is_empty() || tok.len() > MAX_DIGITS || tok.split_whitespace().count() != 1 {
        return None;
    }
    Converter::new(f, Format::DEC, Options::default()).convert(tok).ok()
}

fn payload(f: Format, input: &str) -> (f64, Option<String>) {
    if f == Format::Char {
        return (prose_likeness(input), None);
    }
    let conv = Converter::new(f, Format::Char, Options::default());
    match conv.convert(input) {
        Ok(text) => {
            let p = text_plausibility(&text);
            // A number is under no obligation to spell anything.
            let p = if f.is_positional() { p.max(0.5) } else { p };
            let show = (p >= 0.6 && !text.is_empty()).then(|| preview(&text));
            (p, show)
        }
        // Only reachable for positional bases: a value too large to be a code
        // point. That says nothing about whether the base was right.
        Err(_) => (if f.is_positional() { 0.5 } else { 0.05 }, None),
    }
}

/// A single printable line, short enough to sit in a report.
fn preview(s: &str) -> String {
    const MAX: usize = 60;
    let mut out = String::new();
    for c in s.chars() {
        if out.chars().count() >= MAX {
            out.push('…');
            break;
        }
        match c {
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => out.push('.'),
            c => out.push(c),
        }
    }
    out
}

// ---------------------------------------------------------------- structure

/// Does the input have the *shape* the format is normally written in? Returns
/// a 0..1 score and the evidence for it.
fn structure(f: Format, tokens: &[&str], body: &str) -> (f64, Vec<String>) {
    match f {
        // Text has no shape to check, so the evidence is whatever made it look
        // like text in the first place.
        Format::Char => {
            let mut reasons = Vec::new();
            if flagish(body) {
                reasons.push("already reads like a flag".into());
            } else if body.contains(' ') {
                reasons.push("spaces between words, not one run of symbols".into());
            }
            (1.0, reasons)
        }
        Format::Esc => {
            let n = esc_count(body);
            // A run that is nothing but escapes is unambiguous; escapes mixed
            // into literal text are still esc, just less emphatically.
            let covered = (n * 4) as f64 / body.len().max(1) as f64;
            if covered > 0.95 {
                (1.0, vec![format!("{n} escaped bytes and nothing else")])
            } else {
                (0.75, vec![format!("{n} escaped bytes mixed with literal text")])
            }
        }
        Format::Positional(r) => positional_structure(r, tokens),
        enc => {
            let (unit, _) = encoding_shape(enc);
            let n = body.len();
            if body.ends_with('=') {
                (1.0, vec![format!("padded out to a multiple of {unit}")])
            } else if n % unit == 0 {
                (0.9, vec![format!("length {n} is a multiple of {unit}")])
            } else {
                (
                    0.6,
                    vec![format!("length {n} is a valid unpadded length")],
                )
            }
        }
    }
}

fn positional_structure(r: u32, tokens: &[&str]) -> (f64, Vec<String>) {
    let mut reasons = Vec::new();
    let mut score: f64 = 0.5;

    // An explicit prefix settles the question by itself.
    let prefixed = base_prefix(r)
        .filter(|p| tokens.iter().any(|t| t.len() > 2 && t[..2].eq_ignore_ascii_case(p)));
    if let Some(p) = prefixed {
        return (1.0, vec![format!("written with an {p} prefix")]);
    }

    let widths: Vec<usize> = tokens.iter().map(|t| t.trim_matches('_').len()).collect();
    let uniform = widths.windows(2).all(|w| w[0] == w[1]).then(|| widths[0]);

    if r == 10 {
        // Base 10 carries no byte structure, so the only structural evidence
        // is whether the values land in a range that means something.
        let vals: Vec<u128> = tokens.iter().filter_map(|t| t.parse().ok()).collect();
        if vals.len() == tokens.len() && !vals.is_empty() {
            if vals.iter().all(|v| *v <= 255) {
                score = 0.85;
                reasons.push("every value fits in a byte".into());
            } else if vals.iter().all(|v| *v <= 0x10FFFF) {
                score = 0.7;
                reasons.push("every value is a possible code point".into());
            } else {
                // Not evidence against base 10 — a large number is still a
                // number — so this stays neutral rather than counting against.
                reasons.push("the values are too large to be characters".into());
            }
        }
        return (score, reasons);
    }

    let bits = Format::Positional(r).bits_per_digit();
    match (uniform, bits) {
        (Some(w), Some(b)) if (w as u32 * b) % 8 == 0 && tokens.len() > 1 => {
            let bytes = w as u32 * b / 8;
            score = 1.0;
            reasons.push(format!(
                "every group is {w} digits — {bytes} byte{} each",
                if bytes == 1 { "" } else { "s" }
            ));
        }
        // Weaker than the multi-token case above: one run of digits that
        // happens to divide into bytes is a coincidence about as often as it
        // is a fact about the format.
        (_, Some(b)) if tokens.len() == 1 => {
            let total = widths[0] as u32 * b;
            if total % 8 == 0 {
                score = 0.65;
                reasons.push(format!("{} digits — a whole number of bytes", widths[0]));
            } else {
                score = 0.45;
                reasons.push(format!("{} digits is not a whole number of bytes", widths[0]));
            }
        }
        _ => {}
    }
    (score, reasons)
}

fn base_prefix(r: u32) -> Option<&'static str> {
    match r {
        2 => Some("0b"),
        8 => Some("0o"),
        16 => Some("0x"),
        _ => None,
    }
}

// ---------------------------------------------------------------- detection

/// Rank the ways this text could be read, best first.
///
/// Returns an empty list only for empty input: `char` admits everything else,
/// so there is always at least the answer "it is already text".
pub fn detect(input: &str) -> Vec<Candidate> {
    let text = input.trim();
    if text.is_empty() {
        return Vec::new();
    }

    let tokens: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|t| !t.is_empty())
        .collect();
    // Line breaks are how long base64 is wrapped, so they do not break a blob;
    // any other whitespace does, and `encoding_admits` will say so.
    let body: String = text.chars().filter(|c| !matches!(c, '\n' | '\r')).collect();

    let fitting: Vec<Format> = CANDIDATES
        .into_iter()
        .filter(|&f| admits(f, &tokens, &body))
        .collect();
    // The narrowest alphabet that fits is the yardstick everything else is
    // measured against.
    let narrowest = fitting
        .iter()
        .map(|&f| alphabet_size(f))
        .min()
        .unwrap_or(95) as f64;

    let mut out: Vec<Candidate> = Vec::new();
    for f in fitting {
        // How much of its alphabet the format had to leave unused. `deadbeef`
        // sits inside base64's 64 symbols, but it only ever reaches for 16 of
        // them, and the reading that needs no excuse wins.
        let parsimony = narrowest / alphabet_size(f) as f64;
        let (structure, mut reasons) = structure(f, &tokens, &body);
        let (plaus, decoded) = payload(f, text);

        let base = W_PARSIMONY * parsimony + W_STRUCTURE * structure + W_FAMILIARITY * familiarity(f, &body);
        // The payload never adds, it only withholds: a format that fits
        // perfectly but decodes to noise keeps roughly a third of its score.
        let score = (100.0 * base * (0.35 + 0.65 * plaus)).round() as u32;

        reasons.insert(0, alphabet_reason(f, &tokens, &body));
        if f != Format::Char {
            reasons.push(match (&decoded, plaus) {
                (Some(_), _) => "decodes to printable text".into(),
                (None, p) if p < 0.4 => "decodes to bytes that are not text".into(),
                _ => "the decoding is not obviously text".into(),
            });
        }

        out.push(Candidate {
            format: f,
            score,
            value: base_ten(f, text),
            twin: None,
            reasons,
            decoded,
            note: None,
        });
    }

    merge_url_twin(&mut out, &body);
    out.retain(|c| c.score >= FLOOR);
    // Sort is stable, so an exact tie falls back to CANDIDATES order.
    out.sort_by_key(|c| std::cmp::Reverse(c.score));
    out
}

fn alphabet_reason(f: Format, tokens: &[&str], body: &str) -> String {
    match f {
        Format::Char => "printable text as it stands".into(),
        Format::Esc => format!("written as {} \\xNN escapes", esc_count(body)),
        Format::Positional(r) => format!(
            "{} token{} of nothing but base-{r} digits",
            tokens.len(),
            if tokens.len() == 1 { "" } else { "s" }
        ),
        enc => format!(
            "all {} characters are in the {} alphabet",
            body.len(),
            alphabet_label(enc)
        ),
    }
}

/// The spelling to use in prose, where `b32hex` reads as a typo.
fn alphabet_label(f: Format) -> &'static str {
    match f {
        Format::B64 => "base64",
        Format::B64Url => "url-safe base64",
        Format::B32 => "base32",
        Format::B32Hex => "base32hex",
        other => other.name(),
    }
}

/// base64 and base64url differ only in two characters. When neither turns up,
/// the two readings are the same string, so report one and mention the other
/// rather than pretending to have distinguished them.
fn merge_url_twin(cands: &mut Vec<Candidate>, body: &str) {
    if body.contains(['-', '_', '+', '/']) {
        return;
    }
    let has_both = cands.iter().any(|c| c.format == Format::B64)
        && cands.iter().any(|c| c.format == Format::B64Url);
    if !has_both {
        return;
    }
    cands.retain(|c| c.format != Format::B64Url);
    if let Some(c) = cands.iter_mut().find(|c| c.format == Format::B64) {
        c.twin = Some(Format::B64Url);
        c.note = Some("b64url reads the same here — no + / - _ to tell them apart".into());
    }
}

// ---------------------------------------------------------------- reporting

/// The human-readable answer. Pure: the caller does the printing.
///
/// `all` widens it from "the answer, plus what else it could be" to every
/// candidate that scored at all.
/// A rival is worth naming when it scores within this much of the winner.
///
/// Tuned against real inputs rather than picked: `123456` reads as decimal
/// 123456 or octal 42798 (a 0.92 ratio) and the difference matters, while
/// `deadbeef` scores hex 59 to char 35 (0.59) and is not in any doubt. The
/// line sits between them, so silence here is itself a claim of confidence.
const RIVAL_RATIO: f64 = 0.75;

/// The default report: the answer, and a caveat only when one is earned.
///
/// The verbose form explains itself before it answers, which is backwards for
/// the common case — you already suspect what the blob is and want it read.
pub fn report_terse(cands: &[Candidate], all: bool) -> String {
    if cands.is_empty() {
        return "nothing to identify\n".to_string();
    }
    let best = &cands[0];

    if all {
        let mut s = String::new();
        for c in cands {
            s.push_str(format!("{:<7} {}", c.format.to_string(), shown(c)).trim_end());
            s.push('\n');
        }
        return s;
    }

    let mut s = format!("{:<7} {}", best.format.to_string(), shown(best));
    s.truncate(s.trim_end().len());
    s.push('\n');

    // Two ways to earn the second line: a twin that decodes identically, or a
    // rival close enough that picking between them is a real decision.
    if let Some(t) = best.twin {
        s.push_str(&format!("        also reads as {t} (identical here)\n"));
    } else if let Some(r) = cands.get(1).filter(|r| {
        best.score > 0 && f64::from(r.score) / f64::from(best.score) >= RIVAL_RATIO
    }) {
        let what = shown(r);
        s.push_str(&format!("        also reads as {}", r.format));
        if !what.is_empty() {
            s.push_str(&format!(" ({what})"));
        }
        s.push('\n');
    }
    s
}

/// What goes in the second column: the text it spells, else the number it is.
///
/// A single decoded character loses to the number, because one printable byte
/// is usually a coincidence — `01010101` is far more useful as `85` than as
/// `U`. Two characters is where a decoding starts to look intended.
fn shown(c: &Candidate) -> String {
    let spelled = c
        .decoded
        .as_ref()
        .filter(|d| d.chars().count() > 1 || c.value.is_none());
    match (spelled, &c.value, c.format) {
        (Some(d), _, _) => d.clone(),
        (None, Some(v), _) => v.clone(),
        (None, None, Format::Char) => "(already text)".to_string(),
        _ => String::new(),
    }
}

pub fn report(input: &str, cands: &[Candidate], all: bool) -> String {
    let mut s = String::new();
    if cands.is_empty() {
        return "nothing to identify\n".to_string();
    }

    let best = &cands[0];
    s.push_str(&format!("{:<8} {:>3}%", best.format.to_string(), best.score));
    match (&best.decoded, best.format) {
        (Some(d), _) => s.push_str(&format!("   {d}\n")),
        (None, Format::Char) => s.push_str("   (already text — nothing to decode)\n"),
        _ => s.push('\n'),
    }
    for r in &best.reasons {
        s.push_str(&format!("         {r}\n"));
    }
    if let Some(n) = &best.note {
        s.push_str(&format!("         {n}\n"));
    }
    if let Some(cmd) = best.command() {
        s.push_str(&format!("\n  try: {}\n", run_line(&cmd, input)));
    }

    let rest = &cands[1..];
    if !rest.is_empty() {
        s.push('\n');
        if all {
            for c in rest {
                let line = format!(
                    "{:<8} {:>3}%   {}",
                    c.format.to_string(),
                    c.score,
                    c.decoded.as_deref().unwrap_or("")
                );
                s.push_str(line.trim_end());
                s.push('\n');
            }
        } else {
            let names: Vec<String> = rest
                .iter()
                .take(3)
                .map(|c| format!("{} {}%", c.format, c.score))
                .collect();
            s.push_str(&format!("  could also be: {}\n", names.join(", ")));
        }
    }
    s
}

/// The command to suggest. Long or multi-line input goes through a pipe rather
/// than being echoed back at the reader.
fn run_line(cmd: &str, input: &str) -> String {
    let one_line = !input.contains('\n');
    if one_line && input.len() <= 48 && !input.contains(' ') {
        format!("{cmd} {input}")
    } else if one_line && input.len() <= 48 {
        format!("{cmd} '{input}'")
    } else {
        format!("{cmd}   (or pipe the text into it)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn best(input: &str) -> Format {
        detect(input).first().expect("no candidates").format
    }

    fn score_of(input: &str, f: Format) -> u32 {
        detect(input)
            .into_iter()
            .find(|c| c.format == f)
            .map(|c| c.score)
            .unwrap_or(0)
    }

    // ---- the case that started it: a picoCTF base64 string

    #[test]
    fn identifies_a_base64_flag() {
        let cands = detect("bDNhcm5fdGgzX3IwcDM1");
        assert_eq!(cands[0].format, Format::B64);
        assert_eq!(cands[0].decoded.as_deref(), Some("l3arn_th3_r0p35"));
        assert!(cands[0].score >= 85, "scored only {}", cands[0].score);
        // And by a wide margin over whatever came second.
        assert!(cands[0].score > cands[1].score + 30);
    }

    #[test]
    fn the_suggested_command_actually_works() {
        let input = "bDNhcm5fdGgzX3IwcDM1";
        let c = &detect(input)[0];
        assert_eq!(c.command().as_deref(), Some("b642char"));
        let (from, to) = crate::names::resolve(&c.command().unwrap()).unwrap();
        // The detector never suggests a byte order, so both ends are big-endian.
        let out = Converter::new(from.format, to.format, Options::default())
            .convert(input)
            .unwrap();
        assert_eq!(out, "l3arn_th3_r0p35");
    }

    // ---- one format per shape

    #[test]
    fn identifies_each_encoding() {
        assert_eq!(best("aGVsbG8="), Format::B64);
        assert_eq!(best("NBSWY3DP"), Format::B32);
        assert_eq!(best("68656c6c6f"), Format::HEX);
        assert_eq!(best("48 65 6c 6c 6f"), Format::HEX);
        assert_eq!(best("104 101 108 108 111"), Format::DEC);
        assert_eq!(best("01101000 01101001"), Format::BIN);
    }

    #[test]
    fn parsimony_beats_the_wider_alphabet() {
        // Every one of these fits base64's alphabet too.
        assert_eq!(best("01010101"), Format::BIN);
        assert_eq!(best("1234567890"), Format::DEC);
        assert_eq!(best("deadbeef"), Format::HEX);
    }

    #[test]
    fn url_safe_base64_is_recognized_by_its_own_characters() {
        // "??>>" round-tripped through base64url: contains - and _.
        let encoded = Converter::new(Format::Char, Format::B64Url, Options::default())
            .convert("~~~>>>???")
            .unwrap();
        assert!(encoded.contains(['-', '_']), "{encoded} is not distinguishing");
        assert_eq!(best(&encoded), Format::B64Url);
    }

    #[test]
    fn the_two_base64_flavours_are_merged_when_indistinguishable() {
        let cands = detect("bDNhcm5fdGgzX3IwcDM1");
        assert!(!cands.iter().any(|c| c.format == Format::B64Url));
        assert!(cands[0].note.as_ref().unwrap().contains("b64url"));
    }

    // ---- text that is not an encoding at all

    #[test]
    fn plain_text_is_reported_as_plain_text() {
        assert_eq!(best("hello world"), Format::Char);
        assert_eq!(best("picoCTF{th1s_1s_th3_fl4g}"), Format::Char);
        assert_eq!(best("l3arn_th3_r0p35"), Format::Char);
    }

    #[test]
    fn text_never_wins_against_a_real_encoding() {
        assert!(score_of("bDNhcm5fdGgzX3IwcDM1", Format::Char) < 50);
        assert!(score_of("aGVsbG8=", Format::Char) < 50);
    }

    // ---- gating

    #[test]
    fn an_out_of_alphabet_character_eliminates_a_format() {
        assert_eq!(score_of("zz", Format::HEX), 0);
        assert_eq!(score_of("0189", Format::OCT), 0);
        assert_eq!(score_of("0102", Format::BIN), 0);
        // A length base64 can never have.
        assert_eq!(score_of("abcde", Format::B64), 0);
    }

    #[test]
    fn interior_spaces_rule_out_a_blob_but_line_breaks_do_not() {
        assert_eq!(score_of("aGVs bG8=", Format::B64), 0);
        let wrapped = "aGVsbG8g\nd29ybGQh";
        assert_eq!(best(wrapped), Format::B64);
        assert_eq!(detect(wrapped)[0].decoded.as_deref(), Some("hello world!"));
    }

    #[test]
    fn a_prefix_settles_the_base() {
        assert_eq!(best("0x41424344"), Format::HEX);
        assert_eq!(best("0b01000001"), Format::BIN);
    }

    // ---- edges

    #[test]
    fn empty_input_has_no_candidates() {
        assert!(detect("").is_empty());
        assert!(detect("   \n ").is_empty());
    }

    #[test]
    fn candidates_come_back_sorted() {
        let cands = detect("68656c6c6f");
        for pair in cands.windows(2) {
            assert!(pair[0].score >= pair[1].score);
        }
    }

    /// Nothing else in the wild looks like `\\x48`, so when it is there the
    /// answer should not be in doubt.
    #[test]
    fn escaped_hex_is_recognized_on_sight() {
        assert_eq!(report_terse(&detect("\\x68\\x65\\x6c\\x6c\\x6f"), false), "esc     hello\n");
        let c = &detect("\\x41\\x42\\x43")[0];
        assert_eq!(c.format, Format::Esc);
        assert!(c.score > 90, "score was {}", c.score);
    }

    /// A Windows path is full of things that look like short escapes — `\\temp`
    /// is a tab, `\\new` is a newline. Requiring a real `\\xNN` is what keeps
    /// the detector from mangling one.
    #[test]
    fn a_backslash_alone_does_not_make_it_escaped_hex() {
        for s in [r"C:\temp\new", r"GET /\r\n", "hello world", "68656c6c6f"] {
            assert_ne!(detect(s)[0].format, Format::Esc, "{s} should not read as esc");
        }
    }

    /// The line between "worth mentioning" and "noise" — the reason terse
    /// output can stay quiet without hiding a real ambiguity.
    #[test]
    fn a_rival_is_named_only_when_it_is_close() {
        // dec 123456 vs oct 42798: different answers, too close to call.
        let close = report_terse(&detect("123456"), false);
        assert!(close.contains("also reads as oct (42798)"), "{close}");

        // hex is not in any doubt here, so nothing is added.
        assert_eq!(report_terse(&detect("68656c6c6f"), false), "hex     hello\n");
        assert_eq!(report_terse(&detect("deadbeef"), false), "hex     3735928559\n");
    }

    /// A twin decodes identically, so no evidence could ever separate them —
    /// that is worth saying however confident the score is.
    #[test]
    fn an_indistinguishable_twin_is_always_named() {
        let r = report_terse(&detect("bDNhcm5fdGgzX3IwcDM1"), false);
        assert_eq!(
            r,
            "b64     l3arn_th3_r0p35\n        also reads as b64url (identical here)\n"
        );
    }

    #[test]
    fn a_positional_base_shows_its_value_when_it_is_not_text() {
        assert_eq!(report_terse(&detect("ff"), false), "hex     255\n");
        assert_eq!(report_terse(&detect("01010101"), false), "bin     85\n");
        assert_eq!(report_terse(&detect("hello world"), false), "char    (already text)\n");
    }

    /// Converting a huge blob to decimal is a bignum operation quadratic in the
    /// input, and nobody asked for the decimal expansion of a memory dump.
    #[test]
    fn a_long_blob_is_not_rendered_as_a_decimal_number() {
        let blob = "de".repeat(400);
        let c = &detect(&blob)[0];
        assert_eq!(c.format, Format::HEX);
        assert!(c.value.is_none(), "should not have built a 800-digit decimal");
    }

    #[test]
    fn the_report_names_the_format_and_the_command() {
        let input = "bDNhcm5fdGgzX3IwcDM1";
        let text = report(input, &detect(input), false);
        assert!(text.contains("b64"), "{text}");
        assert!(text.contains("l3arn_th3_r0p35"), "{text}");
        assert!(text.contains("b642char bDNhcm5fdGgzX3IwcDM1"), "{text}");
    }

    #[test]
    fn long_input_is_not_echoed_back_into_the_suggestion() {
        let long = "a".repeat(200);
        assert!(!run_line("hex2char", &long).contains(&long));
        assert!(run_line("hex2char", &long).contains("pipe"));
    }

    #[test]
    fn previews_stay_on_one_short_line() {
        let p = preview("a\nb\tc");
        assert_eq!(p, "a\\nb\\tc");
        assert!(preview(&"x".repeat(200)).chars().count() <= 61);
    }
}
