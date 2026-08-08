//! Shared domain types for Token-Shrinker.

use serde::{Deserialize, Serialize};
use std::{fmt, num::NonZeroU32, str::FromStr};

/// A caller-provided identifier used to correlate one request across transports.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct RequestId(String);

impl RequestId {
    /// Creates a request identifier after validating its transport-safe shape.
    ///
    /// # Errors
    ///
    /// Returns [`RequestIdError`] for an empty, oversized, or non-ASCII value.
    pub fn new(value: impl Into<String>) -> Result<Self, RequestIdError> {
        let value = value.into();
        if value.is_empty() {
            return Err(RequestIdError::Empty);
        }
        if value.len() > 128 {
            return Err(RequestIdError::TooLong);
        }
        if !value.is_ascii() {
            return Err(RequestIdError::NonAscii);
        }
        Ok(Self(value))
    }

    /// Returns the identifier as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl TryFrom<String> for RequestId {
    type Error = RequestIdError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<RequestId> for String {
    fn from(value: RequestId) -> Self {
        value.0
    }
}

/// Why a request identifier could not be constructed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestIdError {
    /// The identifier was empty.
    Empty,
    /// The identifier exceeded 128 bytes.
    TooLong,
    /// The identifier contained a non-ASCII character.
    NonAscii,
}

impl fmt::Display for RequestIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Empty => "request ID must not be empty",
            Self::TooLong => "request ID must not exceed 128 bytes",
            Self::NonAscii => "request ID must contain only ASCII characters",
        })
    }
}

impl std::error::Error for RequestIdError {}

/// Version of the language-neutral Token-Shrinker protocol.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProtocolVersion {
    /// Breaking protocol generation.
    pub major: u16,
    /// Backward-compatible feature generation.
    pub minor: u16,
}

impl ProtocolVersion {
    /// Protocol implemented by this build.
    pub const CURRENT: Self = Self { major: 1, minor: 0 };

    /// Creates a protocol version.
    #[must_use]
    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }

    /// Returns whether two endpoints can exchange baseline messages.
    #[must_use]
    pub const fn is_compatible_with(self, other: Self) -> bool {
        self.major == other.major
    }
}

impl fmt::Display for ProtocolVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}", self.major, self.minor)
    }
}

/// Work profile selected by the deterministic router.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RouteMode {
    /// Focused lookup or low-risk command.
    Fast,
    /// Normal coding, debugging, and multi-file work.
    #[default]
    Build,
    /// Broad architecture or repository-wide investigation.
    Deep,
}

impl RouteMode {
    /// Stable wire/config spelling for this mode.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fast => "FAST",
            Self::Build => "BUILD",
            Self::Deep => "DEEP",
        }
    }
}

impl fmt::Display for RouteMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for RouteMode {
    type Err = ParseRouteModeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.eq_ignore_ascii_case("fast") {
            Ok(Self::Fast)
        } else if value.eq_ignore_ascii_case("build") {
            Ok(Self::Build)
        } else if value.eq_ignore_ascii_case("deep") {
            Ok(Self::Deep)
        } else {
            Err(ParseRouteModeError)
        }
    }
}

/// Returned when a route mode is not `FAST`, `BUILD`, or `DEEP`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseRouteModeError;

impl fmt::Display for ParseRouteModeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("route mode must be FAST, BUILD, or DEEP")
    }
}

impl std::error::Error for ParseRouteModeError {}

/// Positive maximum number of context tokens available to a request.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct TokenBudget(NonZeroU32);

impl TokenBudget {
    /// Creates a positive token budget.
    #[must_use]
    pub const fn new(tokens: NonZeroU32) -> Self {
        Self(tokens)
    }

    /// Returns the configured token count.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0.get()
    }

    /// Creates a budget from a primitive integer, returning `None` for zero.
    #[must_use]
    pub const fn from_u32(tokens: u32) -> Option<Self> {
        match NonZeroU32::new(tokens) {
            Some(tokens) => Some(Self(tokens)),
            None => None,
        }
    }
}

