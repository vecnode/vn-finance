//! Where a client is, and why it matters.
//!
//! The art. 53.º CIVA ceiling is measured in **national territory** only, while
//! the other two scopes change the VAT treatment, the VIES registration and the
//! declaração recapitulativa. Keeping this in one module means the rule engine, the
//! invoice defaults and the alert engine cannot disagree about what "foreign"
//! means.

/// The three scopes a client's country can fall into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnoverScope {
    /// Portugal.
    National,
    /// Another European Union member state.
    Eu,
    /// Anywhere else.
    NonEu,
}

/// The 27 member states, as ISO 3166-1 alpha-2 codes.
const EU_COUNTRIES: [&str; 27] = [
    "AT", "BE", "BG", "HR", "CY", "CZ", "DK", "EE", "FI", "FR", "DE", "GR", "HU", "IE", "IT", "LV", "LT",
    "LU", "MT", "NL", "PL", "PT", "RO", "SK", "SI", "ES", "SE",
];

/// Classify a client's country code.
///
/// Case and surrounding whitespace do not matter: a person typing `pt` and a PDF
/// printing `PORTUGAL` both have to land where they belong, and an unrecognised
/// code is outside the Union rather than an error — the ledger records the text it
/// was given and the engine decides the scope from it.
#[must_use]
pub fn scope_of(country: &str) -> TurnoverScope {
    let code = country.trim().to_uppercase();
    if code == "PT" {
        return TurnoverScope::National;
    }
    if EU_COUNTRIES.contains(&code.as_str()) {
        TurnoverScope::Eu
    } else {
        TurnoverScope::NonEu
    }
}

/// Whether the country is another European Union member state.
#[must_use]
pub fn is_eu_country(country: &str) -> bool {
    scope_of(country) == TurnoverScope::Eu
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portugal_is_national_and_the_union_is_european() {
        assert_eq!(scope_of("PT"), TurnoverScope::National);
        assert_eq!(scope_of("pt"), TurnoverScope::National);
        assert_eq!(scope_of("  Pt  "), TurnoverScope::National);
        assert_eq!(scope_of("ES"), TurnoverScope::Eu);
        assert_eq!(scope_of("de"), TurnoverScope::Eu);
        assert!(is_eu_country("FR"));
    }

    #[test]
    fn everything_else_is_outside_the_union() {
        for country in ["US", "NO", "CH", "GB", "AO", "BR", "XX", ""] {
            assert_eq!(scope_of(country), TurnoverScope::NonEu, "{country:?}");
        }
        assert!(!is_eu_country("US"));
    }

    #[test]
    fn the_member_state_list_is_exactly_twenty_seven() {
        // A list that silently loses a member changes an exemption ceiling.
        assert_eq!(EU_COUNTRIES.len(), 27);
        assert!(EU_COUNTRIES.contains(&"HR"), "Croatia joined in 2013");
        assert!(!EU_COUNTRIES.contains(&"GB"), "the United Kingdom left in 2020");
    }
}
