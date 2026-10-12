//! The real Bambu deployment remains unavailable until its complete neutral
//! release handoff is reviewed. Test-only synthetic entries exercise the service
//! orchestration without asserting approval of missing release identities.

use std::{os::unix::fs::MetadataExt, path::Path};

use anyhow::{bail, ensure};

use crate::{
    deployed_generator::{
        DeployedGenerator, DeployedGeneratorAvailability, GeneratorCompatibilityRequest,
    },
    generator_inputs::GeneratorInputPolicy,
    generator_processing::GeneratorProcessingProtocolInputs,
    generator_protocol::{OutputDeclaration, OutputRole},
};

pub const RELEASE_TAG: &str = "bambu-studio-v0.1.0";
pub const PACKAGE_SHA256: &str = "a9c859962d23de9812a28871fa6a80334b7543ab695b52db73ca2d91387c3d9c";
pub const BINARY_SHA256: &str = "734aa2340c954b7d06f1cea0d6c307b33279ffdd74b10cfe1639cdefc7e5e44c";
pub const INSTALL_ROOT: &str = "/opt/onshape-export/generators/bambu-studio-v0.1.0";

pub fn configured_generator(state: &crate::AppState) -> anyhow::Result<&DeployedGenerator> {
    let Some(DeployedGeneratorAvailability::Available(generator)) =
        state.deployed_generator.as_ref()
    else {
        bail!("Bambu project generation is not configured");
    };
    validate_release(generator)?;
    Ok(generator)
}

/// Called after the existing startup loader; absent configuration stays optional.
pub fn validate_availability(availability: &DeployedGeneratorAvailability) -> anyhow::Result<()> {
    if let DeployedGeneratorAvailability::Available(generator) = availability {
        let document = generator.document();
        if document.package_sha256 == PACKAGE_SHA256 || document.binary_sha256 == BINARY_SHA256 {
            validate_release(generator)?;
        }
    }
    Ok(())
}

fn validate_release(generator: &DeployedGenerator) -> anyhow::Result<()> {
    #[cfg(test)]
    if generator
        .document()
        .dialect_identity
        .starts_with("synthetic-")
    {
        return Ok(());
    }
    let document = generator.document();
    ensure!(
        document.package_sha256 == PACKAGE_SHA256 && document.binary_sha256 == BINARY_SHA256,
        "configured deployment does not match the measured Bambu release bytes"
    );
    ensure!(
        document.executable_path
            == Path::new(INSTALL_ROOT).join("slicer-project-generator-bambu-studio"),
        "Bambu deployment executable must use the reviewed external installation"
    );
    verify_root_owned_path(&document.executable_path, true)?;
    let config = std::env::var_os(crate::deployed_generator::CONFIG_PATH_ENVIRONMENT_VARIABLE)
        .ok_or_else(|| anyhow::anyhow!("Bambu deployment configuration path is absent"))?;
    verify_root_owned_path(Path::new(&config), false)?;
    bail!(
        "{RELEASE_TAG} deployment requires the missing reviewed complete neutral identity handoff"
    );
}

/// The deployment installer creates regular root-owned files in root-owned
/// directories. This check is available for the eventual reviewed configuration;
/// it does not substitute for the missing immutable identity handoff.
pub fn verify_root_owned_path(path: &Path, executable: bool) -> anyhow::Result<()> {
    ensure!(path.is_absolute(), "deployment path must be absolute");
    for (index, current) in path.ancestors().enumerate() {
        let metadata = std::fs::symlink_metadata(current)?;
        let terminal = index == 0;
        ensure!(
            metadata.uid() == 0 && metadata.mode() & 0o022 == 0,
            "deployment files and directories must be root-owned and not group/world writable"
        );
        ensure!(
            if terminal {
                metadata.is_file()
            } else {
                metadata.is_dir()
            },
            "deployment paths must use regular files and directories"
        );
        if terminal && executable {
            ensure!(
                metadata.mode() & 0o111 != 0,
                "deployment executable lacks execute permission"
            );
        }
    }
    Ok(())
}

