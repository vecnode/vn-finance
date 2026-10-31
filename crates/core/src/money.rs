//! Monetary and rate arithmetic.
//!
//! Two rules govern every number in this crate:
//!
//! 1. **Money is an integer number of euro cents** ([`Cents`]). There is no
//!    `f64` that means "euros" anywhere in the domain, and no operation may
//!    introduce one: floating-point money is how an accountant ends up with a
//!    one-cent difference nobody can explain.
//! 2. **Rates are integer basis points** ([`BasisPoints`]): 1 % = 100,
//!    21,4 % = 2140, a coefficient of 0,75 = 7500.
//!
//! The formatting functions reproduce `Intl.NumberFormat('pt-PT')` byte for byte,
//! because the same strings reach the panel, the audit log and the tests. Two
//! details are not guessable from "Portuguese convention" and are therefore
//! pinned by the golden fixtures in `tests/golden/`:
//!
//! - pt-PT has `minimumGroupingDigits = 2`, so a **four-digit integer is never
//!   grouped**: `1000,00 €`, but `10 000,00 €`.
//! - the group separator, and the space before the euro sign, are U+202F (narrow
//!   no-break space), not U+0020 and not U+00A0.

use core::fmt;

/// Integer euro cents. `4_860_000` is 48 600,00 EUR.
pub type Cents = i64;

/// Integer basis points. 1 % = 100; 21,4 % = 2140; coefficient 0,75 = 7500.
pub type BasisPoints = i64;

/// Why a monetary value could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoneyError {
    /// The text held no amount at all.
    EmptyAmount,
    /// The text held something, but not an amount in the accepted forms.
    NotAnAmount {
        /// The text as it arrived, so the message can quote it back.
        input: String,
    },
    /// An amount in the accepted forms whose cents do not fit [`Cents`].
    ///
    /// The reference implementation would round this through a `f64` and return
    /// a figure that is no longer an integer number of cents; refusing is the
    /// only honest answer, and no invoice reaches this range.
    OutOfRange {
        /// The text as it arrived.
        input: String,
    },
    /// A split into zero parts, which has no meaning.
    NoParts,
}

impl fmt::Display for MoneyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyAmount => f.write_str("empty amount"),
            Self::NotAnAmount { input } => write!(f, "cannot parse \"{input}\" as a EUR amount"),
            Self::OutOfRange { input } => {
                write!(f, "\"{input}\" is too large to be an amount in cents")
            }
            Self::NoParts => f.write_str("parts must be a positive integer"),
        }
    }
}

impl std::error::Error for MoneyError {}

/// Round a fraction of a cent the way an accountant expects: halves away from zero.
///
/// The whole crate rounds through this one rule. `2_906,12` over three instalments
/// is `968,71 + 968,71 + 968,70`; nothing anywhere truncates a half.
fn round_scaled(numerator: i128, denominator: i128) -> i128 {
    let half = denominator / 2;
    (numerator + numerator.signum() * half) / denominator
}

/// Apply a basis-point rate to an amount, rounded to whole cents.
///
/// Computed in `i128` and rounded half away from zero, which is bit-identical to
/// the reference implementation for every amount the application can hold: its
/// `(amount * bp) / 10_000` is exact while `|amount × bp| < 2^53`, and this
/// function stays exact far beyond that. The bound that matters is documented on
/// [`Cents`]: an amount is validated when it enters the vault, and no legitimate
/// figure approaches the point where `i128` would not fit back into `i64`.
#[must_use]
pub fn apply_bp(amount: Cents, bp: BasisPoints) -> Cents {
    let product = i128::from(amount) * i128::from(bp);
    let rounded = round_scaled(product, 10_000);
    debug_assert!(
        rounded >= i128::from(Cents::MIN) && rounded <= i128::from(Cents::MAX),
        "apply_bp overflowed: {amount} at {bp} bp"
    );
    rounded as Cents
}

