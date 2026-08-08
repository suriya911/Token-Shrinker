//! Human-facing output profiles and preservation policy.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Supported human-output compression profiles.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputMode {
    /// Full sentences with filler removed.
    Lite,
    /// Concise fragments with technical detail preserved.
    #[default]
    Full,
    /// Shortest unambiguous English form.
    Ultra,
    /// Concise semi-classical Chinese.
    WenyanLite,
    /// Strongly compressed classical Chinese.
    WenyanFull,
    /// Maximum unambiguous classical Chinese compression.
    WenyanUltra,
    /// No human-output compression.
    Off,
}

impl OutputMode {
    fn clearer(self) -> Self {
        match self {
            Self::Full | Self::Ultra => Self::Lite,
            Self::WenyanFull | Self::WenyanUltra => Self::WenyanLite,
            Self::Lite | Self::WenyanLite | Self::Off => self,
        }
    }
}

/// Configured output defaults and named scope overrides.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfileConfig {
    /// Global fallback mode.
    #[serde(default)]
    pub default_mode: OutputMode,
    /// Whether safety-sensitive text temporarily uses a clearer profile.
    #[serde(default = "enabled")]
    pub auto_clarity: bool,
    /// Per-agent overrides.
    #[serde(default)]
    pub agents: BTreeMap<String, OutputMode>,
    /// Per-tool overrides.
    #[serde(default)]
    pub tools: BTreeMap<String, OutputMode>,
}

impl Default for ProfileConfig {
    fn default() -> Self {
        Self {
            default_mode: OutputMode::Full,
            auto_clarity: true,
            agents: BTreeMap::new(),
            tools: BTreeMap::new(),
        }
    }
}

const fn enabled() -> bool {
    true
}

/// Kind of payload presented to the output-policy engine.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PayloadKind {
    /// Normalized prose intended for a person.
    #[default]
    HumanText,
    /// JSON, CSV, or another machine-readable representation.
    MachineReadable,
    /// Raw retained evidence or provenance content.
    RawEvidence,
    /// Source code requiring byte-for-byte preservation.
    SourceCode,
    /// Message authored by a third party.
    ThirdPartyMessage,
    /// Prompt, reasoning, or other internal-only text.
    Internal,
}

/// Safety condition that may require clearer prose.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClarityTrigger {
    /// Normal human-facing output.
    #[default]
    Routine,
    /// Security-sensitive warning or instruction.
    SecurityWarning,
    /// Irreversible or destructive action.
    IrreversibleAction,
    /// Ordered recovery sequence where order must remain explicit.
    OrderedRecovery,
    /// Any text whose compression could change meaning.
    MeaningSensitive,
}

/// Inputs used to resolve one output decision.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FormatRequest {
    /// Calling agent identity.
    #[serde(default)]
    pub agent: Option<String>,
    /// Producing tool identity.
    #[serde(default)]
    pub tool: Option<String>,
    /// Current session override.
    #[serde(default)]
    pub session_mode: Option<OutputMode>,
    /// Per-request override.
    #[serde(default)]
    pub request_mode: Option<OutputMode>,
    /// Payload safety class.
    #[serde(default)]
    pub payload_kind: PayloadKind,
    /// Clarity requirement for this result.
    #[serde(default)]
    pub clarity: ClarityTrigger,
}

/// Scope that supplied the configured mode.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileSource {
    /// Per-request override.
    Request,
    /// Session override.
    Session,
    /// Producing-tool override.
    Tool,
    /// Calling-agent override.
    Agent,
    /// Global default.
    Global,
}

/// Why formatting must not touch a payload.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BypassReason {
    /// Payload is structured for another program.
    MachineReadable,
    /// Payload is retained source evidence.
    RawEvidence,
    /// Payload is source code.
    SourceCode,
    /// Payload belongs to a third party.
    ThirdPartyMessage,
    /// Payload is internal and outside the formatter boundary.
    Internal,
}

/// Fully resolved formatting policy for one payload.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FormatDecision {
    /// Mode selected through scoped precedence.
    pub configured_mode: OutputMode,
    /// Mode safe to apply after bypass and clarity rules.
    pub effective_mode: OutputMode,
    /// Scope that selected the configured mode.
    pub source: ProfileSource,
    /// Present when the payload must remain untouched.
    pub bypass: Option<BypassReason>,
    /// Present when automatic clarity changed the configured mode.
    pub clarity_trigger: Option<ClarityTrigger>,
}

