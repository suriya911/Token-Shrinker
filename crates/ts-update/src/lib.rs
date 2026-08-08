//! Signed compatibility manifests and read-only update checks.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signature, VerifyingKey};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fmt};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use token_shrinker_types::ProtocolVersion;

/// Compatibility-manifest schema implemented by this build.
pub const MANIFEST_SCHEMA_VERSION: u16 = 1;

/// Signed compatibility manifest fetched from an authoritative source.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompatibilityManifest {
    /// Schema generation.
    pub schema_version: u16,
    /// RFC 3339 generation timestamp.
    pub generated_at: String,
    /// RFC 3339 hard expiry.
    pub expires_at: String,
    /// Independently owned components.
    pub components: Vec<Component>,
    /// Signature over the canonical unsigned manifest.
    pub signature: ManifestSignature,
}

/// One updateable component and its authoritative ownership.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Component {
    /// Stable component identifier.
    pub id: String,
    /// Exact source identity trusted for this component.
    pub authoritative_source: String,
    /// Package manager or external owner.
    pub ownership: Ownership,
    /// Published releases.
    pub releases: Vec<Release>,
}

/// Owner responsible for applying an update.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Ownership {
    /// JavaScript package manager.
    Npm,
    /// Rust package manager.
    Cargo,
    /// Manually installed artifact.
    Manual,
    /// Third-party manager or integration.
    External,
    /// Reserved for the post-v1 managed store.
    TokenShrinkerManaged,
}

impl Ownership {
    /// Read-only action a user should run through the owning manager.
    #[must_use]
    pub fn action(self, component: &str, version: &str) -> String {
        match self {
            Self::Npm => format!("npm install {component}@{version}"),
            Self::Cargo => format!("cargo install {component} --version {version}"),
            Self::Manual => format!("download {component} {version} from its authoritative source"),
            Self::External => format!("update {component} with its external owner"),
            Self::TokenShrinkerManaged => {
                format!("managed activation for {component} {version} is not enabled in v1")
            }
        }
    }
}

/// One released component version.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Release {
    /// Semantic version.
    pub version: String,
    /// Release channel.
    pub channel: Channel,
    /// Compatible domain-protocol range.
    pub protocol: ProtocolRange,
    /// Platform artifacts.
    pub artifacts: Vec<Artifact>,
}

/// Release stability channel.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    /// Production release.
    Stable,
    /// Preview release.
    Beta,
    /// Early release.
    Alpha,
}

/// Inclusive compatible domain-protocol range.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProtocolRange {
    /// Minimum version.
    pub min: String,
    /// Maximum version.
    pub max: String,
}

/// One immutable platform artifact.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Artifact {
    /// Stable platform key.
    pub platform: String,
    /// Exact artifact URL.
    pub url: String,
    /// Lowercase SHA-256 digest.
    pub sha256: String,
}

/// Ed25519 signature metadata.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManifestSignature {
    /// Must be `ed25519`.
    pub algorithm: String,
    /// Trusted key identifier.
    pub key_id: String,
    /// Base64 signature bytes.
    pub value: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UnsignedManifest<'a> {
    schema_version: u16,
    generated_at: &'a str,
    expires_at: &'a str,
    components: &'a [Component],
}

/// Inputs to one read-only resolution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateQuery<'a> {
    /// Component to inspect.
    pub component: &'a str,
    /// Locally installed semantic version.
    pub installed_version: &'a str,
    /// Locally detected authoritative source.
    pub authoritative_source: &'a str,
    /// Current platform key.
    pub platform: &'a str,
    /// Current domain protocol.
    pub protocol: ProtocolVersion,
    /// Wall clock used for expiry checks.
    pub now: OffsetDateTime,
}

/// Safe update report. It never mutates the installation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateReport {
    /// Component inspected.
    pub component: String,
    /// Installed version.
    pub installed_version: String,
    /// Newest compatible version, if newer.
    pub available_version: Option<String>,
    /// Component owner.
    pub ownership: Ownership,
    /// Exact owner-aware action, if an update is available.
    pub action: Option<String>,
    /// Matching immutable artifact.
    pub artifact: Option<Artifact>,
    /// Explicitly confirms this operation did not mutate state.
    pub read_only: bool,
}

