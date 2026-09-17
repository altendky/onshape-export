use std::{
    env,
    ffi::OsString,
    fmt, fs,
    io::Read,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use serde::{
    Deserialize, Deserializer, Serialize,
    de::{MapAccess, SeqAccess, Visitor},
};
use serde_json::{Map, Number, Value};

use crate::{
    cache_key,
    generator_protocol::{IdentityBindings, PROTOCOL_VERSION},
};

pub const CONFIG_PATH_ENVIRONMENT_VARIABLE: &str = "TRUSTED_GENERATOR_CONFIG_PATH";
pub const DEPLOYED_GENERATOR_IDENTITY_DOMAIN: &[u8] = b"onshape-export-deployed-generator-v1\0";
const MAX_IDENTITIES: usize = 256;
const MAX_IDENTITY_LENGTH: usize = 256;
const DUPLICATE_MARKER: &str = "onshape-export-duplicate-field:";

#[derive(Debug, thiserror::Error)]
pub enum DeployedGeneratorError {
    #[error("trusted generator is not configured")]
    NotConfigured,
    #[error("could not read deployed-generator configuration {path}: {message}")]
    ConfigReadFailure { path: PathBuf, message: String },
    #[error("could not decode deployed-generator configuration: {message}")]
    ConfigDecodeFailure { message: String },
    #[error("deployed-generator configuration repeats a field")]
    DuplicateField,
    #[error("deployed-generator configuration is invalid: {message}")]
    ConfigInvalid { message: String },
    #[error("deployed-generator executable does not exist: {path}")]
    ExecutableMissing { path: PathBuf },
    #[error("deployed-generator executable is not a regular file: {path}")]
    ExecutableNotRegularFile { path: PathBuf },
    #[error("deployed-generator executable is not readable: {path}: {message}")]
    ExecutableUnreadable { path: PathBuf, message: String },
    #[error("deployed-generator executable has no Linux executable mode bit: {path}")]
    ExecutableNotExecutable { path: PathBuf },
    #[error(
        "deployed-generator executable digest mismatch: expected {expected}, measured {measured}"
    )]
    BinaryDigestMismatch { expected: String, measured: String },
    #[error("deployed generator does not support the requested combination: {field}")]
    UnsupportedCombination { field: &'static str },
    #[error("could not compute deployed-generator identity: {message}")]
    IdentityFailure { message: String },
}

#[derive(Debug, Clone)]
pub enum DeployedGeneratorAvailability {
    NotConfigured,
    Available(Box<DeployedGenerator>),
}

impl DeployedGeneratorAvailability {
    pub fn load_from_env() -> Result<Self, DeployedGeneratorError> {
        match DeployedGenerator::load_from_env() {
            Ok(generator) => Ok(Self::Available(Box::new(generator))),
            Err(DeployedGeneratorError::NotConfigured) => Ok(Self::NotConfigured),
            Err(error) => Err(error),
        }
    }

