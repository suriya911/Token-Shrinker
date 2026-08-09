//! Context candidates, ranking, budgets, and provenance.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fmt};
use token_shrinker_types::{CountPrecision, CounterId, TokenBudget, TokenCount};

/// Stable identity of the baseline UTF-8 byte upper-bound estimator.
pub const CONSERVATIVE_ESTIMATOR_ID: &str = "byte_upper_bound_v1";

/// Counts text for context budgeting while disclosing precision and identity.
pub trait TokenCounter {
    /// Stable tokenizer or estimator identity.
    fn id(&self) -> CounterId;

    /// Whether this counter is exact for a known tokenizer.
    fn precision(&self) -> CountPrecision;

    /// Counts one text segment deterministically.
    fn count(&self, text: &str) -> TokenCount;
}

/// Provider-neutral text and path hints for context discovery.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ContextQuery {
    /// Case-insensitive terms to find in candidate content.
    pub terms: Vec<String>,
    /// Case-insensitive fragments to find in source paths or URIs.
    pub path_hints: Vec<String>,
}

/// Common discovery and retrieval contract for built-in and optional context providers.
pub trait ContextProvider: Send + Sync {
    /// Provider-specific error type.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Discovers normalized candidates for the supplied query.
    ///
    /// # Errors
    ///
    /// Returns the provider error when discovery cannot complete.
    fn candidates(&self, query: &ContextQuery) -> Result<Vec<ContextCandidate>, Self::Error>;

    /// Retrieves one normalized source using its opaque source identifier.
    ///
    /// # Errors
    ///
    /// Returns the provider error when the identifier is invalid, unavailable, or disallowed.
    fn fetch(&self, source_id: &SourceId) -> Result<ContextCandidate, Self::Error>;
}

/// Offline fallback that assigns at most one conservative unit per UTF-8 byte.
#[derive(Clone, Copy, Debug, Default)]
pub struct ConservativeEstimator;

impl TokenCounter for ConservativeEstimator {
    fn id(&self) -> CounterId {
        CounterId::new(CONSERVATIVE_ESTIMATOR_ID).expect("built-in counter ID is valid")
    }

    fn precision(&self) -> CountPrecision {
        CountPrecision::Estimated
    }

    fn count(&self, text: &str) -> TokenCount {
        TokenCount::new(text.len() as u64, self.precision(), self.id())
    }
}

/// Result of deterministic in-order candidate selection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BudgetSelection {
    /// Indices of candidates that fit, in input order.
    pub selected: Vec<usize>,
    /// Indices omitted because they would exceed the budget.
    pub omitted: Vec<usize>,
    /// Labeled total for selected candidate content.
    pub used: TokenCount,
}

/// Selects candidates in order without allowing their counted content to exceed the budget.
#[must_use]
pub fn select_within_budget<C: TokenCounter>(
    candidates: &[&str],
    budget: TokenBudget,
    counter: &C,
) -> BudgetSelection {
    let limit = u64::from(budget.get());
    let mut used = 0_u64;
    let mut selected = Vec::new();
    let mut omitted = Vec::new();

    for (index, candidate) in candidates.iter().enumerate() {
        let candidate_tokens = counter.count(candidate).tokens();
        if used.saturating_add(candidate_tokens) <= limit {
            used += candidate_tokens;
            selected.push(index);
        } else {
            omitted.push(index);
        }
    }

    BudgetSelection {
        selected,
        omitted,
        used: TokenCount::new(used, counter.precision(), counter.id()),
    }
}

/// Stable opaque handle used to retrieve a source again.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct SourceId(String);

impl SourceId {
    /// Creates a source handle safe for language-neutral transports.
    ///
    /// # Errors
    ///
    /// Returns [`SourceIdError`] for an empty, oversized, non-ASCII, or control-containing value.
    pub fn new(value: impl Into<String>) -> Result<Self, SourceIdError> {
        let value = value.into();
        if value.is_empty() {
            return Err(SourceIdError::Empty);
        }
        if value.len() > 1_024 {
            return Err(SourceIdError::TooLong);
        }
        if !value.is_ascii() || value.bytes().any(|byte| byte.is_ascii_control()) {
            return Err(SourceIdError::UnsafeCharacter);
        }
        Ok(Self(value))
    }

    /// Returns the opaque handle text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for SourceId {
    type Error = SourceIdError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<SourceId> for String {
    fn from(value: SourceId) -> Self {
        value.0
    }
}

/// Why a source handle could not be constructed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceIdError {
    /// The handle was empty.
    Empty,
    /// The handle exceeded 1,024 bytes.
    TooLong,
    /// The handle contained non-ASCII or control characters.
    UnsafeCharacter,
}