/// Split an amount into `parts` as evenly as legally possible.
///
/// The parts always sum **exactly** to `total`. The remainder cents go to the
/// first parts, which is what an instalment plan does: 2 906,12 EUR in three
/// payments is 968,71 + 968,71 + 968,70, never three times 968,71 with a cent
/// lost.
///
/// # Errors
///
/// [`MoneyError::NoParts`] when `parts` is zero.
pub fn split_evenly(total: Cents, parts: u32) -> Result<Vec<Cents>, MoneyError> {
    if parts == 0 {
        return Err(MoneyError::NoParts);
    }
    let count = i64::from(parts);
    let base = total / count;
    let remainder = total - base * count;
    let step = remainder.signum();
    let magnitude = u32::try_from(remainder.unsigned_abs()).unwrap_or(u32::MAX);
    Ok((0..parts)
        .map(|index| if index < magnitude { base + step } else { base })
        .collect())
}

/// Format cents as a pt-PT currency string, e.g. `4_860_000` -> `48 600,00 €`.
#[must_use]
pub fn format_eur(cents: Cents) -> String {
    let magnitude = cents.unsigned_abs();
    let euros = magnitude / 100;
    let rest = magnitude % 100;
    let mut text = String::with_capacity(16);
    if cents < 0 {
        text.push('-');
    }
    text.push_str(&group_thousands(euros));
    text.push(',');
    text.push_str(&format!("{rest:02}"));
    text.push('\u{202f}');
    text.push('€');
    text
}

/// Insert the pt-PT group separator, honouring `minimumGroupingDigits = 2`.
fn group_thousands(value: u64) -> String {
    let digits = value.to_string();
    // A four-digit integer is not grouped in pt-PT: `1000,00 €`, not `1 000,00 €`.
    if digits.len() < 5 {
        return digits;
    }
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push('\u{202f}');
        }
        out.push(ch);
    }
    out
}

/// Parse a human-entered EUR amount into cents.
///
/// Accepts `"1 234,56"`, `"1.234,56"`, `"1234.56"`, `"1,234.56"` and
/// `"1 234,56 €"`. Ambiguity is resolved in favour of the Portuguese convention:
/// comma is the decimal separator, dot and space are thousands separators.
///
/// # Errors
///
/// [`MoneyError::EmptyAmount`] for empty or whitespace-only input,
/// [`MoneyError::NotAnAmount`] for text that is not an amount, and
/// [`MoneyError::OutOfRange`] for an amount too large to be cents.
pub fn parse_eur_to_cents(input: &str) -> Result<Cents, MoneyError> {
    let cleaned: String = input
        .chars()
        .filter(|ch| *ch != '€' && !ch.is_whitespace() && *ch != '\u{a0}' && *ch != '\u{202f}')
        .collect();
    if cleaned.is_empty() {
        return Err(MoneyError::EmptyAmount);
    }

    let normalised = normalise_separators(&cleaned);
    let (negative, unsigned) = match normalised.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, normalised.as_str()),
    };
    let (whole, fraction) = match unsigned.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (unsigned, None),
    };
    let digits_only = |text: &str| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    let fraction = match fraction {
        None => "",
        Some(fraction) if fraction.is_empty() || fraction.len() > 2 || !digits_only(fraction) => {
            return Err(MoneyError::NotAnAmount {
                input: input.to_owned(),
            });
        }
        Some(fraction) => fraction,
    };
    if !digits_only(whole) {
        return Err(MoneyError::NotAnAmount {
            input: input.to_owned(),
        });
    }

    let out_of_range = || MoneyError::OutOfRange {
        input: input.to_owned(),
    };
    let euros: i64 = whole.parse().map_err(|_| out_of_range())?;
    let mut cents = euros.checked_mul(100).ok_or_else(out_of_range)?;
    for (index, byte) in fraction.bytes().enumerate() {
        let digit = i64::from(byte - b'0');
        let place = if index == 0 { 10 } else { 1 };
        cents = cents.checked_add(digit * place).ok_or_else(out_of_range)?;
    }
    Ok(if negative { -cents } else { cents })
}

/// Resolve the comma/dot ambiguity the Portuguese way.
///
/// Mirrors the reference implementation: when both separators appear, the one
/// that comes last is the decimal separator; a lone comma is decimal; a lone dot
/// is left for the strict digit check to judge.
fn normalise_separators(cleaned: &str) -> String {
    match (cleaned.rfind(','), cleaned.rfind('.')) {
        (Some(comma), Some(dot)) if comma > dot => cleaned.replace('.', "").replace(',', "."),
        (Some(_), Some(_)) => cleaned.replace(',', ""),
        (Some(_), None) => cleaned.replacen(',', ".", 1),
        _ => cleaned.to_owned(),
    }
}