    pub fn static_identity(&self) -> Option<&str> {
        match self {
            Self::NotConfigured => None,
            Self::Available(generator) => Some(generator.static_identity()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeployedGeneratorDocument {
    pub executable_path: PathBuf,
    pub package_identity: String,
    pub package_sha256: String,
    pub build_identity: String,
    pub binary_identity: String,
    pub binary_sha256: String,
    pub protocol_version: u32,
    pub dialect_identity: String,
    pub capability_identities: Vec<String>,
    pub input_kind_identity: String,
    pub input_schema_identity: String,
    pub settings_schema_identity: String,
    pub provenance_set_identity: String,
    pub normalization_identity: String,
    pub validation_identity: String,
}

#[derive(Debug, Clone)]
pub struct DeployedGenerator {
    document: DeployedGeneratorDocument,
    static_identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratorCompatibilityRequest {
    pub protocol_version: u32,
    pub dialect_identity: String,
    pub capability_identities: Vec<String>,
    pub input_kind_identity: String,
    pub input_schema_identity: String,
    pub settings_schema_identity: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ImmutableIdentityPayload<'a> {
    package_identity: &'a str,
    package_sha256: &'a str,
    build_identity: &'a str,
    binary_identity: &'a str,
    binary_sha256: &'a str,
    protocol_version: u32,
    dialect_identity: &'a str,
    capability_identities: &'a [String],
    input_kind_identity: &'a str,
    input_schema_identity: &'a str,
    settings_schema_identity: &'a str,
    provenance_set_identity: &'a str,
    normalization_identity: &'a str,
    validation_identity: &'a str,
}

impl DeployedGenerator {
    pub fn load_from_env() -> Result<Self, DeployedGeneratorError> {
        Self::load_from_environment_path(env::var_os(CONFIG_PATH_ENVIRONMENT_VARIABLE))
    }

    fn load_from_environment_path(path: Option<OsString>) -> Result<Self, DeployedGeneratorError> {
        let Some(path) = path else {
            return Err(DeployedGeneratorError::NotConfigured);
        };
        Self::load(Path::new(&path))
    }

    pub fn load(path: &Path) -> Result<Self, DeployedGeneratorError> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(DeployedGeneratorError::NotConfigured);
            }
            Err(error) => {
                return Err(DeployedGeneratorError::ConfigReadFailure {
                    path: path.to_owned(),
                    message: error.to_string(),
                });
            }
        };
        Self::from_bytes(&bytes)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DeployedGeneratorError> {
        let value = parse_strict_value(bytes)?;
        if !value.is_object() {
            return Err(DeployedGeneratorError::ConfigInvalid {
                message: "document must be one JSON object".to_owned(),
            });
        }
        let document: DeployedGeneratorDocument =
            serde_json::from_value(value).map_err(|error| {
                DeployedGeneratorError::ConfigInvalid {
                    message: error.to_string(),
                }
            })?;
        document.validate()?;
        validate_executable(&document)?;
        let static_identity = document.computed_static_identity()?;
        Ok(Self {
            document,
            static_identity,
        })
    }

    pub fn document(&self) -> &DeployedGeneratorDocument {
        &self.document
    }

    pub fn static_identity(&self) -> &str {
        &self.static_identity
    }

    pub fn ensure_compatible(
        &self,
        request: &GeneratorCompatibilityRequest,
    ) -> Result<(), DeployedGeneratorError> {
        for (matches, field) in [
            (
                request.protocol_version == self.document.protocol_version,
                "protocolVersion",
            ),
            (
                request.dialect_identity == self.document.dialect_identity,
                "dialectIdentity",
            ),
            (
                request.capability_identities == self.document.capability_identities,
                "capabilityIdentities",
            ),
            (
                request.input_kind_identity == self.document.input_kind_identity,
                "inputKindIdentity",
            ),
            (
                request.input_schema_identity == self.document.input_schema_identity,
                "inputSchemaIdentity",
            ),
            (
                request.settings_schema_identity == self.document.settings_schema_identity,
                "settingsSchemaIdentity",
            ),
        ] {
            if !matches {
                return Err(DeployedGeneratorError::UnsupportedCombination { field });
            }
        }
        Ok(())
    }

    pub fn identity_bindings(&self, settings_identity: impl Into<String>) -> IdentityBindings {
        IdentityBindings {
            package_identity: self.document.package_identity.clone(),
            build_identity: self.document.build_identity.clone(),
            binary_identity: self.document.binary_identity.clone(),
            dialect_identity: self.document.dialect_identity.clone(),
            capability_identities: self.document.capability_identities.clone(),
            input_kind_identity: self.document.input_kind_identity.clone(),
            input_schema_identity: self.document.input_schema_identity.clone(),
            settings_identity: settings_identity.into(),
            settings_schema_identity: self.document.settings_schema_identity.clone(),
            provenance_set_identity: self.document.provenance_set_identity.clone(),
            normalization_identity: self.document.normalization_identity.clone(),
            validation_identity: self.document.validation_identity.clone(),
        }
    }
}

impl DeployedGeneratorDocument {
    fn validate(&self) -> Result<(), DeployedGeneratorError> {
        ensure_config(
            self.executable_path.is_absolute(),
            "executablePath must be absolute",
        )?;
        ensure_config(
            self.protocol_version == PROTOCOL_VERSION,
            format!("protocolVersion must be the implemented version {PROTOCOL_VERSION}"),
        )?;
        for (field, value) in [
            ("packageIdentity", self.package_identity.as_str()),
            ("buildIdentity", self.build_identity.as_str()),
            ("binaryIdentity", self.binary_identity.as_str()),
            ("dialectIdentity", self.dialect_identity.as_str()),
            ("inputKindIdentity", self.input_kind_identity.as_str()),
            ("inputSchemaIdentity", self.input_schema_identity.as_str()),
            (
                "settingsSchemaIdentity",
                self.settings_schema_identity.as_str(),
            ),
            (
                "provenanceSetIdentity",
                self.provenance_set_identity.as_str(),
            ),
            (
                "normalizationIdentity",
                self.normalization_identity.as_str(),
            ),
            ("validationIdentity", self.validation_identity.as_str()),
        ] {
            validate_identity(value, field)?;
        }
        validate_sha256(&self.package_sha256, "packageSha256")?;
        validate_sha256(&self.binary_sha256, "binarySha256")?;
        ensure_config(
            self.capability_identities.len() <= MAX_IDENTITIES,
            format!("capabilityIdentities exceeds {MAX_IDENTITIES} entries"),
        )?;
        for (index, identity) in self.capability_identities.iter().enumerate() {
            validate_identity(identity, &format!("capabilityIdentities[{index}]"))?;
        }
        ensure_config(
            self.capability_identities
                .windows(2)
                .all(|pair| pair[0] < pair[1]),
            "capabilityIdentities must be unique and sorted lexicographically",
        )
    }

    fn computed_static_identity(&self) -> Result<String, DeployedGeneratorError> {
        let payload = self.immutable_payload();
        let canonical = cache_key::canonical_json_bytes(&payload).map_err(|error| {
            DeployedGeneratorError::IdentityFailure {
                message: error.to_string(),
            }
        })?;
        let mut preimage =
            Vec::with_capacity(DEPLOYED_GENERATOR_IDENTITY_DOMAIN.len() + canonical.len());
        preimage.extend_from_slice(DEPLOYED_GENERATOR_IDENTITY_DOMAIN);
        preimage.extend_from_slice(&canonical);
        Ok(cache_key::hex_sha256(&preimage))
    }

    fn immutable_payload(&self) -> ImmutableIdentityPayload<'_> {
        ImmutableIdentityPayload {
            package_identity: &self.package_identity,
            package_sha256: &self.package_sha256,
            build_identity: &self.build_identity,
            binary_identity: &self.binary_identity,
            binary_sha256: &self.binary_sha256,
            protocol_version: self.protocol_version,
            dialect_identity: &self.dialect_identity,
            capability_identities: &self.capability_identities,
            input_kind_identity: &self.input_kind_identity,
            input_schema_identity: &self.input_schema_identity,
            settings_schema_identity: &self.settings_schema_identity,
            provenance_set_identity: &self.provenance_set_identity,
            normalization_identity: &self.normalization_identity,
            validation_identity: &self.validation_identity,
        }
    }
}

fn validate_executable(document: &DeployedGeneratorDocument) -> Result<(), DeployedGeneratorError> {
    let path = &document.executable_path;
    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(DeployedGeneratorError::ExecutableMissing { path: path.clone() });
        }
        Err(error) => {
            return Err(DeployedGeneratorError::ExecutableUnreadable {
                path: path.clone(),
                message: error.to_string(),
            });
        }
    };
    let metadata =
        file.metadata()
            .map_err(|error| DeployedGeneratorError::ExecutableUnreadable {
                path: path.clone(),
                message: error.to_string(),
            })?;
    if !metadata.is_file() {
        return Err(DeployedGeneratorError::ExecutableNotRegularFile { path: path.clone() });
    }
    if metadata.permissions().mode() & 0o111 == 0 {
        return Err(DeployedGeneratorError::ExecutableNotExecutable { path: path.clone() });
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| DeployedGeneratorError::ExecutableUnreadable {
            path: path.clone(),
            message: error.to_string(),
        })?;
    let measured = cache_key::hex_sha256(&bytes);
    if measured != document.binary_sha256 {
        return Err(DeployedGeneratorError::BinaryDigestMismatch {
            expected: document.binary_sha256.clone(),
            measured,
        });
    }
    Ok(())
}