impl fmt::Display for SourceIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Empty => "source ID must not be empty",
            Self::TooLong => "source ID must not exceed 1024 bytes",
            Self::UnsafeCharacter => "source ID must contain printable ASCII characters only",
        })
    }
}

impl std::error::Error for SourceIdError {}

/// Lowercase SHA-256 digest of source content.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct ContentHash(String);

impl ContentHash {
    /// Creates a validated lowercase SHA-256 digest.
    ///
    /// # Errors
    ///
    /// Returns [`ContentHashError`] unless the value is exactly 64 lowercase hexadecimal bytes.
    pub fn new(value: impl Into<String>) -> Result<Self, ContentHashError> {
        let value = value.into();
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(ContentHashError);
        }
        Ok(Self(value))
    }

    /// Returns the lowercase digest text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ContentHash {
    type Error = ContentHashError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ContentHash> for String {
    fn from(value: ContentHash) -> Self {
        value.0
    }
}

/// Returned when a content hash is not lowercase SHA-256 text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContentHashError;

impl fmt::Display for ContentHashError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("content hash must be 64 lowercase hexadecimal characters")
    }
}

impl std::error::Error for ContentHashError {}

/// Origin category for one context candidate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// File discovered inside an allowed repository root.
    RepositoryFile,
    /// Definition snippet derived from an admitted repository file.
    RepositorySymbol,
    /// Context returned by an optional repository graph provider.
    RepositoryGraph,
    /// Record returned by the local memory provider.
    Memory,
}

/// Data sensitivity state after provider policy has run.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Sensitivity {
    /// No sensitive content was identified by the active provider policy.
    #[default]
    Public,
    /// Content is intentionally private to the local user or repository scope.
    Private,
    /// One or more sensitive values were removed before candidate admission.
    Redacted,
}

/// Address and optional line range for provenance display and retrieval.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceLocation {
    /// Provider-specific display URI, such as a repository-relative path.
    pub uri: String,
    /// Optional one-based first line.
    pub start_line: Option<u32>,
    /// Optional one-based inclusive last line.
    pub end_line: Option<u32>,
}

/// Deterministic relevance inputs supplied by a provider or query matcher.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RelevanceSignals {
    /// Candidate contains an exact requested term or symbol.
    #[serde(default)]
    pub exact_match: bool,
    /// Number of distinct normalized query terms present in this candidate.
    #[serde(default)]
    pub term_match_count: u8,
    /// Candidate path matches a named path or symbol hint.
    #[serde(default)]
    pub path_match: bool,
    /// Candidate contains a diagnostic relevant to the request.
    #[serde(default)]
    pub diagnostic: bool,
    /// Provider-normalized freshness score from zero to 100.
    #[serde(default)]
    pub freshness: u8,
}

impl RelevanceSignals {
    /// Returns a stable integer score with no floating-point ordering ambiguity.
    #[must_use]
    pub fn score(self) -> u32 {
        let exact = if self.exact_match { 800 } else { 0 };
        let path = if self.path_match { 400 } else { 0 };
        let diagnostic = if self.diagnostic { 1_000 } else { 0 };
        exact
            + u32::from(self.term_match_count) * 100
            + path
            + diagnostic
            + u32::from(self.freshness)
    }

    /// Returns the nonzero components that make up [`Self::score`].
    #[must_use]
    pub fn breakdown(self) -> Vec<ScoreComponent> {
        let mut components = Vec::new();
        if self.exact_match {
            components.push(ScoreComponent {
                kind: ScoreComponentKind::ExactMatch,
                value: 800,
            });
        }
        if self.term_match_count > 0 {
            components.push(ScoreComponent {
                kind: ScoreComponentKind::QueryCoverage,
                value: u32::from(self.term_match_count) * 100,
            });
        }
        if self.path_match {
            components.push(ScoreComponent {
                kind: ScoreComponentKind::PathMatch,
                value: 400,
            });
        }
        if self.diagnostic {
            components.push(ScoreComponent {
                kind: ScoreComponentKind::DiagnosticPriority,
                value: 1_000,
            });
        }
        if self.freshness > 0 {
            components.push(ScoreComponent {
                kind: ScoreComponentKind::Freshness,
                value: u32::from(self.freshness),
            });
        }
        components
    }
}

