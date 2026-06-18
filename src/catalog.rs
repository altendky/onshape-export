use std::{
    collections::{HashMap, HashSet},
    fs,
    hash::Hash,
    path::{Path, PathBuf},
};

use anyhow::Context;
use serde::{Deserialize, Serialize};

pub const CATALOG_SCHEMA_VERSION: u32 = 1;
pub const CATALOG_ENTRY_VERSION: u32 = 1;
pub const DEFAULT_PREVIEW_GLB_RESOLUTION: &str = "FINE";
pub const DEFAULT_STEP_VERSION_STRING: &str = "AP242";
pub const DEFAULT_GENERIC_MESH_RESOLUTION: &str = "fine";
pub const DEFAULT_STL_MODE: &str = "BINARY";

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    #[serde(default = "default_catalog_schema_version")]
    pub catalog_schema_version: u32,
    models: Vec<Model>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[serde(default = "default_catalog_schema_version")]
    pub catalog_schema_version: u32,
    #[serde(default = "default_catalog_entry_version")]
    pub entry_version: u32,
    pub slug: String,
    pub name: String,
    pub description: String,
    #[serde(default = "default_published")]
    pub published: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
    pub onshape: OnshapeSource,
    pub exports: ExportConfig,
    pub parameter_policy: ParameterPolicy,
    #[serde(default)]
    pub parameter_presets: Vec<ParameterPreset>,
    #[serde(default)]
    pub parameter_overrides: HashMap<String, ParameterOverride>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParameterOverride {
    pub label: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub hidden: bool,
    pub precision: Option<u32>,
    pub widget: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParameterPreset {
    pub slug: String,
    pub name: String,
    pub values: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnshapeSource {
    pub document_id: String,
    pub version_id: String,
    pub element_id: String,
    pub element_kind: ElementKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_document_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ElementKind {
    PartStudio,
    Assembly,
}

impl ElementKind {
    pub fn key(&self) -> &'static str {
        match self {
            Self::PartStudio => "part_studio",
            Self::Assembly => "assembly",
        }
    }
}

impl OnshapeSource {
    pub fn identity_key(&self) -> String {
        match &self.link_document_id {
            Some(link_document_id) => format!(
                "{}:{}:{}:{}:{}",
                self.element_kind.key(),
                self.document_id,
                self.version_id,
                self.element_id,
                link_document_id
            ),
            None => format!(
                "{}:{}:{}:{}",
                self.element_kind.key(),
                self.document_id,
                self.version_id,
                self.element_id
            ),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExportConfig {
    pub downloads: Vec<DownloadFormat>,
    pub preview: PreviewFormat,
    #[serde(default)]
    pub preview_options: PreviewOptions,
    #[serde(default)]
    pub download_options: DownloadOptions,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewOptions {
    pub resolution: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadOptions {
    pub step_version_string: Option<String>,
    #[serde(default)]
    pub stl: MeshDownloadOptions,
    #[serde(rename = "3mf")]
    #[serde(default)]
    pub three_mf: MeshDownloadOptions,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeshDownloadOptions {
    pub resolution: Option<String>,
    pub stl_mode: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectivePreviewOptions {
    pub resolution: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveStepDownloadOptions {
    pub step_version_string: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveMeshDownloadOptions {
    pub resolution: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stl_mode: Option<String>,
}

impl PreviewOptions {
    pub fn effective_glb(&self) -> EffectivePreviewOptions {
        EffectivePreviewOptions {
            resolution: self
                .resolution
                .clone()
                .unwrap_or_else(|| DEFAULT_PREVIEW_GLB_RESOLUTION.to_owned()),
        }
    }
}

impl DownloadOptions {
    pub fn effective_step(&self) -> EffectiveStepDownloadOptions {
        EffectiveStepDownloadOptions {
            step_version_string: self
                .step_version_string
                .clone()
                .unwrap_or_else(|| DEFAULT_STEP_VERSION_STRING.to_owned()),
        }
    }

    pub fn effective_stl(&self) -> EffectiveMeshDownloadOptions {
        self.stl.effective_with_stl_mode()
    }

    pub fn effective_three_mf(&self) -> EffectiveMeshDownloadOptions {
        self.three_mf.effective_without_stl_mode()
    }
}

impl MeshDownloadOptions {
    fn effective_with_stl_mode(&self) -> EffectiveMeshDownloadOptions {
        EffectiveMeshDownloadOptions {
            resolution: self
                .resolution
                .clone()
                .unwrap_or_else(|| DEFAULT_GENERIC_MESH_RESOLUTION.to_owned()),
            stl_mode: Some(
                self.stl_mode
                    .clone()
                    .unwrap_or_else(|| DEFAULT_STL_MODE.to_owned()),
            ),
        }
    }

    fn effective_without_stl_mode(&self) -> EffectiveMeshDownloadOptions {
        EffectiveMeshDownloadOptions {
            resolution: self
                .resolution
                .clone()
                .unwrap_or_else(|| DEFAULT_GENERIC_MESH_RESOLUTION.to_owned()),
            stl_mode: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DownloadFormat {
    Step,
    Stl,
    #[serde(rename = "3mf")]
    ThreeMf,
}

impl DownloadFormat {
    pub fn from_slug(value: &str) -> Option<Self> {
        match value {
            "step" => Some(Self::Step),
            "stl" => Some(Self::Stl),
            "3mf" => Some(Self::ThreeMf),
            _ => None,
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::Step => "step",
            Self::Stl => "stl",
            Self::ThreeMf => "3mf",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Step => "STEP",
            Self::Stl => "STL",
            Self::ThreeMf => "3MF",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Step => "step",
            Self::Stl => "stl",
            Self::ThreeMf => "3mf",
        }
    }

    pub fn content_type(self) -> &'static str {
        match self {
            Self::Step => "model/step",
            Self::Stl => "model/stl",
            Self::ThreeMf => "model/3mf",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PreviewFormat {
    Glb,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParameterPolicy {
    pub source: ParameterSource,
    pub allow_unknown: bool,
    #[serde(default = "default_auto_refresh")]
    pub auto_refresh: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ParameterSource {
    Onshape,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogFile {
    #[serde(default = "default_catalog_schema_version")]
    catalog_schema_version: u32,
    models: Vec<CatalogModelReference>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum CatalogModelReference {
    Inline(Box<Model>),
    Reference(CatalogIndexModel),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogIndexModel {
    slug: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    model_path: Option<PathBuf>,
}

impl Catalog {
    pub fn from_models(models: Vec<Model>) -> anyhow::Result<Self> {
        let catalog = Self {
            catalog_schema_version: CATALOG_SCHEMA_VERSION,
            models,
        };
        catalog.validate()?;
        Ok(catalog)
    }

    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let contents = fs::read_to_string(path)?;
        let file: CatalogFile = serde_json::from_str(&contents)?;
        anyhow::ensure!(
            file.catalog_schema_version == CATALOG_SCHEMA_VERSION,
            "unsupported catalog schema version: {}",
            file.catalog_schema_version
        );

        let base_path = path.parent().unwrap_or_else(|| Path::new(""));
        let mut models = Vec::with_capacity(file.models.len());
        for reference in file.models {
            models.push(reference.load_model(base_path)?);
        }
        Self::from_models(models)
    }

    pub fn models(&self) -> &[Model] {
        &self.models
    }

    pub fn find(&self, slug: &str) -> Option<&Model> {
        self.models.iter().find(|model| model.slug == slug)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.catalog_schema_version == CATALOG_SCHEMA_VERSION,
            "unsupported catalog schema version: {}",
            self.catalog_schema_version
        );
        let mut slugs = HashSet::new();
        let mut sources = HashMap::new();
        for model in &self.models {
            anyhow::ensure!(
                model.catalog_schema_version == CATALOG_SCHEMA_VERSION,
                "unsupported catalog model schema version for {}: {}",
                model.slug,
                model.catalog_schema_version
            );
            anyhow::ensure!(
                model.entry_version > 0,
                "catalog model entry version must be greater than zero for {}",
                model.slug
            );
            anyhow::ensure!(!model.slug.is_empty(), "catalog model slug cannot be empty");
            anyhow::ensure!(
                is_path_slug(&model.slug),
                "catalog model slug must use lowercase letters, numbers, and hyphens: {}",
                model.slug
            );
            anyhow::ensure!(
                slugs.insert(&model.slug),
                "duplicate catalog model slug: {}",
                model.slug
            );
            anyhow::ensure!(
                !model.name.is_empty(),
                "catalog model name cannot be empty for {}",
                model.slug
            );
            anyhow::ensure!(
                !model.description.is_empty(),
                "catalog model description cannot be empty for {}",
                model.slug
            );
            for tag in &model.tags {
                anyhow::ensure!(
                    is_path_slug(tag),
                    "catalog model tags must use lowercase letters, numbers, and hyphens for {}: {}",
                    model.slug,
                    tag
                );
            }
            if let Some(thumbnail) = &model.thumbnail {
                anyhow::ensure!(
                    !thumbnail.trim().is_empty(),
                    "catalog model thumbnail cannot be empty for {}",
                    model.slug
                );
            }
            anyhow::ensure!(
                !model.onshape.document_id.is_empty(),
                "document id cannot be empty for {}",
                model.slug
            );
            anyhow::ensure!(
                !model.onshape.version_id.is_empty(),
                "version id cannot be empty for {}",
                model.slug
            );
            anyhow::ensure!(
                !model.onshape.element_id.is_empty(),
                "element id cannot be empty for {}",
                model.slug
            );
            if let Some(link_document_id) = &model.onshape.link_document_id {
                anyhow::ensure!(
                    !link_document_id.is_empty(),
                    "link document id cannot be empty for {}",
                    model.slug
                );
            }
            let source_identity = model.onshape.identity_key();
            if let Some(existing_slug) = sources.insert(source_identity.clone(), model.slug.clone())
            {
                anyhow::bail!(
                    "duplicate catalog source identity for {} and {}: {}",
                    existing_slug,
                    model.slug,
                    source_identity
                );
            }
            anyhow::ensure!(
                !model.exports.downloads.is_empty(),
                "at least one download format is required for {}",
                model.slug
            );
            ensure_unique(&model.exports.downloads, || {
                format!("duplicate download format for {}", model.slug)
            })?;
            if let Some(resolution) = &model.exports.preview_options.resolution {
                anyhow::ensure!(
                    matches!(resolution.as_str(), "COARSE" | "MEDIUM" | "FINE"),
                    "preview resolution for {} must be COARSE, MEDIUM, or FINE",
                    model.slug
                );
            }
            if let Some(step_version_string) = &model.exports.download_options.step_version_string {
                anyhow::ensure!(
                    !step_version_string.is_empty(),
                    "STEP version string cannot be empty for {}",
                    model.slug
                );
            }
            validate_mesh_download_options(
                &model.exports.download_options.stl,
                &model.slug,
                "STL",
            )?;
            validate_mesh_download_options(
                &model.exports.download_options.three_mf,
                &model.slug,
                "3MF",
            )?;
            if let Some(stl_mode) = &model.exports.download_options.stl.stl_mode {
                anyhow::ensure!(
                    matches!(stl_mode.as_str(), "BINARY" | "TEXT"),
                    "STL mode for {} must be BINARY or TEXT",
                    model.slug
                );
            }
            anyhow::ensure!(
                model.exports.download_options.three_mf.stl_mode.is_none(),
                "3MF download options for {} cannot set stlMode",
                model.slug
            );
            let mut preset_slugs = HashSet::new();
            for preset in &model.parameter_presets {
                anyhow::ensure!(
                    !preset.slug.is_empty(),
                    "parameter preset slug cannot be empty for {}",
                    model.slug
                );
                anyhow::ensure!(
                    is_path_slug(&preset.slug),
                    "parameter preset slug must use lowercase letters, numbers, and hyphens for {}: {}",
                    model.slug,
                    preset.slug
                );
                anyhow::ensure!(
                    preset.slug != "default",
                    "parameter preset slug cannot be 'default' for {}",
                    model.slug
                );
                anyhow::ensure!(
                    !preset.slug.starts_with("--"),
                    "parameter preset slug cannot start with '--' for {}",
                    model.slug
                );
                anyhow::ensure!(
                    preset_slugs.insert(&preset.slug),
                    "duplicate parameter preset slug for {}: {}",
                    model.slug,
                    preset.slug
                );
                anyhow::ensure!(
                    !preset.name.is_empty(),
                    "parameter preset name cannot be empty for {}:{}",
                    model.slug,
                    preset.slug
                );
            }
            for (parameter_id, override_) in &model.parameter_overrides {
                anyhow::ensure!(
                    !parameter_id.is_empty(),
                    "parameter override id cannot be empty for {}",
                    model.slug
                );
                if let Some(label) = &override_.label {
                    anyhow::ensure!(
                        !label.is_empty(),
                        "parameter override label cannot be empty for {}:{}",
                        model.slug,
                        parameter_id
                    );
                }
                if let Some(precision) = override_.precision {
                    anyhow::ensure!(
                        precision <= 12,
                        "parameter override precision cannot exceed 12 for {}:{}",
                        model.slug,
                        parameter_id
                    );
                }
                if let Some(widget) = &override_.widget {
                    anyhow::ensure!(
                        matches!(widget.as_str(), "number" | "range" | "text" | "textarea"),
                        "unsupported parameter override widget for {}:{}: {}",
                        model.slug,
                        parameter_id,
                        widget
                    );
                }
            }
        }
        Ok(())
    }
}

fn validate_mesh_download_options(
    options: &MeshDownloadOptions,
    model_slug: &str,
    label: &str,
) -> anyhow::Result<()> {
    if let Some(resolution) = &options.resolution {
        anyhow::ensure!(
            matches!(resolution.as_str(), "coarse" | "medium" | "fine"),
            "{label} resolution for {model_slug} must be coarse, medium, or fine"
        );
    }
    Ok(())
}

impl CatalogModelReference {
    fn load_model(self, base_path: &Path) -> anyhow::Result<Model> {
        match self {
            Self::Inline(model) => Ok(*model),
            Self::Reference(index_model) => index_model.load_model(base_path),
        }
    }
}

impl CatalogIndexModel {
    fn load_model(self, base_path: &Path) -> anyhow::Result<Model> {
        anyhow::ensure!(
            is_path_slug(&self.slug),
            "catalog index model slug must use lowercase letters, numbers, and hyphens: {}",
            self.slug
        );
        let model_path = self
            .model_path
            .unwrap_or_else(|| PathBuf::from(format!("models/{}.json", self.slug)));
        let path = base_path.join(model_path);
        let contents = fs::read_to_string(&path)
            .map_err(anyhow::Error::from)
            .with_context(|| format!("loading catalog model {}", path.display()))?;
        let model: Model = serde_json::from_str(&contents)
            .map_err(anyhow::Error::from)
            .with_context(|| format!("parsing catalog model {}", path.display()))?;

        anyhow::ensure!(
            model.slug == self.slug,
            "catalog index slug {} does not match model file slug {}",
            self.slug,
            model.slug
        );
        if let Some(name) = self.name {
            anyhow::ensure!(
                model.name == name,
                "catalog index name does not match model file for {}",
                model.slug
            );
        }
        if let Some(description) = self.description {
            anyhow::ensure!(
                model.description == description,
                "catalog index description does not match model file for {}",
                model.slug
            );
        }

        Ok(model)
    }
}

fn ensure_unique<T, F>(values: &[T], message: F) -> anyhow::Result<()>
where
    T: Copy + Eq + Hash,
    F: Fn() -> String,
{
    let mut seen = HashSet::new();
    for value in values {
        anyhow::ensure!(seen.insert(*value), "{}", message());
    }
    Ok(())
}

fn default_catalog_schema_version() -> u32 {
    CATALOG_SCHEMA_VERSION
}

fn default_catalog_entry_version() -> u32 {
    CATALOG_ENTRY_VERSION
}

fn default_published() -> bool {
    true
}

fn default_auto_refresh() -> bool {
    true
}

fn is_path_slug(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !value.starts_with('-')
        && !value.ends_with('-')
        && !value.contains("--")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn rejects_duplicate_slugs() {
        let catalog = catalog(vec![model("same"), model("same")]);

        assert!(catalog.validate().is_err());
    }

    #[test]
    fn rejects_duplicate_parameter_preset_slugs() {
        let mut model = model("model");
        model.parameter_presets = vec![preset("small"), preset("small")];
        let catalog = catalog(vec![model]);

        assert!(catalog.validate().is_err());
    }

    #[test]
    fn rejects_unsafe_slugs() {
        let catalog = catalog(vec![model("Bad/Slug")]);

        assert!(catalog.validate().is_err());
    }

    #[test]
    fn rejects_duplicate_download_formats() {
        let mut model = model("model");
        model.exports.downloads = vec![DownloadFormat::Step, DownloadFormat::Step];
        let catalog = catalog(vec![model]);

        assert!(catalog.validate().is_err());
    }

    #[test]
    fn rejects_invalid_preview_options() {
        let mut model = model("model");
        model.exports.preview_options.resolution = Some("ULTRA".to_owned());
        let catalog = catalog(vec![model]);

        assert!(catalog.validate().is_err());
    }

    #[test]
    fn accepts_valid_export_mesh_options() {
        let mut model = model("model");
        model.exports.preview_options.resolution = Some("FINE".to_owned());
        model.exports.download_options.stl.resolution = Some("fine".to_owned());
        model.exports.download_options.stl.stl_mode = Some("BINARY".to_owned());
        model.exports.download_options.three_mf.resolution = Some("medium".to_owned());
        let catalog = catalog(vec![model]);

        catalog.validate().unwrap();
    }

    #[test]
    fn rejects_invalid_stl_resolution() {
        let mut model = model("model");
        model.exports.download_options.stl.resolution = Some("FINE".to_owned());
        let catalog = catalog(vec![model]);

        assert!(catalog.validate().is_err());
    }

    #[test]
    fn rejects_invalid_three_mf_resolution() {
        let mut model = model("model");
        model.exports.download_options.three_mf.resolution = Some("FINE".to_owned());
        let catalog = catalog(vec![model]);

        assert!(catalog.validate().is_err());
    }

    #[test]
    fn rejects_invalid_stl_mode() {
        let mut model = model("model");
        model.exports.download_options.stl.stl_mode = Some("ASCII".to_owned());
        let catalog = catalog(vec![model]);

        assert!(catalog.validate().is_err());
    }

    #[test]
    fn deserializes_legacy_download_options_json() {
        let options: DownloadOptions =
            serde_json::from_str(r#"{"stepVersionString":"AP242"}"#).unwrap();

        assert_eq!(
            options.effective_step().step_version_string,
            DEFAULT_STEP_VERSION_STRING
        );
        assert_eq!(
            options.effective_stl().resolution,
            DEFAULT_GENERIC_MESH_RESOLUTION
        );
        assert_eq!(
            options.effective_stl().stl_mode.as_deref(),
            Some(DEFAULT_STL_MODE)
        );
        assert_eq!(
            options.effective_three_mf().resolution,
            DEFAULT_GENERIC_MESH_RESOLUTION
        );
        assert!(options.effective_three_mf().stl_mode.is_none());
    }

    #[test]
    fn rejects_invalid_parameter_overrides() {
        let mut model = model("model");
        model.parameter_overrides.insert(
            "size".to_owned(),
            ParameterOverride {
                precision: Some(13),
                ..Default::default()
            },
        );
        let catalog = catalog(vec![model]);

        assert!(catalog.validate().is_err());
    }

    #[test]
    fn rejects_unsupported_schema_versions() {
        let mut catalog = catalog(vec![model("model")]);
        catalog.catalog_schema_version = 2;

        assert!(catalog.validate().is_err());
    }

    #[test]
    fn rejects_duplicate_source_identities() {
        let mut first = model("first");
        let mut second = model("second");
        first.onshape.document_id = "same-did".to_owned();
        second.onshape.document_id = "same-did".to_owned();

        let catalog = catalog(vec![first, second]);

        assert!(catalog.validate().is_err());
    }

    #[test]
    fn loads_v1_split_catalog_layout() {
        let directory = tempfile::tempdir().unwrap();
        let models_directory = directory.path().join("models");
        fs::create_dir(&models_directory).unwrap();
        fs::write(
            directory.path().join("models.json"),
            r#"{
              "catalogSchemaVersion": 1,
              "models": [
                {"slug": "demo", "name": "Demo", "description": "Demo model"}
              ]
            }"#,
        )
        .unwrap();
        fs::write(
            models_directory.join("demo.json"),
            r#"{
              "catalogSchemaVersion": 1,
              "entryVersion": 1,
              "slug": "demo",
              "name": "Demo",
              "description": "Demo model",
              "onshape": {
                "documentId": "did",
                "versionId": "vid",
                "elementId": "eid",
                "elementKind": "part_studio"
              },
              "exports": {
                "downloads": ["step"],
                "preview": "glb"
              },
              "parameterPolicy": {
                "source": "onshape",
                "allowUnknown": false
              }
            }"#,
        )
        .unwrap();

        let catalog = Catalog::load(directory.path().join("models.json")).unwrap();

        assert_eq!(catalog.models()[0].slug, "demo");
        assert_eq!(catalog.models()[0].entry_version, 1);
    }

    fn catalog(models: Vec<Model>) -> Catalog {
        Catalog {
            catalog_schema_version: CATALOG_SCHEMA_VERSION,
            models,
        }
    }

    fn preset(slug: &str) -> ParameterPreset {
        ParameterPreset {
            slug: slug.to_owned(),
            name: "Name".to_owned(),
            values: HashMap::new(),
        }
    }

    fn model(slug: &str) -> Model {
        Model {
            catalog_schema_version: CATALOG_SCHEMA_VERSION,
            entry_version: CATALOG_ENTRY_VERSION,
            slug: slug.to_owned(),
            name: "Name".to_owned(),
            description: "Description".to_owned(),
            published: true,
            tags: Vec::new(),
            thumbnail: None,
            onshape: OnshapeSource {
                document_id: "did".to_owned(),
                version_id: "vid".to_owned(),
                element_id: "eid".to_owned(),
                element_kind: ElementKind::PartStudio,
                link_document_id: None,
            },
            exports: ExportConfig {
                downloads: vec![
                    DownloadFormat::Step,
                    DownloadFormat::Stl,
                    DownloadFormat::ThreeMf,
                ],
                preview: PreviewFormat::Glb,
                preview_options: PreviewOptions::default(),
                download_options: DownloadOptions::default(),
            },
            parameter_policy: ParameterPolicy {
                source: ParameterSource::Onshape,
                allow_unknown: false,
                auto_refresh: true,
            },
            parameter_presets: Vec::new(),
            parameter_overrides: HashMap::new(),
        }
    }
}