/// These service-owned declarations are used only after configured_generator
/// has admitted the deployment. Currently admission exists only for synthetic
/// test entries; production identities are not inferred from these values.
pub fn input_policy(generator: &DeployedGenerator) -> GeneratorInputPolicy {
    let document = generator.document();
    GeneratorInputPolicy {
        requirements_identity: "onshape-export-bambu-mvp-v1".to_owned(),
        input_kind_identity: document.input_kind_identity.clone(),
        input_schema_identity: document.input_schema_identity.clone(),
        detected_kind_identity: document.input_kind_identity.clone(),
        media_type: "application/octet-stream".to_owned(),
    }
}

pub fn compatibility(generator: &DeployedGenerator) -> GeneratorCompatibilityRequest {
    let document = generator.document();
    GeneratorCompatibilityRequest {
        protocol_version: document.protocol_version,
        dialect_identity: document.dialect_identity.clone(),
        capability_identities: document.capability_identities.clone(),
        input_kind_identity: document.input_kind_identity.clone(),
        input_schema_identity: document.input_schema_identity.clone(),
        settings_schema_identity: document.settings_schema_identity.clone(),
    }
}

pub fn protocol_inputs(generator: &DeployedGenerator) -> GeneratorProcessingProtocolInputs {
    GeneratorProcessingProtocolInputs {
        manifest_path: "inputs/manifest.json".to_owned(),
        settings_content_identity: "onshape-export-generator-settings-v2".to_owned(),
        settings_path: "inputs/settings.json".to_owned(),
        settings_media_type: "application/json".to_owned(),
        settings_detected_kind_identity: generator.document().settings_schema_identity.clone(),
        output: OutputDeclaration {
            output_identity: "onshape-export-bambu-project-output-v1".to_owned(),
            role: OutputRole::GeneratedProject,
            path: "outputs/project.3mf".to_owned(),
            media_type: "model/3mf".to_owned(),
            max_byte_length: 134_217_728,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn regular_fixture(package_sha256: String) -> (tempfile::TempDir, DeployedGenerator) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("neutral-generator");
        let bytes = b"neutral executable fixture";
        std::fs::write(&path, bytes).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let document = crate::deployed_generator::DeployedGeneratorDocument {
            executable_path: path,
            package_identity: "neutral-package".to_owned(),
            package_sha256,
            build_identity: "neutral-build".to_owned(),
            binary_identity: "neutral-binary".to_owned(),
            binary_sha256: crate::cache_key::hex_sha256(bytes),
            protocol_version: 1,
            dialect_identity: "neutral-dialect-v1".to_owned(),
            capability_identities: vec!["neutral-capability-v1".to_owned()],
            input_kind_identity: "neutral-input-kind-v1".to_owned(),
            input_schema_identity: "neutral-input-schema-v1".to_owned(),
            settings_schema_identity: "neutral-settings-schema-v2".to_owned(),
            provenance_set_identity: "neutral-provenance-v1".to_owned(),
            normalization_identity: "neutral-normalization-v1".to_owned(),
            validation_identity: "neutral-validation-v1".to_owned(),
        };
        let generator =
            DeployedGenerator::from_bytes(&serde_json::to_vec(&document).unwrap()).unwrap();
        (directory, generator)
    }

    #[test]
    fn absent_deployment_remains_unavailable_without_release_approval() {
        validate_availability(&DeployedGeneratorAvailability::NotConfigured).unwrap();
    }

    #[test]
    fn relative_installation_paths_are_rejected() {
        assert!(verify_root_owned_path(Path::new("generator"), true).is_err());
    }

    #[test]
    fn generic_neutral_deployment_preserves_startup_without_admitting_bambu() {
        let (_directory, generator) = regular_fixture("0".repeat(64));
        let availability = DeployedGeneratorAvailability::Available(Box::new(generator.clone()));
        validate_availability(&availability).unwrap();
        assert!(validate_release(&generator).is_err());
    }

    #[test]
    fn partial_bambu_package_binding_does_not_bypass_startup_admission() {
        let (_directory, generator) = regular_fixture(PACKAGE_SHA256.to_owned());
        let availability = DeployedGeneratorAvailability::Available(Box::new(generator));
        assert!(validate_availability(&availability).is_err());
    }
}
