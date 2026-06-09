use std::{collections::HashSet, fs, path::Path};

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
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DownloadFormat {
    Step,
    Stl,
    #[serde(rename = "3mf")]
    ThreeMf,
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
        }
        Ok(())
    }
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
            },
            parameter_policy: ParameterPolicy {
                source: ParameterSource::Onshape,
                allow_unknown: false,
            },
        }
    }
}
