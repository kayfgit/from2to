//! Integer expression evaluation, for the arithmetic that sits between two
//! conversions.
//!
//! Reading disassembly means doing math on addresses — an offset added to a
//! label, the distance between two jumps halved to get a word count. That was
//! two `from2to` invocations wrapped around a step done in your head, and this
//! is the step.
//!
//! The evaluator is deliberately small: integers only, a closed operator set,
//! no variables and no functions. Bare literals are read in the *source* base,
//! so `hex2hex '4438+1a'` is hex all the way through.

use std::fmt;

use num_bigint::{BigInt, Sign};

use crate::convert::clean_token;

/// Characters that can only ever be an operator.
///
/// Recognized by an explicit set, never by "not a digit in this base" — in
/// base 16 the letters `a`-`f` are digits and in base 36 *every* letter is
/// one, so a word operator like `and` or `shl` could never be told apart from
/// the literal 13801. That is why there are none, and why there must never be.
///
/// `:` and `.` are deliberately absent, so `hex2dec '00:11:22'` and
/// `hex2dec '1.5'` keep the clear `BadDigit` message they give today rather
/// than becoming "unknown operator".
///
/// `-` is present but carries a further restriction; see [`is_tight_minus`].
const OPERATORS: &[char] = &['+', '-', '*', '/', '%', '&', '|', '^', '~', '<', '>', '(', ')'];

/// True when the `-` at `i` has a digit-ish character on *both* sides.
///
/// A dash wedged between two alphanumerics is punctuation inside one value,
/// not subtraction. Hex-shaped identifiers are written exactly that way —
/// `550e8400-e29b-41d4-a716-446655440000` is a UUID and `00-11-22-33-44-55` is
/// a MAC address — and every character in them is a hex digit, so nothing but
/// the dash itself can tell them apart from a chain of subtractions. Before
/// expressions existed both were a clean `BadDigit`; reading them as arithmetic
/// would answer a question nobody asked, with a plausible number, at exit 0.
/// That is the one outcome this tool refuses everywhere else.
///
/// So binary subtraction is spelled with a space: `4500 - 4438`. Only `-`
/// needs the rule, because only `-` is also how identifiers are punctuated;
/// nothing is written with a `+` in the middle of it.
fn is_tight_minus(cs: &[char], i: usize) -> bool {
    let digitish = |c: Option<&char>| matches!(c, Some(&c) if c.is_alphanumeric() || c == '_');
    i > 0 && digitish(cs.get(i - 1)) && digitish(cs.get(i + 1))
}

/// Recursive descent on `((((…` is a stack overflow, which aborts the process
/// instead of raising something catchable. Depth is capped rather than trusted.
const MAX_DEPTH: u32 = 64;

/// A shift wide enough to be an allocation failure rather than an answer.
/// A million bits is 125 KB of result, which is past any real register.
const MAX_SHIFT: u64 = 1_000_000;

/// True when `s` holds a character that can only be an operator.
///
/// A lone `<` or `>` does not count. It is far more likely to be a shell
/// redirect that got through than an intended shift, so it stays the
/// `BadDigit` error it is today unless some other operator has already put us
/// in expression mode.
pub fn has_operator(s: &str) -> bool {
    let cs: Vec<char> = s.chars().collect();
    cs.iter().enumerate().any(|(i, &c)| match c {
        '<' | '>' => cs.get(i + 1) == Some(&c),
        _ => OPERATORS.contains(&c),
    })
}

/// Evaluate `s`, reading bare literals in `base`.
///
/// `word` is a register width in bytes: `Some(n)` evaluates in an `n`-byte
/// unsigned register, reducing mod 2^(8n) after *every* operation, so `>>` is
/// a logical shift and `-` is the two's complement. `None` evaluates in
/// arbitrary-precision signed integers.
pub fn eval(s: &str, base: u32, word: Option<usize>) -> Result<BigInt, ExprError> {
    let toks = tokenize(s, base)?;
    if toks.is_empty() {
        return Err(ExprError::Empty);
    }
    let mut p = Parser {
        toks: &toks,
        pos: 0,
        depth: 0,
        word: word.filter(|w| *w > 0),
    };
    let v = p.expr(1)?;
    match p.peek() {
        None => Ok(v),
        // A value butting up against another value: `ff 10+1` is not one
        // expression, and guessing which operator was meant is not our job.
        Some(Tok::Num(_)) | Some(Tok::LParen) => Err(ExprError::Juxtaposition),
        Some(Tok::RParen) => Err(ExprError::Unopened),
        Some(Tok::Op(_)) => Err(ExprError::Trailing),
    }
}

