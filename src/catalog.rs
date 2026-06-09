use std::{
    collections::{HashMap, HashSet},
    fs,
    hash::Hash,
    path::Path,
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Catalog {
    models: Vec<Model>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub slug: String,
    pub name: String,
    pub description: String,
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
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ElementKind {
    PartStudio,
    Assembly,
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
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ParameterSource {
    Onshape,
}

impl Catalog {
    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let contents = fs::read_to_string(path)?;
        let catalog: Self = serde_json::from_str(&contents)?;
        catalog.validate()?;
        Ok(catalog)
    }

    pub fn models(&self) -> &[Model] {
        &self.models
    }

    pub fn find(&self, slug: &str) -> Option<&Model> {
        self.models.iter().find(|model| model.slug == slug)
    }

    fn validate(&self) -> anyhow::Result<()> {
        let mut slugs = HashSet::new();
        for model in &self.models {
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
    use super::*;

    #[test]
    fn rejects_duplicate_slugs() {
        let catalog = Catalog {
            models: vec![model("same"), model("same")],
        };

        assert!(catalog.validate().is_err());
    }

    #[test]
    fn rejects_duplicate_parameter_preset_slugs() {
        let mut model = model("model");
        model.parameter_presets = vec![preset("small"), preset("small")];
        let catalog = Catalog {
            models: vec![model],
        };

        assert!(catalog.validate().is_err());
    }

    #[test]
    fn rejects_unsafe_slugs() {
        let catalog = Catalog {
            models: vec![model("Bad/Slug")],
        };

        assert!(catalog.validate().is_err());
    }

    #[test]
    fn rejects_duplicate_download_formats() {
        let mut model = model("model");
        model.exports.downloads = vec![DownloadFormat::Step, DownloadFormat::Step];
        let catalog = Catalog {
            models: vec![model],
        };

        assert!(catalog.validate().is_err());
    }

    #[test]
    fn rejects_invalid_preview_options() {
        let mut model = model("model");
        model.exports.preview_options.resolution = Some("ULTRA".to_owned());
        let catalog = Catalog {
            models: vec![model],
        };

        assert!(catalog.validate().is_err());
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
        let catalog = Catalog {
            models: vec![model],
        };

        assert!(catalog.validate().is_err());
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
            slug: slug.to_owned(),
            name: "Name".to_owned(),
            description: "Description".to_owned(),
            onshape: OnshapeSource {
                document_id: "did".to_owned(),
                version_id: "vid".to_owned(),
                element_id: "eid".to_owned(),
                element_kind: ElementKind::PartStudio,
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
            },
            parameter_presets: Vec::new(),
            parameter_overrides: HashMap::new(),
        }
    }
}