/// Parses, authenticates, and resolves a compatibility manifest.
///
/// # Errors
///
/// Rejects malformed, unknown-key, unsigned, expired, source-mismatched, incompatible, and
/// invalid-version manifests.
pub fn check_update(
    manifest_json: &[u8],
    trusted_keys: &BTreeMap<String, VerifyingKey>,
    query: &UpdateQuery<'_>,
) -> Result<UpdateReport, UpdateError> {
    let manifest: CompatibilityManifest =
        serde_json::from_slice(manifest_json).map_err(UpdateError::Json)?;
    verify_manifest(&manifest, trusted_keys, query.now)?;
    let component = manifest
        .components
        .iter()
        .find(|component| component.id == query.component)
        .ok_or(UpdateError::ComponentNotFound)?;
    if component.authoritative_source != query.authoritative_source {
        return Err(UpdateError::SourceMismatch);
    }
    let installed = Version::parse(query.installed_version).map_err(UpdateError::Version)?;
    let mut compatible = Vec::new();
    for release in &component.releases {
        let version = Version::parse(&release.version).map_err(UpdateError::Version)?;
        if protocol_matches(&release.protocol, query.protocol)? {
            compatible.push((version, release));
        }
    }
    if compatible.is_empty() && !component.releases.is_empty() {
        return Err(UpdateError::ProtocolIncompatible);
    }
    compatible.sort_by(|left, right| right.0.cmp(&left.0));
    let selected = compatible
        .into_iter()
        .find(|(version, _)| version > &installed);
    let (available_version, action, artifact) = selected.map_or_else(
        || (None, None, None),
        |(_, release)| {
            let artifact = release
                .artifacts
                .iter()
                .find(|artifact| artifact.platform == query.platform)
                .cloned();
            (
                Some(release.version.clone()),
                Some(component.ownership.action(&component.id, &release.version)),
                artifact,
            )
        },
    );
    Ok(UpdateReport {
        component: component.id.clone(),
        installed_version: query.installed_version.to_owned(),
        available_version,
        ownership: component.ownership,
        action,
        artifact,
        read_only: true,
    })
}

/// Verifies downloaded bytes against an authenticated artifact digest.
#[must_use]
pub fn verify_artifact(bytes: &[u8], artifact: &Artifact) -> bool {
    let observed = lowercase_hex(&Sha256::digest(bytes));
    constant_time_eq(observed.as_bytes(), artifact.sha256.as_bytes())
}

fn lowercase_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn verify_manifest(
    manifest: &CompatibilityManifest,
    trusted_keys: &BTreeMap<String, VerifyingKey>,
    now: OffsetDateTime,
) -> Result<(), UpdateError> {
    if manifest.schema_version != MANIFEST_SCHEMA_VERSION {
        return Err(UpdateError::SchemaVersion(manifest.schema_version));
    }
    if manifest.signature.algorithm != "ed25519" || manifest.signature.value.is_empty() {
        return Err(UpdateError::Unsigned);
    }
    let generated = parse_time(&manifest.generated_at)?;
    let expires = parse_time(&manifest.expires_at)?;
    if generated > expires || generated > now || now > expires {
        return Err(UpdateError::Expired);
    }
    let key = trusted_keys
        .get(&manifest.signature.key_id)
        .ok_or(UpdateError::UnknownKey)?;
    let signature_bytes = STANDARD
        .decode(&manifest.signature.value)
        .map_err(UpdateError::Base64)?;
    let signature = Signature::from_slice(&signature_bytes).map_err(UpdateError::Signature)?;
    let unsigned = UnsignedManifest {
        schema_version: manifest.schema_version,
        generated_at: &manifest.generated_at,
        expires_at: &manifest.expires_at,
        components: &manifest.components,
    };
    let payload = serde_json::to_vec(&unsigned).map_err(UpdateError::Json)?;
    key.verify_strict(&payload, &signature)
        .map_err(UpdateError::Signature)
}

fn parse_time(value: &str) -> Result<OffsetDateTime, UpdateError> {
    OffsetDateTime::parse(value, &Rfc3339).map_err(UpdateError::Time)
}

