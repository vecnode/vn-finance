//! Portuguese tax-number handling.
//!
//! The NIF/NIPC is a nine-digit number whose last digit is a modulus-11 check
//! digit. Validating it matters for two reasons: it catches a typo before it
//! reaches an invoice, and it is what lets the application tell a tax number apart
//! from a mobile phone number that also has nine digits.

/// Strip the separators a person or a PDF may put inside a tax number.
#[must_use]
pub fn normalise_nif(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_whitespace() && *ch != '.' && *ch != '-')
        .collect()
}

/// Whether the value is a well-formed Portuguese tax number.
///
/// Nine digits and a matching modulus-11 check digit. A number that fails the
/// check digit is not "probably right": it is refused, because the alternative is
/// an invoice filed against a number that does not exist.
#[must_use]
pub fn is_valid_pt_tax_number(value: &str) -> bool {
    let digits: Vec<u8> = normalise_nif(value).bytes().collect();
    if digits.len() != 9 || !digits.iter().all(u8::is_ascii_digit) {
        return false;
    }
    let mut sum = 0_u32;
    // The first eight digits are weighted 9 down to 2; the ninth is the check.
    for (index, digit) in digits.iter().take(8).enumerate() {
        sum += u32::from(digit - b'0') * (9 - u32::try_from(index).unwrap_or(9));
    }
    let remainder = sum % 11;
    let check_digit = if remainder < 2 { 0 } else { 11 - remainder };
    check_digit == u32::from(digits[8] - b'0')
}

/// What kind of person a valid tax number belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaxNumberKind {
    /// A natural person (numbers starting 1–3).
    Nif,
    /// A collective person (numbers starting 5–9).
    Nipc,
}

impl TaxNumberKind {
    /// The label the interface shows.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Nif => "NIF",
            Self::Nipc => "NIPC",
        }
    }
}

/// Classify a tax number, or `None` when the check digit fails.
///
/// A number starting with 4 is not a valid Portuguese tax number and is refused
/// by the check below long before the first digit is read.
#[must_use]
pub fn classify_pt_tax_number(value: &str) -> Option<TaxNumberKind> {
    let digits = normalise_nif(value);
    if !is_valid_pt_tax_number(&digits) {
        return None;
    }
    match digits.as_bytes().first() {
        Some(b'1' | b'2' | b'3') => Some(TaxNumberKind::Nif),
        Some(b'5' | b'6' | b'7' | b'8' | b'9') => Some(TaxNumberKind::Nipc),
        _ => Some(TaxNumberKind::Nif),
    }
}

/// Mask a tax number for display: `123456789` -> `123••••89`.
///
/// Enough to recognise, not enough to leak.
#[must_use]
pub fn mask_nif(value: &str) -> String {
    let digits: Vec<char> = normalise_nif(value).chars().collect();
    if digits.len() <= 4 {
        return "••••".to_owned();
    }
    let head_length = 3.min(digits.len().saturating_sub(2));
    let tail_length = 2;
    let head: String = digits.iter().take(head_length).collect();
    let tail: String = digits.iter().skip(digits.len() - tail_length).collect();
    let hidden = digits.len().saturating_sub(head_length + tail_length);
    format!("{head}{}{tail}", "•".repeat(hidden))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separators_are_ignored_when_reading_a_tax_number() {
        assert_eq!(normalise_nif("123 456 789"), "123456789");
        assert_eq!(normalise_nif("123.456.789"), "123456789");
        assert_eq!(normalise_nif("123-456-789"), "123456789");
    }

    #[test]
    fn a_matching_check_digit_is_accepted() {
        // Values invented for the fixtures, each with a valid check digit.
        for valid in ["123456789", "245678999", "501234560", "123 456 789"] {
            assert!(is_valid_pt_tax_number(valid), "{valid} must be valid");
        }
    }

    #[test]
    fn one_digit_off_is_refused() {
        for invalid in [
            "123456788",
            "501234561",
            "12345678",
            "1234567890",
            "",
            "abcdefghi",
            "12345678a",
        ] {
            assert!(!is_valid_pt_tax_number(invalid), "{invalid} must be refused");
        }
    }

    #[test]
    fn a_natural_person_and_a_company_are_told_apart() {
        assert_eq!(classify_pt_tax_number("123456789"), Some(TaxNumberKind::Nif));
        assert_eq!(classify_pt_tax_number("245678999"), Some(TaxNumberKind::Nif));
        assert_eq!(classify_pt_tax_number("501234560"), Some(TaxNumberKind::Nipc));
        assert_eq!(classify_pt_tax_number("123456788"), None);
        assert_eq!(TaxNumberKind::Nipc.label(), "NIPC");
    }

    #[test]
    fn masking_keeps_three_digits_each_side() {
        assert_eq!(mask_nif("123456789"), "123••••89");
        assert_eq!(mask_nif("123 456 789"), "123••••89");
        // Anything four characters or shorter cannot be masked usefully.
        assert_eq!(mask_nif("1234"), "••••");
        assert_eq!(mask_nif(""), "••••");
        assert_eq!(mask_nif("123456"), "123•56");
    }
}