fn validate_identity(value: &str, field: &str) -> Result<(), DeployedGeneratorError> {
    ensure_config(
        !value.is_empty()
            && value.len() <= MAX_IDENTITY_LENGTH
            && value.bytes().all(|byte| byte.is_ascii_graphic()),
        format!("{field} must contain 1 to {MAX_IDENTITY_LENGTH} visible ASCII characters"),
    )
}

fn validate_sha256(value: &str, field: &str) -> Result<(), DeployedGeneratorError> {
    ensure_config(
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        format!("{field} must be a lowercase 64-character SHA-256 value"),
    )
}

fn ensure_config(
    condition: bool,
    message: impl Into<String>,
) -> Result<(), DeployedGeneratorError> {
    if condition {
        Ok(())
    } else {
        Err(DeployedGeneratorError::ConfigInvalid {
            message: message.into(),
        })
    }
}

fn parse_strict_value(bytes: &[u8]) -> Result<Value, DeployedGeneratorError> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let StrictValue(value) =
        StrictValue::deserialize(&mut deserializer).map_err(map_decode_error)?;
    deserializer.end().map_err(map_decode_error)?;
    Ok(value)
}

fn map_decode_error(error: serde_json::Error) -> DeployedGeneratorError {
    let message = error.to_string();
    if message.contains(DUPLICATE_MARKER) {
        DeployedGeneratorError::DuplicateField
    } else {
        DeployedGeneratorError::ConfigDecodeFailure { message }
    }
}