fn protocol_matches(range: &ProtocolRange, current: ProtocolVersion) -> Result<bool, UpdateError> {
    let min = Version::parse(&range.min).map_err(UpdateError::Version)?;
    let max = Version::parse(&range.max).map_err(UpdateError::Version)?;
    let current = Version::new(u64::from(current.major), u64::from(current.minor), 0);
    Ok(current >= min && current <= max)
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    let maximum = left.len().max(right.len());
    for index in 0..maximum {
        difference |= usize::from(
            left.get(index).copied().unwrap_or_default()
                ^ right.get(index).copied().unwrap_or_default(),
        );
    }
    difference == 0
}

/// Read-only update validation or resolution failure.
#[derive(Debug)]
pub enum UpdateError {
    /// Invalid JSON or shape.
    Json(serde_json::Error),
    /// Unsupported manifest generation.
    SchemaVersion(u16),
    /// Missing or unsupported signature declaration.
    Unsigned,
    /// Signing key is not trusted.
    UnknownKey,
    /// Base64 signature is invalid.
    Base64(base64::DecodeError),
    /// Signature bytes or verification failed.
    Signature(ed25519_dalek::SignatureError),
    /// Timestamp is invalid.
    Time(time::error::Parse),
    /// Manifest is expired or has an invalid time window.
    Expired,
    /// Requested component does not exist.
    ComponentNotFound,
    /// Manifest source identity differs from the installed component.
    SourceMismatch,
    /// No release supports the running domain protocol.
    ProtocolIncompatible,
    /// Semantic version or protocol range is invalid.
    Version(semver::Error),
}

impl fmt::Display for UpdateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(formatter, "invalid compatibility manifest: {error}"),
            Self::SchemaVersion(version) => {
                write!(formatter, "unsupported manifest schema {version}")
            }
            Self::Unsigned => formatter.write_str("manifest is unsigned"),
            Self::UnknownKey => formatter.write_str("manifest signing key is not trusted"),
            Self::Base64(error) => {
                write!(formatter, "invalid manifest signature encoding: {error}")
            }
            Self::Signature(_) => formatter.write_str("manifest signature verification failed"),
            Self::Time(error) => write!(formatter, "invalid manifest timestamp: {error}"),
            Self::Expired => {
                formatter.write_str("manifest is expired or has an invalid time window")
            }
            Self::ComponentNotFound => formatter.write_str("component is absent from manifest"),
            Self::SourceMismatch => {
                formatter.write_str("component authoritative source does not match")
            }
            Self::ProtocolIncompatible => {
                formatter.write_str("component has no release compatible with this protocol")
            }
            Self::Version(error) => write!(formatter, "invalid semantic version: {error}"),
        }
    }
}