// ------------------------------------------------------------------- errors

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExprError {
    /// Nothing but whitespace.
    Empty,
    /// A `(` that is never closed.
    Unclosed,
    /// A `)` with no `(` to match.
    Unopened,
    TooDeep,
    /// A character that is neither a digit nor an operator.
    NotAnOperator { ch: char },
    /// `<` or `>` on its own, where `<<` or `>>` was meant.
    LoneAngle { ch: char },
    /// A `-` with digits hard up against it on both sides, as in a UUID.
    TightMinus,
    /// A binary operator with nothing on its right.
    Trailing,
    /// A binary operator with nothing on its left.
    Leading { op: &'static str },
    /// Two values with no operator between them.
    Juxtaposition,
    DivZero,
    RemZero,
    ShiftTooBig { count: String },
    ShiftNegative { count: String },
    /// A prefix or a run of `_` with no digits after it.
    EmptyLiteral { text: String },
    BadDigit { token: String, ch: char, base: u32 },
}

impl fmt::Display for ExprError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExprError::Empty => write!(f, "there is no expression here"),
            ExprError::Unclosed => write!(f, "a '(' is never closed"),
            ExprError::Unopened => write!(f, "a ')' has no matching '('"),
            ExprError::TooDeep => write!(f, "parentheses nested more than {MAX_DEPTH} deep"),
            ExprError::NotAnOperator { ch } => write!(
                f,
                "'{ch}' is not an operator; the operators are + - * / % & | ^ ~ << >> and ( )"
            ),
            ExprError::LoneAngle { ch } => {
                write!(f, "'{ch}' alone is not an operator; use '{ch}{ch}' to shift")
            }
            ExprError::TightMinus => write!(
                f,
                "a '-' between two digits is part of the value, not subtraction \
                 (a UUID or a MAC address is written that way, and would \
                 otherwise read as a sum); put spaces around it to subtract"
            ),
            ExprError::Trailing => write!(f, "the expression ends after an operator"),
            ExprError::Leading { op } => write!(f, "'{op}' needs a value on its left"),
            ExprError::Juxtaposition => write!(
                f,
                "two values with nothing between them; put an operator between them, \
                 or a comma to keep them as separate values"
            ),
            ExprError::DivZero => write!(f, "division by zero"),
            ExprError::RemZero => write!(f, "remainder by zero"),
            ExprError::ShiftTooBig { count } => write!(
                f,
                "a shift of {count} places is past the {MAX_SHIFT} bit limit; \
                 that result would not fit in memory"
            ),
            ExprError::ShiftNegative { count } => write!(
                f,
                "a shift count of {count} is negative; shift the other way instead"
            ),
            ExprError::EmptyLiteral { text } => write!(f, "'{text}' has no digits in it"),
            ExprError::BadDigit { token, ch, base } => {
                write!(f, "'{token}' is not valid base {base} (bad digit '{ch}')")
            }
        }
    }
}

impl std::error::Error for ExprError {}

// -------------------------------------------------------------------- tokens

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Num(BigInt),
    Op(Op),
    LParen,
    RParen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Or,
    Xor,
    And,
    Shl,
    Shr,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    /// Unary only; never returned by `binding`.
    Not,
}

impl Op {
    /// C's precedence, loosest first.
    ///
    /// The shifts sit *below* `+` and `-`, so `1<<2+3` is 32 and not 7. That is
    /// a famous C gotcha and it is reproduced on purpose: the audience for this
    /// reads C and its disassembly all day, and a quietly "corrected"
    /// precedence would be a trap rather than a kindness.
    fn prec(self) -> u8 {
        match self {
            Op::Or => 1,
            Op::Xor => 2,
            Op::And => 3,
            Op::Shl | Op::Shr => 4,
            Op::Add | Op::Sub => 5,
            Op::Mul | Op::Div | Op::Rem => 6,
            Op::Not => 7,
        }
    }

    fn spelling(self) -> &'static str {
        match self {
            Op::Or => "|",
            Op::Xor => "^",
            Op::And => "&",
            Op::Shl => "<<",
            Op::Shr => ">>",
            Op::Add => "+",
            Op::Sub => "-",
            Op::Mul => "*",
            Op::Div => "/",
            Op::Rem => "%",
            Op::Not => "~",
        }
    }
}