/// Stable component contributing to a deterministic relevance score.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScoreComponentKind {
    /// Exact term or symbol match.
    ExactMatch,
    /// Distinct normalized query terms present in the candidate.
    QueryCoverage,
    /// Named path or symbol-path match.
    PathMatch,
    /// Diagnostic evidence priority.
    DiagnosticPriority,
    /// Provider-normalized freshness contribution.
    Freshness,
}

/// One explainable relevance-score contribution.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScoreComponent {
    /// Stable component category.
    pub kind: ScoreComponentKind,
    /// Nonnegative integer contribution.
    pub value: u32,
}

/// Raw provider candidate before deduplication and budget packing.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContextCandidate {
    /// Opaque follow-up retrieval handle.
    pub source_id: SourceId,
    /// Provider origin category.
    pub source_kind: SourceKind,
    /// Human-readable location.
    pub location: SourceLocation,
    /// Raw candidate content.
    pub content: String,
    /// Digest of the raw candidate content.
    pub content_hash: ContentHash,
    /// Sensitivity state after provider policy.
    #[serde(default)]
    pub sensitivity: Sensitivity,
    /// Optional source modification time in Unix milliseconds.
    pub modified_unix_ms: Option<i64>,
    /// Deterministic ranking signals.
    #[serde(default)]
    pub relevance: RelevanceSignals,
}

/// Why a candidate did not appear in the packed bundle.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum OmissionReason {
    /// Identical content was already retained through another source.
    Duplicate {
        /// Source selected as the canonical copy.
        kept_source_id: SourceId,
    },
    /// Candidate did not fit in the remaining token budget.
    BudgetExceeded,
}

/// Addressable record explaining an omitted candidate.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Omission {
    /// Omitted source handle.
    pub source_id: SourceId,
    /// Deterministic omission reason.
    pub reason: OmissionReason,
}

/// Candidate selected into the optimized context bundle.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContextItem {
    /// Original source handle.
    pub source_id: SourceId,
    /// Source origin category.
    pub source_kind: SourceKind,
    /// Human-readable source location.
    pub location: SourceLocation,
    /// Selected raw content.
    pub content: String,
    /// Digest of the raw selected content.
    pub content_hash: ContentHash,
    /// Sensitivity state carried from the normalized candidate.
    pub sensitivity: Sensitivity,
    /// Stable relevance score used for ordering.
    pub score: u32,
    /// Ordered nonzero score contributions.
    pub score_breakdown: Vec<ScoreComponent>,
    /// Labeled count used for budget packing.
    pub token_count: TokenCount,
}

/// Deterministically ranked, deduplicated, and budget-bounded context.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContextBundle {
    /// Selected evidence in deterministic rank order.
    pub items: Vec<ContextItem>,
    /// Addressable candidates omitted during deduplication or packing.
    pub omissions: Vec<Omission>,
    /// Total labeled count of selected candidate content.
    pub used: TokenCount,
    /// Effective request budget.
    pub budget: TokenBudget,
    /// Digest of ordered selected source IDs, content hashes, scores, and counts.
    pub bundle_hash: ContentHash,
    /// Content-free pipeline counters safe for local trace events.
    pub trace: ContextTrace,
}

/// Content-free context pipeline trace suitable for telemetry storage.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContextTrace {
    /// Number of raw candidates supplied.
    pub candidate_count: u64,
    /// Number omitted as exact-content duplicates.
    pub duplicate_count: u64,
    /// Number included in the final bundle.
    pub included_count: u64,
    /// Number omitted because of the budget.
    pub over_budget_count: u64,
    /// Selected count under the active counter.
    pub used_tokens: u64,
}

