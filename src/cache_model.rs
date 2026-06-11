use std::collections::{BTreeMap, HashMap};

use serde::Serialize;

use crate::{cache_key, catalog, parameters::ParameterSchema};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedOnshapeSourceIdentity {
    pub document_id: String,
    pub version_id: String,
    pub microversion_id: String,
    pub element_id: String,
    pub element_kind: catalog::ElementKind,
    pub link_document_id: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceHashPayload<'a> {
    document_id: &'a str,
    microversion_id: &'a str,
    element_id: &'a str,
    element_kind: &'a str,
    link_document_id: Option<&'a str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfigHashPayload<'a> {
    canonicalization_version: u32,
    source_hash: &'a str,
    parameter_schema_version: u32,
    values: BTreeMap<String, String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OptionsHashPayload<'a, T> {
    exporter_version: &'static str,
    options_version: &'static str,
    format: &'a str,
    options: &'a T,
}

pub fn canonical_values(values: &HashMap<String, String>) -> BTreeMap<String, String> {
    values
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

pub fn source_hash(identity: &ResolvedOnshapeSourceIdentity) -> anyhow::Result<String> {
    cache_key::hash_json(
        "source-v2",
        &SourceHashPayload {
            document_id: &identity.document_id,
            microversion_id: &identity.microversion_id,
            element_id: &identity.element_id,
            element_kind: identity.element_kind.key(),
            link_document_id: identity.link_document_id.as_deref(),
        },
    )
}

pub fn config_hash(
    source_hash: &str,
    parameter_schema_version: u32,
    values: &HashMap<String, String>,
) -> anyhow::Result<String> {
    cache_key::hash_json(
        "config-v2",
        &ConfigHashPayload {
            canonicalization_version: cache_key::CANONICALIZATION_VERSION,
            source_hash,
            parameter_schema_version,
            values: canonical_values(values),
        },
    )
}

pub fn parameter_schema_hash(schema: &ParameterSchema) -> anyhow::Result<String> {
    cache_key::hash_json("parameter-schema-v2", schema)
}

pub fn options_hash<T>(
    format: &str,
    exporter_version: &'static str,
    options_version: &'static str,
    options: &T,
) -> anyhow::Result<String>
where
    T: Serialize,
{
    cache_key::hash_json(
        "options-v2",
        &OptionsHashPayload {
            exporter_version,
            options_version,
            format,
            options,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::ElementKind;

    #[test]
    fn source_hash_ignores_version_id_once_microversion_is_resolved() {
        let first = ResolvedOnshapeSourceIdentity {
            document_id: "did".to_owned(),
            version_id: "vid-1".to_owned(),
            microversion_id: "mid".to_owned(),
            element_id: "eid".to_owned(),
            element_kind: ElementKind::PartStudio,
            link_document_id: None,
        };
        let second = ResolvedOnshapeSourceIdentity {
            version_id: "vid-2".to_owned(),
            ..first.clone()
        };

        assert_eq!(source_hash(&first).unwrap(), source_hash(&second).unwrap());
    }

    #[test]
    fn source_hash_changes_when_microversion_changes() {
        let first = ResolvedOnshapeSourceIdentity {
            document_id: "did".to_owned(),
            version_id: "vid".to_owned(),
            microversion_id: "mid-1".to_owned(),
            element_id: "eid".to_owned(),
            element_kind: ElementKind::PartStudio,
            link_document_id: None,
        };
        let second = ResolvedOnshapeSourceIdentity {
            microversion_id: "mid-2".to_owned(),
            ..first.clone()
        };

        assert_ne!(source_hash(&first).unwrap(), source_hash(&second).unwrap());
    }

    #[test]
    fn config_hash_uses_source_hash_and_canonical_values() {
        let source_hash = "source";
        let first = HashMap::from([
            ("b".to_owned(), "2".to_owned()),
            ("a".to_owned(), "1".to_owned()),
        ]);
        let second = HashMap::from([
            ("a".to_owned(), "1".to_owned()),
            ("b".to_owned(), "2".to_owned()),
        ]);

        assert_eq!(
            config_hash(source_hash, 2, &first).unwrap(),
            config_hash(source_hash, 2, &second).unwrap()
        );
        assert_ne!(
            config_hash(source_hash, 2, &first).unwrap(),
            config_hash("other", 2, &second).unwrap()
        );
    }
}