fn tokenize(s: &str, base: u32) -> Result<Vec<Tok>, ExprError> {
    let cs: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;

    while i < cs.len() {
        let c = cs[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        match c {
            '(' => {
                out.push(Tok::LParen);
                i += 1;
                continue;
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1;
                continue;
            }
            '<' | '>' => {
                if cs.get(i + 1) != Some(&c) {
                    return Err(ExprError::LoneAngle { ch: c });
                }
                out.push(Tok::Op(if c == '<' { Op::Shl } else { Op::Shr }));
                i += 2;
                continue;
            }
            _ => {}
        }
        if c == '-' && is_tight_minus(&cs, i) {
            return Err(ExprError::TightMinus);
        }
        let op = match c {
            '+' => Some(Op::Add),
            '-' => Some(Op::Sub),
            '*' => Some(Op::Mul),
            '/' => Some(Op::Div),
            '%' => Some(Op::Rem),
            '&' => Some(Op::And),
            '|' => Some(Op::Or),
            '^' => Some(Op::Xor),
            '~' => Some(Op::Not),
            _ => None,
        };
        if let Some(op) = op {
            out.push(Tok::Op(op));
            i += 1;
            continue;
        }
        if !(c.is_alphanumeric() || c == '_') {
            return Err(ExprError::NotAnOperator { ch: c });
        }
        // A run of digits. `clean_token` does the prefix and `_` stripping, so
        // a literal means exactly what the same characters mean on their own.
        let start = i;
        while i < cs.len() && (cs[i].is_alphanumeric() || cs[i] == '_') {
            i += 1;
        }
        let run: String = cs[start..i].iter().collect();
        out.push(Tok::Num(literal(&run, base)?));
    }
    Ok(out)
}

fn literal(run: &str, base: u32) -> Result<BigInt, ExprError> {
    let cleaned = clean_token(run, base, false);
    if cleaned.is_empty() {
        return Err(ExprError::EmptyLiteral {
            text: run.to_string(),
        });
    }
    let mut v = BigInt::from(0u32);
    for ch in cleaned.chars() {
        match ch.to_digit(base) {
            Some(d) => {
                v *= base;
                v += d;
            }
            None => {
                return Err(ExprError::BadDigit {
                    token: run.to_string(),
                    ch,
                    base,
                });
            }
        }
    }
    Ok(v)
}

// -------------------------------------------------------------------- parser

/// Precedence climbing, evaluating as it parses.
///
/// No AST: nothing here re-walks the expression, so building one would only be
/// ceremony.
struct Parser<'a> {
    toks: &'a [Tok],
    pos: usize,
    depth: u32,
    /// Register width in bytes, already filtered to be non-zero.
    word: Option<usize>,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn bump(&mut self) {
        self.pos += 1;
    }

    /// Reduce into the register, if there is one.
    ///
    /// Applied after every operation rather than only at the end, because that
    /// is what makes `>>` mean the same thing as the shift in the disassembly:
    /// `(ffff+1)>>4` is 0 in a 16-bit register, not 1000.
    fn norm(&self, v: BigInt) -> BigInt {
        let Some(w) = self.word else { return v };
        let m = BigInt::from(1) << (8 * w);
        let r = v % &m;
        if r.sign() == Sign::Minus { r + &m } else { r }
    }

    fn expr(&mut self, min_prec: u8) -> Result<BigInt, ExprError> {
        let mut lhs = self.unary()?;
        while let Some(op) = self.binding() {
            if op.prec() < min_prec {
                break;
            }
            self.bump();
            // `prec + 1` on the right-hand side is what makes every binary
            // operator left-associative: `10 - 3 - 2` is 5, not 9.
            let rhs = self.expr(op.prec() + 1)?;
            lhs = self.apply(op, lhs, rhs)?;
        }
        Ok(lhs)
    }

    /// The next token as a *binary* operator, if it can be one.
    fn binding(&self) -> Option<Op> {
        match self.peek() {
            Some(Tok::Op(Op::Not)) => None,
            Some(Tok::Op(op)) => Some(*op),
            _ => None,
        }
    }

    fn unary(&mut self) -> Result<BigInt, ExprError> {
        match self.peek() {
            Some(Tok::Op(Op::Sub)) => {
                self.bump();
                let v = self.unary()?;
                Ok(self.norm(-v))
            }
            Some(Tok::Op(Op::Add)) => {
                self.bump();
                self.unary()
            }
            Some(Tok::Op(Op::Not)) => {
                self.bump();
                let v = self.unary()?;
                // `!v` on a BigInt is two's complement, so `~0` is -1. In a
                // register the reduction turns that into the width's all-ones.
                Ok(self.norm(!v))
            }
            Some(Tok::Op(op)) => Err(ExprError::Leading { op: op.spelling() }),
            Some(Tok::LParen) => {
                if self.depth >= MAX_DEPTH {
                    return Err(ExprError::TooDeep);
                }
                self.bump();
                self.depth += 1;
                let v = self.expr(1)?;
                self.depth -= 1;
                match self.peek() {
                    Some(Tok::RParen) => {
                        self.bump();
                        Ok(v)
                    }
                    _ => Err(ExprError::Unclosed),
                }
            }
            Some(Tok::RParen) => Err(ExprError::Unopened),
            Some(Tok::Num(v)) => {
                let v = v.clone();
                self.bump();
                Ok(v)
            }
            None => Err(ExprError::Trailing),
        }
    }

    fn apply(&self, op: Op, lhs: BigInt, rhs: BigInt) -> Result<BigInt, ExprError> {
        let v = match op {
            Op::Add => lhs + rhs,
            Op::Sub => lhs - rhs,
            Op::Mul => lhs * rhs,
            // Truncating toward zero, as in C and Rust, so `-7/2` is -3. That
            // is what the debugger next to you says.
            Op::Div => {
                if rhs.sign() == Sign::NoSign {
                    return Err(ExprError::DivZero);
                }
                lhs / rhs
            }
            Op::Rem => {
                if rhs.sign() == Sign::NoSign {
                    return Err(ExprError::RemZero);
                }
                lhs % rhs
            }
            Op::Shl => lhs << self.shift_count(&rhs)?,
            // Arithmetic on a signed value; in a register everything is
            // already non-negative, so the same operation is the logical one.
            Op::Shr => lhs >> self.shift_count(&rhs)?,
            Op::And => lhs & rhs,
            Op::Or => lhs | rhs,
            Op::Xor => lhs ^ rhs,
            Op::Not => unreachable!("~ is unary"),
        };
        Ok(self.norm(v))
    }

    fn shift_count(&self, rhs: &BigInt) -> Result<u64, ExprError> {
        if rhs.sign() == Sign::Minus {
            return Err(ExprError::ShiftNegative {
                count: rhs.to_string(),
            });
        }
        let n = u64::try_from(rhs).map_err(|_| ExprError::ShiftTooBig {
            count: rhs.to_string(),
        })?;
        if n > MAX_SHIFT {
            return Err(ExprError::ShiftTooBig {
                count: rhs.to_string(),
            });
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(s: &str, base: u32) -> BigInt {
        eval(s, base, None).unwrap_or_else(|err| panic!("{s} (base {base}): {err}"))
    }

    fn w(s: &str, base: u32, word: usize) -> BigInt {
        eval(s, base, Some(word)).unwrap_or_else(|err| panic!("{s}: {err}"))
    }

    fn err(s: &str, base: u32) -> ExprError {
        eval(s, base, None).expect_err(s)
    }

    fn n(v: i64) -> BigInt {
        BigInt::from(v)
    }

    /// C's precedence, gotchas and all.
    #[test]
    fn precedence_is_the_one_c_uses() {
        assert_eq!(e("1+2*3", 10), n(7));
        assert_eq!(e("2*3+1", 10), n(7));
        // Shifts bind looser than +, which is the surprising half.
        assert_eq!(e("1<<2+3", 10), n(32));
        assert_eq!(e("1+2<<3", 10), n(24));
        // | looser than ^ looser than & looser than <<.
        assert_eq!(e("1|2^3&4", 10), n(3));
        assert_eq!(e("1^1|2", 10), n(2));
        assert_eq!(e("(1+2)*3", 10), n(9));
    }

    #[test]
    fn binary_operators_are_left_associative() {
        assert_eq!(e("10 - 3 - 2", 10), n(5));
        assert_eq!(e("100/5/2", 10), n(10));
        assert_eq!(e("100 - 10+5", 10), n(95));
        assert_eq!(e("64>>2>>1", 10), n(8));
    }

    #[test]
    fn unary_minus_and_not() {
        assert_eq!(e("-5", 10), n(-5));
        assert_eq!(e("- -5", 10), n(5));
        assert_eq!(e("-2*3", 10), n(-6));
        assert_eq!(e("3*-2", 10), n(-6));
        assert_eq!(e("~0", 10), n(-1));
        assert_eq!(e("~~5", 10), n(5));
        assert_eq!(e("~-1", 10), n(0));
        assert_eq!(e("+7", 10), n(7));
    }

    /// The rule the whole tokenizer rests on: operators are a fixed
    /// non-alphanumeric set, so a letter is always a digit and never a name.
    #[test]
    fn letters_are_digits_at_every_radix() {
        assert_eq!(e("4438+1a", 16), n(0x4452));
        assert_eq!(e("ff+1", 16), n(256));
        assert_eq!(e("zz+1", 36), n(1296));
        // One literal, not `a` `and` `c`. If a word operator were ever added,
        // this is the test that would catch it.
        assert_eq!(e("and", 36), n(13_801));
        assert_eq!(e("abc", 36), n(13_368));
    }

    /// A literal means the same thing inside an expression as outside one.
    ///
    /// `clean_token` strips only the *source* base's own prefix, so `0b1` is
    /// 0xb1 in base 16. Letting a prefix override the base here would make
    /// `hex2dec 0b1` (177) disagree with `hex2dec '0b1+0'` (1) for the same
    /// characters, and `0d26` in hex is already the perfectly good number
    /// 0x0d26 — there is no reading that is safe to guess.
    #[test]
    fn a_prefix_never_overrides_the_source_base() {
        assert_eq!(e("0xff+1", 16), n(256));
        assert_eq!(e("0b1+0", 16), n(0xb1));
        assert_eq!(e("0d26+0", 16), n(0x0d26));
        assert_eq!(e("0d26+0", 10), n(26));
        assert_eq!(e("0b101+0", 2), n(5));
    }

    #[test]
    fn underscores_space_out_a_literal() {
        assert_eq!(e("dead_beef+1", 16), n(0xdead_bef0));
        assert_eq!(e("1_000+1", 10), n(1001));
    }

    #[test]
    fn values_outgrow_u128() {
        assert_eq!(e("1<<200", 10), BigInt::from(1) << 200);
        let big = e("ffffffffffffffffffffffffffffffff*2", 16);
        assert_eq!(big, (BigInt::from(1) << 129) - 2);
    }

    /// Truncating toward zero, matching C and Rust rather than Python.
    #[test]
    fn division_truncates_toward_zero() {
        assert_eq!(e("-7/2", 10), n(-3));
        assert_eq!(e("7/-2", 10), n(-3));
        assert_eq!(e("-7%2", 10), n(-1));
        assert_eq!(e("7%-2", 10), n(1));
    }

    #[test]
    fn shifts_are_arithmetic_without_a_register() {
        assert_eq!(e("-8>>1", 10), n(-4));
        assert_eq!(e("-1>>10", 10), n(-1));
        assert_eq!(e("1<<10", 10), n(1024));
    }

    /// The register is the point of `--word`: everything wraps, and the wrap
    /// happens after every operation rather than only at the end.
    #[test]
    fn a_register_wraps_after_every_operation() {
        assert_eq!(w("4438 - 4500", 16, 2), n(0xff38));
        assert_eq!(w("0 - 1", 16, 2), n(0xffff));
        assert_eq!(w("ffff+1", 16, 2), n(0));
        assert_eq!(w("~0", 16, 2), n(0xffff));
        assert_eq!(w("-1", 16, 4), n(0xffff_ffff));
        // Wrap-at-the-end would give 0x1000 here; a 16-bit machine gives 0.
        assert_eq!(w("(ffff+1)>>4", 16, 2), n(0));
        // With everything non-negative, >> is the logical shift.
        assert_eq!(w("~0>>4", 16, 2), n(0x0fff));
    }

    #[test]
    fn division_by_zero_is_an_error_not_a_panic() {
        assert_eq!(err("1/0", 10), ExprError::DivZero);
        assert_eq!(err("1%0", 10), ExprError::RemZero);
        assert_eq!(err("1/(2 - 2)", 10), ExprError::DivZero);
    }

    #[test]
    fn absurd_shifts_are_refused_before_they_allocate() {
        assert!(matches!(err("1<<99999999", 10), ExprError::ShiftTooBig { .. }));
        assert!(matches!(
            err("1<<ffffffffffffffffffff", 16),
            ExprError::ShiftTooBig { .. }
        ));
        assert!(matches!(err("1<<-1", 10), ExprError::ShiftNegative { .. }));
    }

    /// A stack overflow aborts the process, so depth is capped rather than
    /// trusted. The cap must bite before the stack does.
    #[test]
    fn deep_nesting_is_an_error_not_an_abort() {
        let deep = format!("{}1{}", "(".repeat(500), ")".repeat(500));
        assert_eq!(err(&deep, 10), ExprError::TooDeep);
        let ok = format!("{}1{}", "(".repeat(60), ")".repeat(60));
        assert_eq!(e(&ok, 10), n(1));
    }

    #[test]
    fn malformed_expressions_name_the_problem() {
        assert_eq!(err("(1+2", 10), ExprError::Unclosed);
        assert_eq!(err("1+2)", 10), ExprError::Unopened);
        assert_eq!(err("1+", 10), ExprError::Trailing);
        assert!(matches!(err("*5", 10), ExprError::Leading { op: "*" }));
        assert_eq!(err("1 2+3", 10), ExprError::Juxtaposition);
        assert!(matches!(err("ff$1", 16), ExprError::NotAnOperator { ch: '$' }));
        assert!(matches!(err("1<2", 10), ExprError::LoneAngle { ch: '<' }));
        assert!(matches!(err("1>2", 10), ExprError::LoneAngle { ch: '>' }));
        assert!(matches!(err("_+1", 10), ExprError::EmptyLiteral { .. }));
        // `clean_token` only strips a prefix when digits follow it, so a bare
        // `0x` stays two characters and `x` is simply not a hex digit.
        assert!(matches!(err("0x+1", 16), ExprError::BadDigit { ch: 'x', .. }));
        assert!(matches!(err("1+g", 16), ExprError::BadDigit { ch: 'g', .. }));
    }

    /// What puts a chunk into expression mode at all. Everything false here is
    /// an input that works today and must keep working unchanged.
    #[test]
    fn detection_only_fires_on_a_real_operator() {
        for s in ["1+1", "-5", "(a)", "a|b", "1<<2", "8>>1", "ff&0f", "~0", "a*b"] {
            assert!(has_operator(s), "{s} should be an expression");
        }
        for s in [
            "ff",
            "dead_beef",
            "0xff",
            "ff 10",
            "ff,10",
            "00:11:22",
            "1.5",
            "1<2",
            "1>2",
        ] {
            assert!(!has_operator(s), "{s} must stay a plain token list");
        }
    }

    /// A dash touching digits on both sides is punctuation in one value, not
    /// subtraction — the rule that keeps hex-shaped identifiers an error.
    #[test]
    fn a_tight_dash_is_never_subtraction() {
        // Every character here is a hex digit, so only the dash tells a UUID
        // and a MAC address apart from a chain of subtractions.
        assert_eq!(err("550e8400-e29b-41d4-a716-446655440000", 16), ExprError::TightMinus);
        assert_eq!(err("00-11-22-33-44-55", 16), ExprError::TightMinus);
        assert_eq!(err("4500-4438", 16), ExprError::TightMinus);
        // Still caught when some other operator has already opened the
        // expression, which is the path a real mistake takes.
        assert_eq!(err("4438+1a-5", 16), ExprError::TightMinus);
        // A space on either side is enough to mean it.
        assert_eq!(e("4500 - 4438", 16), n(0xc8));
        assert_eq!(e("4500 -4438", 16), n(0xc8));
        assert_eq!(e("4500- 4438", 16), n(0xc8));
    }

    /// The rule is about a dash *between* two values; a dash that cannot be
    /// subtraction in the first place is unaffected.
    #[test]
    fn unary_minus_is_untouched_by_the_tight_rule() {
        assert_eq!(e("-5", 10), n(-5));
        assert_eq!(e("(-5)+9", 10), n(4));
        assert_eq!(e("9+-5", 10), n(4));
        assert_eq!(e("~-1", 10), n(0));
        // A paren on either side is not a digit, so these stay subtraction.
        assert_eq!(e("(4500)-4438", 16), n(0xc8));
        assert_eq!(e("4500-(4438)", 16), n(0xc8));
    }

    /// `+` deliberately has no such rule: nothing is written with one inside.
    #[test]
    fn only_the_dash_needs_a_space() {
        assert_eq!(e("4438+1a", 16), n(0x4452));
        assert_eq!(e("ff*2", 16), n(510));
        assert_eq!(e("ff&0f", 16), n(0x0f));
    }

}