struct StrictValue(Value);

impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(StrictValueVisitor)
    }
}

struct StrictValueVisitor;

impl<'de> Visitor<'de> for StrictValueVisitor {
    type Value = StrictValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value without duplicate object members")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Number(Number::from(value))))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Number(Number::from(value))))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Number::from_f64(value)
            .map(Value::Number)
            .map(StrictValue)
            .ok_or_else(|| E::custom("non-finite JSON number"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::String(value.to_owned())))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::String(value)))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Null))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Null))
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(StrictValue(value)) = sequence.next_element()? {
            values.push(value);
        }
        Ok(StrictValue(Value::Array(values)))
    }

    fn visit_map<A>(self, mut object: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = Map::new();
        while let Some((key, StrictValue(value))) = object.next_entry::<String, StrictValue>()? {
            if values.contains_key(&key) {
                return Err(serde::de::Error::custom(format!("{DUPLICATE_MARKER}{key}")));
            }
            values.insert(key, value);
        }
        Ok(StrictValue(Value::Object(values)))
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::{PermissionsExt, symlink};

    use serde_json::json;
    use tempfile::TempDir;

    use super::*;

    const SCHEMA: &str =
        include_str!("../config/deployed-generator/v1/deployed-generator.schema.json");
    const EXECUTABLE_BYTES: &[u8] = b"#!/bin/sh\nexit 0\n";

    fn schema() -> Value {
        serde_json::from_str(SCHEMA).unwrap()
    }

    fn executable(directory: &TempDir, name: &str, bytes: &[u8], mode: u32) -> PathBuf {
        let path = directory.path().join(name);
        fs::write(&path, bytes).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(mode);
        fs::set_permissions(&path, permissions).unwrap();
        path
    }

    fn valid_document(path: PathBuf, bytes: &[u8]) -> DeployedGeneratorDocument {
        DeployedGeneratorDocument {
            executable_path: path,
            package_identity: "package-synthetic-v1".to_owned(),
            package_sha256: "1".repeat(64),
            build_identity: "build-synthetic-v1".to_owned(),
            binary_identity: "binary-synthetic-v1".to_owned(),
            binary_sha256: cache_key::hex_sha256(bytes),
            protocol_version: PROTOCOL_VERSION,
            dialect_identity: "dialect-synthetic-v1".to_owned(),
            capability_identities: vec!["capability-a-v1".to_owned(), "capability-b-v1".to_owned()],
            input_kind_identity: "input-kind-synthetic-v1".to_owned(),
            input_schema_identity: "input-schema-synthetic-v1".to_owned(),
            settings_schema_identity: "settings-schema-synthetic-v1".to_owned(),
            provenance_set_identity: "provenance-synthetic-v1".to_owned(),
            normalization_identity: "normalization-synthetic-v1".to_owned(),
            validation_identity: "validation-synthetic-v1".to_owned(),
        }
    }

    fn valid_generator() -> (TempDir, DeployedGenerator) {
        let directory = tempfile::tempdir().unwrap();
        let path = executable(&directory, "generator", EXECUTABLE_BYTES, 0o755);
        let document = valid_document(path, EXECUTABLE_BYTES);
        let generator =
            DeployedGenerator::from_bytes(&serde_json::to_vec(&document).unwrap()).unwrap();
        (directory, generator)
    }

    fn compatibility_request(
        document: &DeployedGeneratorDocument,
    ) -> GeneratorCompatibilityRequest {
        GeneratorCompatibilityRequest {
            protocol_version: document.protocol_version,
            dialect_identity: document.dialect_identity.clone(),
            capability_identities: document.capability_identities.clone(),
            input_kind_identity: document.input_kind_identity.clone(),
            input_schema_identity: document.input_schema_identity.clone(),
            settings_schema_identity: document.settings_schema_identity.clone(),
        }
    }

    #[test]
    fn schema_is_valid_draft_2020_12_and_accepts_valid_document() {
        assert!(jsonschema::draft202012::meta::is_valid(&schema()));
        let directory = tempfile::tempdir().unwrap();
        let document = valid_document(directory.path().join("generator"), EXECUTABLE_BYTES);
        assert!(jsonschema::draft202012::is_valid(
            &schema(),
            &serde_json::to_value(document).unwrap()
        ));
    }

    #[test]
    fn schema_rejects_non_object_unknown_incomplete_and_invalid_fields() {
        let directory = tempfile::tempdir().unwrap();
        let document = valid_document(directory.path().join("generator"), EXECUTABLE_BYTES);
        let valid = serde_json::to_value(document).unwrap();
        for invalid in [
            json!([]),
            {
                let mut value = valid.clone();
                value["unknown"] = json!(true);
                value
            },
            {
                let mut value = valid.clone();
                value.as_object_mut().unwrap().remove("buildIdentity");
                value
            },
            {
                let mut value = valid.clone();
                value["protocolVersion"] = json!(2);
                value
            },
            {
                let mut value = valid.clone();
                value["packageSha256"] = json!("ABC");
                value
            },
            {
                let mut value = valid.clone();
                value["executablePath"] = json!("relative/generator");
                value
            },
        ] {
            assert!(!jsonschema::draft202012::is_valid(&schema(), &invalid));
        }
    }

    #[test]
    fn absent_environment_path_and_missing_document_are_not_configured() {
        assert!(matches!(
            DeployedGenerator::load_from_environment_path(None),
            Err(DeployedGeneratorError::NotConfigured)
        ));
        let directory = tempfile::tempdir().unwrap();
        assert!(matches!(
            DeployedGenerator::load(&directory.path().join("missing.json")),
            Err(DeployedGeneratorError::NotConfigured)
        ));
    }

    #[test]
    fn loads_valid_document_from_the_exact_named_path() {
        let directory = tempfile::tempdir().unwrap();
        let executable_path = executable(&directory, "generator", EXECUTABLE_BYTES, 0o755);
        let document = valid_document(executable_path, EXECUTABLE_BYTES);
        let config_path = directory.path().join("deployed-generator.json");
        fs::write(&config_path, serde_json::to_vec(&document).unwrap()).unwrap();

        let loaded = DeployedGenerator::load(&config_path).unwrap();

        assert_eq!(loaded.document(), &document);
    }

    #[test]
    fn unreadable_configuration_path_is_a_read_failure() {
        let directory = tempfile::tempdir().unwrap();
        assert!(matches!(
            DeployedGenerator::load(directory.path()),
            Err(DeployedGeneratorError::ConfigReadFailure { .. })
        ));
    }

    #[test]
    fn malformed_duplicate_trailing_and_multiple_documents_are_distinct_failures() {
        assert!(matches!(
            DeployedGenerator::from_bytes(br#"{"#),
            Err(DeployedGeneratorError::ConfigDecodeFailure { .. })
        ));
        assert!(matches!(
            DeployedGenerator::from_bytes(br#"{"packageIdentity":"a","packageIdentity":"b"}"#),
            Err(DeployedGeneratorError::DuplicateField)
        ));
        for bytes in [br#"{} trailing"#.as_slice(), br#"{} {}"#.as_slice()] {
            assert!(matches!(
                DeployedGenerator::from_bytes(bytes),
                Err(DeployedGeneratorError::ConfigDecodeFailure { .. })
            ));
        }
    }

    #[test]
    fn arrays_unknown_fields_and_missing_fields_are_invalid_configuration() {
        assert!(matches!(
            DeployedGenerator::from_bytes(b"[]"),
            Err(DeployedGeneratorError::ConfigInvalid { .. })
        ));
        for value in [json!({}), json!({"unknown": true})] {
            assert!(matches!(
                DeployedGenerator::from_bytes(&serde_json::to_vec(&value).unwrap()),
                Err(DeployedGeneratorError::ConfigInvalid { .. })
            ));
        }
    }

    #[test]
    fn rejects_invalid_identity_digest_version_path_and_capability_order() {
        let directory = tempfile::tempdir().unwrap();
        let path = executable(&directory, "generator", EXECUTABLE_BYTES, 0o755);
        let document = valid_document(path, EXECUTABLE_BYTES);
        let cases = [
            {
                let mut value = document.clone();
                value.package_identity = "contains space".to_owned();
                value
            },
            {
                let mut value = document.clone();
                value.package_sha256 = "A".repeat(64);
                value
            },
            {
                let mut value = document.clone();
                value.protocol_version = 2;
                value
            },
            {
                let mut value = document.clone();
                value.executable_path = PathBuf::from("relative/generator");
                value
            },
            {
                let mut value = document.clone();
                value.capability_identities.reverse();
                value
            },
            {
                let mut value = document.clone();
                value.capability_identities[1] = value.capability_identities[0].clone();
                value
            },
        ];
        for invalid in cases {
            assert!(matches!(
                DeployedGenerator::from_bytes(&serde_json::to_vec(&invalid).unwrap()),
                Err(DeployedGeneratorError::ConfigInvalid { .. })
            ));
        }
    }

    #[test]
    fn validates_missing_non_regular_non_executable_and_digest_mismatch() {
        let directory = tempfile::tempdir().unwrap();

        let missing = valid_document(directory.path().join("missing"), EXECUTABLE_BYTES);
        assert!(matches!(
            DeployedGenerator::from_bytes(&serde_json::to_vec(&missing).unwrap()),
            Err(DeployedGeneratorError::ExecutableMissing { .. })
        ));

        let non_regular = valid_document(directory.path().to_owned(), EXECUTABLE_BYTES);
        assert!(matches!(
            DeployedGenerator::from_bytes(&serde_json::to_vec(&non_regular).unwrap()),
            Err(DeployedGeneratorError::ExecutableNotRegularFile { .. })
        ));

        let path = executable(&directory, "not-executable", EXECUTABLE_BYTES, 0o644);
        let not_executable = valid_document(path, EXECUTABLE_BYTES);
        assert!(matches!(
            DeployedGenerator::from_bytes(&serde_json::to_vec(&not_executable).unwrap()),
            Err(DeployedGeneratorError::ExecutableNotExecutable { .. })
        ));

        let path = executable(&directory, "not-readable", EXECUTABLE_BYTES, 0o111);
        let unreadable = valid_document(path, EXECUTABLE_BYTES);
        assert!(matches!(
            DeployedGenerator::from_bytes(&serde_json::to_vec(&unreadable).unwrap()),
            Err(DeployedGeneratorError::ExecutableUnreadable { .. })
        ));

        let path = executable(&directory, "wrong-digest", EXECUTABLE_BYTES, 0o755);
        let mut mismatch = valid_document(path, EXECUTABLE_BYTES);
        mismatch.binary_sha256 = "0".repeat(64);
        assert!(matches!(
            DeployedGenerator::from_bytes(&serde_json::to_vec(&mismatch).unwrap()),
            Err(DeployedGeneratorError::BinaryDigestMismatch { .. })
        ));
    }

    #[test]
    fn accepts_symlink_to_readable_executable_regular_file() {
        let directory = tempfile::tempdir().unwrap();
        let target = executable(&directory, "target", EXECUTABLE_BYTES, 0o755);
        let link = directory.path().join("generator");
        symlink(target, &link).unwrap();
        let document = valid_document(link, EXECUTABLE_BYTES);
        DeployedGenerator::from_bytes(&serde_json::to_vec(&document).unwrap()).unwrap();
    }

    #[test]
    fn static_identity_excludes_path_and_has_stable_domain_separated_value() {
        let first_directory = tempfile::tempdir().unwrap();
        let second_directory = tempfile::tempdir().unwrap();
        let first_path = executable(&first_directory, "first", EXECUTABLE_BYTES, 0o755);
        let second_path = executable(&second_directory, "second", EXECUTABLE_BYTES, 0o755);
        let first = DeployedGenerator::from_bytes(
            &serde_json::to_vec(&valid_document(first_path, EXECUTABLE_BYTES)).unwrap(),
        )
        .unwrap();
        let second = DeployedGenerator::from_bytes(
            &serde_json::to_vec(&valid_document(second_path, EXECUTABLE_BYTES)).unwrap(),
        )
        .unwrap();
        assert_eq!(first.static_identity(), second.static_identity());
        assert_eq!(
            first.static_identity(),
            "60782bb76c5214cd6a46f5d9bb69793661deec5ea3442793c4bd07ab7c99fe2d"
        );

        let payload = first.document.immutable_payload();
        let canonical = cache_key::canonical_json_bytes(&payload).unwrap();
        let mut without_nul = b"onshape-export-deployed-generator-v1".to_vec();
        without_nul.extend_from_slice(&canonical);
        assert_ne!(first.static_identity(), cache_key::hex_sha256(&without_nul));
        assert_ne!(
            first.static_identity(),
            cache_key::hash_json("onshape-export-deployed-generator-v1", &payload).unwrap()
        );
    }

    #[test]
    fn every_immutable_field_changes_static_identity() {
        let directory = tempfile::tempdir().unwrap();
        let path = executable(&directory, "generator", EXECUTABLE_BYTES, 0o755);
        let document = valid_document(path, EXECUTABLE_BYTES);
        let baseline = document.computed_static_identity().unwrap();
        let variants = [
            {
                let mut value = document.clone();
                value.package_identity.push('x');
                value
            },
            {
                let mut value = document.clone();
                value.package_sha256 = "2".repeat(64);
                value
            },
            {
                let mut value = document.clone();
                value.build_identity.push('x');
                value
            },
            {
                let mut value = document.clone();
                value.binary_identity.push('x');
                value
            },
            {
                let mut value = document.clone();
                value.binary_sha256 = "3".repeat(64);
                value
            },
            {
                let mut value = document.clone();
                value.protocol_version += 1;
                value
            },
            {
                let mut value = document.clone();
                value.dialect_identity.push('x');
                value
            },
            {
                let mut value = document.clone();
                value
                    .capability_identities
                    .push("capability-c-v1".to_owned());
                value
            },
            {
                let mut value = document.clone();
                value.input_kind_identity.push('x');
                value
            },
            {
                let mut value = document.clone();
                value.input_schema_identity.push('x');
                value
            },
            {
                let mut value = document.clone();
                value.settings_schema_identity.push('x');
                value
            },
            {
                let mut value = document.clone();
                value.provenance_set_identity.push('x');
                value
            },
            {
                let mut value = document.clone();
                value.normalization_identity.push('x');
                value
            },
            {
                let mut value = document.clone();
                value.validation_identity.push('x');
                value
            },
        ];
        for variant in variants {
            assert_ne!(baseline, variant.computed_static_identity().unwrap());
        }
    }

    #[test]
    fn compatibility_is_exact_without_ranking_or_fallback() {
        let (_directory, generator) = valid_generator();
        let request = compatibility_request(generator.document());
        generator.ensure_compatible(&request).unwrap();

        let mismatches: [(GeneratorCompatibilityRequest, &'static str); 6] = [
            (
                {
                    let mut value = request.clone();
                    value.protocol_version += 1;
                    value
                },
                "protocolVersion",
            ),
            (
                {
                    let mut value = request.clone();
                    value.dialect_identity.push('x');
                    value
                },
                "dialectIdentity",
            ),
            (
                {
                    let mut value = request.clone();
                    value.capability_identities.pop();
                    value
                },
                "capabilityIdentities",
            ),
            (
                {
                    let mut value = request.clone();
                    value.input_kind_identity.push('x');
                    value
                },
                "inputKindIdentity",
            ),
            (
                {
                    let mut value = request.clone();
                    value.input_schema_identity.push('x');
                    value
                },
                "inputSchemaIdentity",
            ),
            (
                {
                    let mut value = request.clone();
                    value.settings_schema_identity.push('x');
                    value
                },
                "settingsSchemaIdentity",
            ),
        ];
        for (mismatch, expected_field) in mismatches {
            assert!(matches!(
                generator.ensure_compatible(&mismatch),
                Err(DeployedGeneratorError::UnsupportedCombination { field }) if field == expected_field
            ));
        }
    }

    #[test]
    fn composes_static_bindings_with_invocation_specific_settings_identity() {
        let (_directory, generator) = valid_generator();
        let bindings = generator.identity_bindings("settings-invocation-synthetic-v1");
        assert_eq!(
            bindings.package_identity,
            generator.document.package_identity
        );
        assert_eq!(bindings.build_identity, generator.document.build_identity);
        assert_eq!(bindings.binary_identity, generator.document.binary_identity);
        assert_eq!(
            bindings.dialect_identity,
            generator.document.dialect_identity
        );
        assert_eq!(
            bindings.capability_identities,
            generator.document.capability_identities
        );
        assert_eq!(
            bindings.input_kind_identity,
            generator.document.input_kind_identity
        );
        assert_eq!(
            bindings.input_schema_identity,
            generator.document.input_schema_identity
        );
        assert_eq!(
            bindings.settings_identity,
            "settings-invocation-synthetic-v1"
        );
        assert_eq!(
            bindings.settings_schema_identity,
            generator.document.settings_schema_identity
        );
        assert_eq!(
            bindings.provenance_set_identity,
            generator.document.provenance_set_identity
        );
        assert_eq!(
            bindings.normalization_identity,
            generator.document.normalization_identity
        );
        assert_eq!(
            bindings.validation_identity,
            generator.document.validation_identity
        );
    }
}
