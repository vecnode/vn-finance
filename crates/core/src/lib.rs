//! vn-finance's deterministic domain.
//!
//! Nothing in this crate touches the filesystem, the clock, the environment or
//! the network: every function is a value in, a value out. The date the
//! application believes it is today arrives as an [`IsoDate`] parameter, which is
//! what makes the obligation calendar and the estimates reproducible in a test —
//! and what lets the browser-era suite and this one agree, figure for figure.
//!
//! Three commitments shape the whole crate:
//!
//! - **The law is data.** Rates, coefficients, deadlines and their citations live
//!   in a versioned rule pack, never in an `if`. When the law changes, a data file
//!   changes and every number keeps its provenance.
//! - **A number is computed or refused.** A constant the pack leaves empty makes
//!   the calculation that needs it refuse, never fall back to a plausible value.
//! - **A citation is checked, not asserted.** [`sources`] compares the passages the
//!   pack quotes with the copies of the documents it kept, and the document's hash
//!   with the one the pack recorded. Evidence that cannot be checked is said to be
//!   unchecked, never silently trusted.
//! - **Money is integer cents and rates are integer basis points** ([`money`]);
//!   dates are ISO strings ([`dates`]). No `f64` means "euros" anywhere.

#![warn(missing_docs)]
#![forbid(unsafe_code)]
#![allow(unreachable_pub)]

pub mod calendar;
pub mod conditions;
pub mod countries;
pub mod dates;
pub mod estimate;
pub mod flags;
pub mod money;
pub mod nif;
pub mod pdf;
pub mod profile;
pub mod receipt;
pub mod rollover;
pub mod rules;
pub mod sources;
pub mod types;