/// Deduplicates, ranks, and packs candidates without exceeding the effective budget.
#[must_use]
pub fn build_context<C: TokenCounter>(
    candidates: &[ContextCandidate],
    budget: TokenBudget,
    counter: &C,
) -> ContextBundle {
    let mut winners = BTreeMap::<ContentHash, usize>::new();
    for (index, candidate) in candidates.iter().enumerate() {
        winners
            .entry(candidate.content_hash.clone())
            .and_modify(|winner| {
                let current = &candidates[*winner];
                if candidate.relevance.score() > current.relevance.score()
                    || (candidate.relevance.score() == current.relevance.score()
                        && candidate.source_id < current.source_id)
                {
                    *winner = index;
                }
            })
            .or_insert(index);
    }

    let mut omissions = Vec::new();
    for (index, candidate) in candidates.iter().enumerate() {
        let kept_index = winners[&candidate.content_hash];
        if kept_index != index {
            omissions.push(Omission {
                source_id: candidate.source_id.clone(),
                reason: OmissionReason::Duplicate {
                    kept_source_id: candidates[kept_index].source_id.clone(),
                },
            });
        }
    }
    let duplicate_count = u64::try_from(omissions.len()).unwrap_or(u64::MAX);

    let mut ranked: Vec<_> = winners.into_values().collect();
    ranked.sort_by(|left, right| {
        candidates[*right]
            .relevance
            .score()
            .cmp(&candidates[*left].relevance.score())
            .then_with(|| {
                candidates[*left]
                    .source_id
                    .cmp(&candidates[*right].source_id)
            })
    });

    let limit = u64::from(budget.get());
    let mut used = 0_u64;
    let mut items = Vec::new();
    for index in ranked {
        let candidate = &candidates[index];
        let token_count = counter.count(&candidate.content);
        if used.saturating_add(token_count.tokens()) > limit {
            omissions.push(Omission {
                source_id: candidate.source_id.clone(),
                reason: OmissionReason::BudgetExceeded,
            });
            continue;
        }
        used += token_count.tokens();
        items.push(ContextItem {
            source_id: candidate.source_id.clone(),
            source_kind: candidate.source_kind,
            location: candidate.location.clone(),
            content: candidate.content.clone(),
            content_hash: candidate.content_hash.clone(),
            sensitivity: candidate.sensitivity,
            score: candidate.relevance.score(),
            score_breakdown: candidate.relevance.breakdown(),
            token_count,
        });
    }

    let over_budget_count = u64::try_from(omissions.len())
        .unwrap_or(u64::MAX)
        .saturating_sub(duplicate_count);
    let trace = ContextTrace {
        candidate_count: u64::try_from(candidates.len()).unwrap_or(u64::MAX),
        duplicate_count,
        included_count: u64::try_from(items.len()).unwrap_or(u64::MAX),
        over_budget_count,
        used_tokens: used,
    };
    let bundle_hash = hash_bundle(&items);

    ContextBundle {
        items,
        omissions,
        used: TokenCount::new(used, counter.precision(), counter.id()),
        budget,
        bundle_hash,
        trace,
    }
}

fn hash_bundle(items: &[ContextItem]) -> ContentHash {
    let mut hasher = Sha256::new();
    for item in items {
        update_framed(&mut hasher, item.source_id.as_str().as_bytes());
        update_framed(&mut hasher, item.content_hash.as_str().as_bytes());
        hasher.update(item.score.to_be_bytes());
        hasher.update(item.token_count.tokens().to_be_bytes());
    }
    digest_to_content_hash(hasher.finalize())
}

fn update_framed(hasher: &mut Sha256, value: &[u8]) {
    hasher.update(u64::try_from(value.len()).unwrap_or(u64::MAX).to_be_bytes());
    hasher.update(value);
}