impl std::error::Error for UpdateError {}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer as _, SigningKey};

    fn signed_manifest() -> (Vec<u8>, BTreeMap<String, VerifyingKey>) {
        let signing = SigningKey::from_bytes(&[7_u8; 32]);
        let mut manifest = CompatibilityManifest {
            schema_version: 1,
            generated_at: "2026-08-07T12:00:00Z".to_owned(),
            expires_at: "2026-08-14T12:00:00Z".to_owned(),
            components: vec![Component {
                id: "token-shrinker".to_owned(),
                authoritative_source: "https://github.com/suriya911/Token-Shrinker".to_owned(),
                ownership: Ownership::Cargo,
                releases: vec![Release {
                    version: "0.1.0-alpha.1".to_owned(),
                    channel: Channel::Alpha,
                    protocol: ProtocolRange {
                        min: "1.0.0".to_owned(),
                        max: "1.0.0".to_owned(),
                    },
                    artifacts: vec![Artifact {
                        platform: "windows-x64".to_owned(),
                        url: "https://example.invalid/token-shrinker.zip".to_owned(),
                        sha256: lowercase_hex(&Sha256::digest(b"binary")),
                    }],
                }],
            }],
            signature: ManifestSignature {
                algorithm: "ed25519".to_owned(),
                key_id: "test-key".to_owned(),
                value: String::new(),
            },
        };
        let unsigned = UnsignedManifest {
            schema_version: manifest.schema_version,
            generated_at: &manifest.generated_at,
            expires_at: &manifest.expires_at,
            components: &manifest.components,
        };
        manifest.signature.value = STANDARD.encode(
            signing
                .sign(&serde_json::to_vec(&unsigned).expect("serialize"))
                .to_bytes(),
        );
        let keys = BTreeMap::from([("test-key".to_owned(), signing.verifying_key())]);
        (serde_json::to_vec(&manifest).expect("manifest"), keys)
    }

    fn query<'a>() -> UpdateQuery<'a> {
        UpdateQuery {
            component: "token-shrinker",
            installed_version: "0.0.0",
            authoritative_source: "https://github.com/suriya911/Token-Shrinker",
            platform: "windows-x64",
            protocol: ProtocolVersion::CURRENT,
            now: OffsetDateTime::parse("2026-08-08T12:00:00Z", &Rfc3339).expect("time"),
        }
    }

    #[test]
    fn signed_compatible_manifest_reports_owner_action_without_mutation() {
        let (manifest, keys) = signed_manifest();
        let report = check_update(&manifest, &keys, &query()).expect("valid update");
        assert_eq!(report.available_version.as_deref(), Some("0.1.0-alpha.1"));
        assert_eq!(
            report.action.as_deref(),
            Some("cargo install token-shrinker --version 0.1.0-alpha.1")
        );
        assert!(report.read_only);
        assert!(verify_artifact(
            b"binary",
            report.artifact.as_ref().expect("artifact")
        ));
    }

    #[test]
    fn tampering_expiry_source_and_protocol_mismatch_are_rejected() {
        let (manifest, keys) = signed_manifest();
        let mut tampered: CompatibilityManifest =
            serde_json::from_slice(&manifest).expect("manifest");
        tampered.components[0].releases[0].version = "9.9.9".to_owned();
        let tampered = serde_json::to_vec(&tampered).expect("tampered manifest");
        assert!(matches!(
            check_update(&tampered, &keys, &query()),
            Err(UpdateError::Signature(_))
        ));

        let mut expired_query = query();
        expired_query.now = OffsetDateTime::parse("2026-08-15T12:00:00Z", &Rfc3339).expect("time");
        assert!(matches!(
            check_update(&manifest, &keys, &expired_query),
            Err(UpdateError::Expired)
        ));

        let mut source_query = query();
        source_query.authoritative_source = "https://example.invalid/fork";
        assert!(matches!(
            check_update(&manifest, &keys, &source_query),
            Err(UpdateError::SourceMismatch)
        ));

        let mut protocol_query = query();
        protocol_query.protocol = ProtocolVersion::new(2, 0);
        assert!(matches!(
            check_update(&manifest, &keys, &protocol_query),
            Err(UpdateError::ProtocolIncompatible)
        ));
    }

    #[test]
    fn unknown_key_unsigned_and_bad_checksum_are_rejected() {
        let (manifest, _) = signed_manifest();
        assert!(matches!(
            check_update(&manifest, &BTreeMap::new(), &query()),
            Err(UpdateError::UnknownKey)
        ));
        let mut parsed: CompatibilityManifest = serde_json::from_slice(&manifest).expect("parse");
        parsed.signature.value.clear();
        assert!(matches!(
            check_update(
                &serde_json::to_vec(&parsed).expect("unsigned"),
                &BTreeMap::new(),
                &query()
            ),
            Err(UpdateError::Unsigned)
        ));
        let parsed: CompatibilityManifest = serde_json::from_slice(&manifest).expect("parse");
        let artifact = &parsed.components[0].releases[0].artifacts[0];
        assert!(!verify_artifact(b"tampered", artifact));
    }

    #[test]
    fn committed_compatibility_fixture_has_a_valid_signature() {
        let public_hex = include_str!("../../../fixtures/compatibility/fixture-key-1.pub").trim();
        let mut public = [0_u8; 32];
        for (index, byte) in public.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&public_hex[index * 2..index * 2 + 2], 16).expect("hex");
        }
        let keys = BTreeMap::from([(
            "fixture-key-1".to_owned(),
            VerifyingKey::from_bytes(&public).expect("key"),
        )]);
        let manifest = include_bytes!("../../../fixtures/compatibility/valid-manifest.json");
        let report = check_update(manifest, &keys, &query()).expect("authentic fixture");
        assert_eq!(report.available_version.as_deref(), Some("0.1.0-alpha.1"));
    }
}