impl From<NonZeroU32> for TokenBudget {
    fn from(value: NonZeroU32) -> Self {
        Self::new(value)
    }
}

/// Stable explanation emitted by the version-one router.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteReason {
    /// The caller supplied a valid explicit mode.
    ExplicitOverride,
    /// A fixed configured mode was selected while the request had no override.
    ConfiguredMode,
    /// The request is a small, focused lookup.
    FocusedLookup,
    /// The request asks for implementation or debugging work.
    BuildOperation,
    /// The request spans broad architecture or repository scope.
    BroadScope,
    /// The caller replaced the configured context budget.
    BudgetOverride,
    /// No stronger rule matched, so the safe default was used.
    AmbiguousDefault,
}

impl RouteReason {
    /// Stable machine-readable reason code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::ExplicitOverride => "explicit_override",
            Self::ConfiguredMode => "configured_mode",
            Self::FocusedLookup => "focused_lookup",
            Self::BuildOperation => "build_operation",
            Self::BroadScope => "broad_scope",
            Self::BudgetOverride => "budget_override",
            Self::AmbiguousDefault => "ambiguous_default",
        }
    }
}

/// Explainable result of deterministic route selection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RouteDecision {
    /// Selected work profile.
    pub mode: RouteMode,
    /// Effective context budget after configuration overrides.
    pub budget: TokenBudget,
    /// Ordered, stable reasons supporting the decision.
    pub reasons: Vec<RouteReason>,
}

/// Normalized operation signal supplied to the router.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteOperation {
    /// Read a known fact or location.
    Lookup,
    /// Run a focused, low-risk command.
    Command,
    /// Change implementation files.
    Edit,
    /// Diagnose incorrect behavior.
    Debug,
    /// Design or assess system architecture.
    Architecture,
    /// Perform a cross-cutting investigation.
    Investigation,
}

/// Normalized breadth of repository context requested by the caller.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteScope {
    /// One explicitly named file or symbol.
    Named,
    /// More than one file in a bounded area.
    MultiFile,
    /// Repository-wide or otherwise unbounded scope.
    Repository,
}

/// Inputs used by the deterministic router after transport normalization.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RouteRequest {
    /// Valid explicit mode, which always wins when present.
    #[serde(default)]
    pub explicit_mode: Option<RouteMode>,
    /// Requested operation classes.
    #[serde(default)]
    pub operations: Vec<RouteOperation>,
    /// Known repository breadth.
    #[serde(default)]
    pub scope: Option<RouteScope>,
    /// Optional per-request context budget.
    #[serde(default)]
    pub budget_override: Option<TokenBudget>,
}

/// Identity of an exact tokenizer or conservative estimator.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct CounterId(String);

impl CounterId {
    /// Creates a stable counter identifier.
    ///
    /// # Errors
    ///
    /// Returns [`CounterIdError`] when the value is empty, exceeds 64 bytes, or
    /// contains characters outside ASCII letters, digits, `.`, `-`, and `_`.
    pub fn new(value: impl Into<String>) -> Result<Self, CounterIdError> {
        let value = value.into();
        if value.is_empty() {
            return Err(CounterIdError::Empty);
        }
        if value.len() > 64 {
            return Err(CounterIdError::TooLong);
        }
        if !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        {
            return Err(CounterIdError::InvalidCharacter);
        }
        Ok(Self(value))
    }

    /// Returns the stable identifier text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for CounterId {
    type Error = CounterIdError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<CounterId> for String {
    fn from(value: CounterId) -> Self {
        value.0
    }
}

/// Why a counter identifier could not be constructed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CounterIdError {
    /// The identifier was empty.
    Empty,
    /// The identifier exceeded 64 bytes.
    TooLong,
    /// The identifier contained a character outside the stable wire alphabet.
    InvalidCharacter,
}