fn digest_to_content_hash(digest: impl AsRef<[u8]>) -> ContentHash {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(64);
    for &byte in digest.as_ref() {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    ContentHash::new(encoded).expect("SHA-256 formatter emits lowercase hexadecimal")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ContextFixture {
        budget: TokenBudget,
        candidates: Vec<ContextCandidate>,
        expected_selected: Vec<String>,
        expected_omitted: Vec<String>,
        expected_used: u64,
    }

    fn candidate(
        source: &str,
        hash_digit: char,
        content: &str,
        relevance: RelevanceSignals,
    ) -> ContextCandidate {
        ContextCandidate {
            source_id: SourceId::new(source).expect("valid fixture source ID"),
            source_kind: SourceKind::RepositoryFile,
            location: SourceLocation {
                uri: source.to_owned(),
                start_line: None,
                end_line: None,
            },
            content: content.to_owned(),
            content_hash: ContentHash::new(hash_digit.to_string().repeat(64))
                .expect("valid fixture hash"),
            sensitivity: Sensitivity::Public,
            modified_unix_ms: None,
            relevance,
        }
    }

    #[test]
    fn estimator_is_deterministic_and_labeled() {
        let estimator = ConservativeEstimator;
        let first = estimator.count("café");
        let second = estimator.count("café");

        assert_eq!(first, second);
        assert_eq!(first.tokens(), "café".len() as u64);
        assert_eq!(first.precision(), CountPrecision::Estimated);
        assert_eq!(first.counter_id().as_str(), CONSERVATIVE_ESTIMATOR_ID);
    }

    #[test]
    fn estimate_is_nonnegative_and_monotonic_for_fixture_corpus() {
        let estimator = ConservativeEstimator;
        for text in ["", "a", "hello world", "你好", "fn main() {}", "not false"] {
            let base = estimator.count(text).tokens();
            let extended = estimator.count(&format!("{text}x")).tokens();
            assert!(extended >= base);
        }
    }

    #[test]
    fn selection_never_exceeds_budget_and_can_skip_large_items() {
        let estimator = ConservativeEstimator;
        let budget = TokenBudget::from_u32(8).expect("nonzero test budget");
        let selection = select_within_budget(&["abc", "oversized", "de"], budget, &estimator);

        assert_eq!(selection.selected, vec![0, 2]);
        assert_eq!(selection.omitted, vec![1]);
        assert_eq!(selection.used.tokens(), 5);
        assert!(selection.used.tokens() <= u64::from(budget.get()));
    }

    #[test]
    fn context_build_deduplicates_ranks_and_obeys_budget() {
        let candidates = vec![
            candidate(
                "repo:duplicate-low.rs",
                'a',
                "same",
                RelevanceSignals::default(),
            ),
            candidate(
                "repo:duplicate-high.rs",
                'a',
                "same",
                RelevanceSignals {
                    exact_match: true,
                    ..RelevanceSignals::default()
                },
            ),
            candidate(
                "repo:diagnostic.rs",
                'b',
                "error",
                RelevanceSignals {
                    diagnostic: true,
                    ..RelevanceSignals::default()
                },
            ),
            candidate(
                "repo:too-large.rs",
                'c',
                "this does not fit",
                RelevanceSignals {
                    path_match: true,
                    ..RelevanceSignals::default()
                },
            ),
        ];
        let budget = TokenBudget::from_u32(9).expect("nonzero fixture budget");

        let bundle = build_context(&candidates, budget, &ConservativeEstimator);

        assert_eq!(
            bundle
                .items
                .iter()
                .map(|item| item.source_id.as_str())
                .collect::<Vec<_>>(),
            vec!["repo:diagnostic.rs", "repo:duplicate-high.rs"]
        );
        assert_eq!(bundle.used.tokens(), 9);
        assert_eq!(bundle.omissions.len(), 2);
        assert!(bundle.used.tokens() <= u64::from(bundle.budget.get()));
    }

    #[test]
    fn public_context_fixture_matches_contract() {
        let fixture: ContextFixture =
            serde_json::from_str(include_str!("../../../fixtures/context/v1.json"))
                .expect("valid context fixture");

        let bundle = build_context(&fixture.candidates, fixture.budget, &ConservativeEstimator);

        assert_eq!(
            bundle
                .items
                .iter()
                .map(|item| item.source_id.as_str().to_owned())
                .collect::<Vec<_>>(),
            fixture.expected_selected
        );
        assert_eq!(
            bundle
                .omissions
                .iter()
                .map(|omission| omission.source_id.as_str().to_owned())
                .collect::<Vec<_>>(),
            fixture.expected_omitted
        );
        assert_eq!(bundle.used.tokens(), fixture.expected_used);
        assert_eq!(bundle.trace.candidate_count, 4);
        assert_eq!(bundle.trace.duplicate_count, 1);
        assert_eq!(bundle.trace.included_count, 2);
        assert_eq!(bundle.trace.over_budget_count, 1);
        assert_eq!(bundle.bundle_hash.as_str().len(), 64);
    }

    #[test]
    fn identical_input_produces_identical_bundle_and_hash() {
        let candidates = vec![candidate(
            "repo:stable.rs",
            'd',
            "stable",
            RelevanceSignals {
                exact_match: true,
                freshness: 5,
                ..RelevanceSignals::default()
            },
        )];
        let budget = TokenBudget::from_u32(100).expect("nonzero fixture budget");

        let first = build_context(&candidates, budget, &ConservativeEstimator);
        let second = build_context(&candidates, budget, &ConservativeEstimator);

        assert_eq!(first, second);
        assert_eq!(
            first.items[0]
                .score_breakdown
                .iter()
                .map(|component| component.value)
                .sum::<u32>(),
            first.items[0].score
        );
    }

    #[test]
    fn provenance_identifiers_reject_invalid_wire_values() {
        assert_eq!(SourceId::new(""), Err(SourceIdError::Empty));
        assert_eq!(
            SourceId::new("bad\nsource"),
            Err(SourceIdError::UnsafeCharacter)
        );
        assert_eq!(ContentHash::new("A".repeat(64)), Err(ContentHashError));
        assert_eq!(ContentHash::new("a".repeat(63)), Err(ContentHashError));
    }
}