/// Resolves request, session, tool, agent, and global policy in priority order.
#[must_use]
pub fn resolve(config: &ProfileConfig, request: &FormatRequest) -> FormatDecision {
    let (configured_mode, source) = request
        .request_mode
        .map(|mode| (mode, ProfileSource::Request))
        .or_else(|| {
            request
                .session_mode
                .map(|mode| (mode, ProfileSource::Session))
        })
        .or_else(|| {
            request.tool.as_ref().and_then(|tool| {
                config
                    .tools
                    .get(tool)
                    .copied()
                    .map(|mode| (mode, ProfileSource::Tool))
            })
        })
        .or_else(|| {
            request.agent.as_ref().and_then(|agent| {
                config
                    .agents
                    .get(agent)
                    .copied()
                    .map(|mode| (mode, ProfileSource::Agent))
            })
        })
        .unwrap_or((config.default_mode, ProfileSource::Global));

    if let Some(bypass) = bypass_reason(request.payload_kind) {
        return FormatDecision {
            configured_mode,
            effective_mode: OutputMode::Off,
            source,
            bypass: Some(bypass),
            clarity_trigger: None,
        };
    }

    let clarified_mode = configured_mode.clearer();
    let clarity_trigger = (config.auto_clarity
        && request.clarity != ClarityTrigger::Routine
        && clarified_mode != configured_mode)
        .then_some(request.clarity);

    FormatDecision {
        configured_mode,
        effective_mode: clarity_trigger.map_or(configured_mode, |_| clarified_mode),
        source,
        bypass: None,
        clarity_trigger,
    }
}

const fn bypass_reason(kind: PayloadKind) -> Option<BypassReason> {
    match kind {
        PayloadKind::HumanText => None,
        PayloadKind::MachineReadable => Some(BypassReason::MachineReadable),
        PayloadKind::RawEvidence => Some(BypassReason::RawEvidence),
        PayloadKind::SourceCode => Some(BypassReason::SourceCode),
        PayloadKind::ThirdPartyMessage => Some(BypassReason::ThirdPartyMessage),
        PayloadKind::Internal => Some(BypassReason::Internal),
    }
}

/// Text fragments that a future formatter must preserve verbatim.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtectedSpanKind {
    /// Source-code span.
    Code,
    /// Executable command or argument.
    Command,
    /// Filesystem path.
    Path,
    /// Exact error text.
    Error,
    /// Number or unit.
    NumberOrUnit,
    /// Citation or provenance identifier.
    Citation,
    /// Warning text.
    Warning,
    /// Uncertainty qualifier.
    Uncertainty,
    /// Meaning-bearing negation such as `not`, `never`, or `only`.
    Negation,
}

/// Non-configurable preservation boundary for human-output compression.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PreservationPolicy;

impl PreservationPolicy {
    /// Returns whether the span must survive formatting unchanged.
    #[must_use]
    pub const fn protects(self, _kind: ProtectedSpanKind) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct FixtureCase {
        name: String,
        config: ProfileConfig,
        request: FormatRequest,
        expected: FormatDecision,
    }

    #[test]
    fn public_profile_fixtures_match_policy() {
        let cases: Vec<FixtureCase> =
            serde_json::from_str(include_str!("../../../fixtures/output-profiles/v1.json"))
                .expect("valid output-profile fixture");

        for case in cases {
            assert_eq!(
                resolve(&case.config, &case.request),
                case.expected,
                "fixture: {}",
                case.name
            );
        }
    }

    #[test]
    fn every_protected_span_class_is_mandatory() {
        let policy = PreservationPolicy;
        for kind in [
            ProtectedSpanKind::Code,
            ProtectedSpanKind::Command,
            ProtectedSpanKind::Path,
            ProtectedSpanKind::Error,
            ProtectedSpanKind::NumberOrUnit,
            ProtectedSpanKind::Citation,
            ProtectedSpanKind::Warning,
            ProtectedSpanKind::Uncertainty,
            ProtectedSpanKind::Negation,
        ] {
            assert!(policy.protects(kind));
        }
    }

    #[test]
    fn every_non_human_payload_bypasses_formatting() {
        let config = ProfileConfig::default();
        for kind in [
            PayloadKind::MachineReadable,
            PayloadKind::RawEvidence,
            PayloadKind::SourceCode,
            PayloadKind::ThirdPartyMessage,
            PayloadKind::Internal,
        ] {
            let decision = resolve(
                &config,
                &FormatRequest {
                    payload_kind: kind,
                    ..FormatRequest::default()
                },
            );
            assert_eq!(decision.effective_mode, OutputMode::Off);
            assert!(decision.bypass.is_some());
        }
    }
}