impl fmt::Display for CounterIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Empty => "counter ID must not be empty",
            Self::TooLong => "counter ID must not exceed 64 bytes",
            Self::InvalidCharacter => {
                "counter ID may contain only ASCII letters, digits, '.', '-', and '_'"
            }
        })
    }
}

impl std::error::Error for CounterIdError {}

/// Whether a token count came from a compatible tokenizer or a fallback estimate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CountPrecision {
    /// Count produced by an identified compatible tokenizer.
    Exact,
    /// Count produced by a documented fallback formula.
    Estimated,
}

/// Labeled token count that never hides whether it is estimated.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TokenCount {
    tokens: u64,
    precision: CountPrecision,
    counter_id: CounterId,
}

impl TokenCount {
    /// Creates a labeled token count.
    #[must_use]
    pub const fn new(tokens: u64, precision: CountPrecision, counter_id: CounterId) -> Self {
        Self {
            tokens,
            precision,
            counter_id,
        }
    }

    /// Returns the number of tokens or conservative token units.
    #[must_use]
    pub const fn tokens(&self) -> u64 {
        self.tokens
    }

    /// Returns whether the count is exact or estimated.
    #[must_use]
    pub const fn precision(&self) -> CountPrecision {
        self.precision
    }

    /// Returns the tokenizer or estimator identity.
    #[must_use]
    pub const fn counter_id(&self) -> &CounterId {
        &self.counter_id
    }
}

impl RouteDecision {
    /// Creates a route decision with its primary reason.
    #[must_use]
    pub fn new(mode: RouteMode, budget: TokenBudget, primary_reason: RouteReason) -> Self {
        Self {
            mode,
            budget,
            reasons: vec![primary_reason],
        }
    }

    /// Appends another reason without duplicating an existing code.
    pub fn add_reason(&mut self, reason: RouteReason) {
        if !self.reasons.contains(&reason) {
            self.reasons.push(reason);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_id_rejects_invalid_values() {
        assert_eq!(RequestId::new(""), Err(RequestIdError::Empty));
        assert_eq!(
            RequestId::new("x".repeat(129)),
            Err(RequestIdError::TooLong)
        );
        assert_eq!(RequestId::new("café"), Err(RequestIdError::NonAscii));
    }

    #[test]
    fn protocol_compatibility_requires_matching_major_version() {
        assert!(ProtocolVersion::CURRENT.is_compatible_with(ProtocolVersion::new(1, 9)));
        assert!(!ProtocolVersion::CURRENT.is_compatible_with(ProtocolVersion::new(2, 0)));
    }

    #[test]
    fn route_mode_parse_is_case_insensitive() {
        for (input, expected) in [
            ("fast", RouteMode::Fast),
            ("BUILD", RouteMode::Build),
            ("Deep", RouteMode::Deep),
        ] {
            assert_eq!(input.parse(), Ok(expected));
            assert_eq!(expected.to_string(), expected.as_str());
        }
        assert_eq!("wide".parse::<RouteMode>(), Err(ParseRouteModeError));
    }

    #[test]
    fn route_decision_keeps_unique_ordered_reasons() {
        let budget = TokenBudget::new(NonZeroU32::new(16_000).expect("nonzero test value"));
        let mut decision =
            RouteDecision::new(RouteMode::Build, budget, RouteReason::BuildOperation);

        decision.add_reason(RouteReason::BuildOperation);
        decision.add_reason(RouteReason::BroadScope);

        assert_eq!(decision.budget.get(), 16_000);
        assert_eq!(
            decision.reasons,
            vec![RouteReason::BuildOperation, RouteReason::BroadScope]
        );
    }

    #[test]
    fn counter_id_rejects_unstable_wire_values() {
        assert_eq!(CounterId::new(""), Err(CounterIdError::Empty));
        assert_eq!(
            CounterId::new("counter with spaces"),
            Err(CounterIdError::InvalidCharacter)
        );
        assert_eq!(CounterId::new("x".repeat(65)), Err(CounterIdError::TooLong));
    }
}