/// Percentage helper for display only: `2140` -> `21,4%`.
///
/// Reproduces the reference implementation's `toFixed(2)` with one trailing zero
/// removed, which for basis points (a value with at most two decimals) is exactly
/// the integer arithmetic below.
#[must_use]
pub fn format_bp_as_percent(bp: BasisPoints) -> String {
    let magnitude = bp.unsigned_abs();
    let whole = magnitude / 100;
    let rest = u32::try_from(magnitude % 100).unwrap_or(0);
    let sign = if bp < 0 { "-" } else { "" };
    if rest == 0 {
        return format!("{sign}{whole}%");
    }
    let mut fraction = format!("{rest:02}");
    if fraction.ends_with('0') {
        fraction.pop();
    }
    format!("{sign}{whole},{fraction}%")
}

/// Coefficient helper for display only: `7500` -> `0,75`.
#[must_use]
pub fn format_bp_as_coefficient(bp: BasisPoints) -> String {
    let magnitude = bp.unsigned_abs();
    let whole = magnitude / 10_000;
    let rest = u32::try_from(magnitude % 10_000).unwrap_or(0);
    let sign = if bp < 0 { "-" } else { "" };
    let mut fraction = format!("{rest:04}");
    while fraction.ends_with('0') {
        fraction.pop();
    }
    if fraction.is_empty() {
        return format!("{sign}{whole}");
    }
    format!("{sign}{whole},{fraction}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applying_basis_points_rounds_halves_away_from_zero() {
        // 48 600,00 EUR at 23 % = 11 178,00 EUR
        assert_eq!(apply_bp(4_860_000, 2300), 1_117_800);
        // 19 400,00 EUR at 70 % = 13 580,00 EUR
        assert_eq!(apply_bp(1_940_000, 7000), 1_358_000);
        // 13 580,00 EUR at 21,4 % = 2 906,12 EUR
        assert_eq!(apply_bp(1_358_000, 2140), 290_612);
        // Half a cent rounds away from zero, the way an accountant expects.
        assert_eq!(apply_bp(1, 5000), 1);
        assert_eq!(apply_bp(-1, 5000), -1);
        // Below half a cent rounds down, on both sides.
        assert_eq!(apply_bp(1, 4999), 0);
        assert_eq!(apply_bp(-1, 4999), 0);
    }

    #[test]
    fn applying_basis_points_stays_exact_at_the_edge_of_the_float_path() {
        // `|amount × bp| < 2^53` is where the reference implementation's f64
        // division is provably exact; this asserts the integer path agrees there
        // and keeps agreeing above it.
        for (amount, bp) in [
            (1_000_000_000_000, 2300),
            (9_000_000_000_000, 10_000),
            (123_456_789, 2140),
        ] {
            let product = amount * bp;
            let reference = (product as f64) / 10_000.0;
            let expected = if reference >= 0.0 {
                (reference + 0.5).floor() as i64
            } else {
                (reference - 0.5).ceil() as i64
            };
            assert_eq!(apply_bp(amount, bp), expected, "apply_bp({amount}, {bp})");
        }
    }

    #[test]
    fn splitting_never_loses_a_cent() {
        let parts = split_evenly(290_612, 3).unwrap_or_default();
        assert_eq!(parts, vec![96_871, 96_871, 96_870]);
        assert_eq!(parts.iter().sum::<Cents>(), 290_612);
    }

    #[test]
    fn splitting_handles_indivisible_and_negative_amounts_exactly() {
        for (total, parts) in [(100, 3), (7, 7), (0, 4), (-290_612, 3), (1, 12)] {
            let split = split_evenly(total, parts).unwrap_or_default();
            assert_eq!(split.len(), usize::try_from(parts).unwrap_or(0));
            assert_eq!(
                split.iter().sum::<Cents>(),
                total,
                "split_evenly({total}, {parts}) must sum back to the total"
            );
            let spread = split.iter().max().copied().unwrap_or(0) - split.iter().min().copied().unwrap_or(0);
            assert!(spread <= 1, "instalments must differ by at most one cent");
        }
        assert_eq!(split_evenly(100, 0), Err(MoneyError::NoParts));
    }

    #[test]
    fn parsing_reads_portuguese_and_international_formats() {
        for (input, cents) in [
            ("1 234,56", 123_456),
            ("1.234,56", 123_456),
            ("1234.56", 123_456),
            ("1,234.56", 123_456),
            ("1 234,56 €", 123_456),
            ("0,01", 1),
            ("-45,50", -4_550),
            ("12", 1_200),
            ("12,5", 1_250),
        ] {
            assert_eq!(parse_eur_to_cents(input), Ok(cents), "parsing {input:?}");
        }
    }

    #[test]
    fn parsing_refuses_what_is_not_an_amount() {
        assert_eq!(parse_eur_to_cents(""), Err(MoneyError::EmptyAmount));
        assert_eq!(parse_eur_to_cents("   "), Err(MoneyError::EmptyAmount));
        for input in ["abc", "1,2,3", "12,345", ".5", "1.", "--1", "1e3"] {
            assert!(
                matches!(parse_eur_to_cents(input), Err(MoneyError::NotAnAmount { .. })),
                "{input:?} must be refused"
            );
        }
        // An amount too large for cents is refused rather than rounded through a
        // float into something that is no longer an integer number of cents.
        assert!(matches!(
            parse_eur_to_cents("99999999999999999999"),
            Err(MoneyError::OutOfRange { .. })
        ));
    }

    #[test]
    fn parsing_round_trips_every_amount_the_panel_can_hold() {
        // 0,00 to 10 000,00 EUR in cents, plus two negative amounts: the text
        // form the panel writes must read back as the same number, never a cent
        // out. A million cases cost a second and cover every two-digit fraction.
        for cents in (0..=1_000_000_i64).chain([-100_i64, -45_500]) {
            let magnitude = cents.unsigned_abs();
            let sign = if cents < 0 { "-" } else { "" };
            let text = format!("{sign}{},{:02}", magnitude / 100, magnitude % 100);
            assert_eq!(parse_eur_to_cents(&text), Ok(cents), "round trip of {text:?}");
        }
    }

    #[test]
    fn formatting_is_the_portuguese_currency_string() {
        assert_eq!(format_eur(4_860_000), "48\u{202f}600,00\u{202f}€");
        assert_eq!(format_eur(123_456_789), "1\u{202f}234\u{202f}567,89\u{202f}€");
        assert_eq!(format_eur(0), "0,00\u{202f}€");
        assert_eq!(format_eur(-4_550), "-45,50\u{202f}€");
        assert_eq!(format_eur(1), "0,01\u{202f}€");
    }

    #[test]
    fn formatting_does_not_group_a_four_digit_integer() {
        // `minimumGroupingDigits = 2`: 1000 is written without a separator, and
        // 10 000 with one. Getting this wrong breaks every fixture and every
        // screenshot, so it is pinned separately from the general case above.
        assert_eq!(format_eur(100_000), "1000,00\u{202f}€");
        assert_eq!(format_eur(1_000_000), "10\u{202f}000,00\u{202f}€");
        assert_eq!(format_eur(99_999_999), "999\u{202f}999,99\u{202f}€");
    }

    #[test]
    fn rates_and_coefficients_read_the_portuguese_way() {
        assert_eq!(format_bp_as_percent(2140), "21,4%");
        assert_eq!(format_bp_as_percent(2300), "23%");
        assert_eq!(format_bp_as_percent(7000), "70%");
        assert_eq!(format_bp_as_percent(2105), "21,05%");
        assert_eq!(format_bp_as_percent(1), "0,01%");
        assert_eq!(format_bp_as_percent(-2500), "-25%");
        assert_eq!(format_bp_as_coefficient(7500), "0,75");
        assert_eq!(format_bp_as_coefficient(3500), "0,35");
        assert_eq!(format_bp_as_coefficient(10_000), "1");
        assert_eq!(format_bp_as_coefficient(1560), "0,156");
    }
}
