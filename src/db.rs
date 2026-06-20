use std::{collections::HashMap, path::Path};

use anyhow::Context;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sqlx::{Executor, Row, Sqlite, SqlitePool, sqlite::SqlitePoolOptions};

use crate::catalog::{
    Catalog, ExportConfig, Model, OnshapeSource, ParameterOverride, ParameterPolicy,
    ParameterPreset, ParameterSource, PreviewFormat,
};

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ParameterMetadataRecord {
    pub source_hash: String,
    pub raw_object_key: String,
    pub normalized_object_key: String,
    pub schema_hash: String,
    pub schema_version: i64,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct SourceResolutionRecord {
    pub source_hash: String,
    pub model_slug: String,
    pub document_id: String,
    pub version_id: String,
    pub microversion_id: String,
    pub element_id: String,
    pub element_kind: String,
    pub link_document_id: Option<String>,
    pub diagnostics_json: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy)]
pub struct SourceResolutionUpsert<'a> {
    pub source_hash: &'a str,
    pub model_slug: &'a str,
    pub document_id: &'a str,
    pub version_id: &'a str,
    pub microversion_id: &'a str,
    pub element_id: &'a str,
    pub element_kind: &'a str,
    pub link_document_id: Option<&'a str>,
    pub diagnostics_json: &'a str,
}

#[allow(dead_code)]
#[cfg(test)]
#[derive(Debug, Clone)]
pub struct ConfigurationSelectionRecord {
    pub source_hash: String,
    pub config_hash: String,
    pub values_json: String,
    pub validation_json: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy)]
pub struct ConfigurationSelectionUpsert<'a> {
    pub source_hash: &'a str,
    pub config_hash: &'a str,
    pub values_json: &'a str,
    pub validation_json: &'a str,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ConfigurationEncodingRecord {
    pub source_hash: String,
    pub config_hash: String,
    pub encoded_id: String,
    pub query_param: String,
    pub request_json: String,
    pub response_json: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy)]
pub struct ConfigurationEncodingUpsert<'a> {
    pub source_hash: &'a str,
    pub config_hash: &'a str,
    pub encoded_id: &'a str,
    pub query_param: &'a str,
    pub request_json: &'a str,
    pub response_json: &'a str,
}

#[allow(dead_code)]
#[cfg(test)]
#[derive(Debug, Clone)]
pub struct ExportRequestRecord {
    pub request_hash: String,
    pub source_hash: String,
    pub config_hash: String,
    pub options_hash: String,
    pub output_kind: String,
    pub format: String,
    pub endpoint: String,
    pub method: String,
    pub path: String,
    pub request_json: String,
    pub defaults_policy_version: String,
    pub request_builder_version: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy)]
pub struct ExportRequestInsert<'a> {
    pub request_hash: &'a str,
    pub source_hash: &'a str,
    pub config_hash: &'a str,
    pub options_hash: &'a str,
    pub output_kind: &'a str,
    pub format: &'a str,
    pub endpoint: &'a str,
    pub method: &'a str,
    pub path: &'a str,
    pub request_json: &'a str,
    pub defaults_policy_version: &'a str,
    pub request_builder_version: &'a str,
    pub status: &'a str,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct TranslationRecord {
    pub translation_id: String,
    pub request_hash: String,
    pub state: String,
    pub start_response_json: Option<String>,
    pub final_response_json: Option<String>,
    pub poll_state_json: Option<String>,
    pub result_external_data_ids_json: Option<String>,
    pub result_element_ids_json: Option<String>,
    pub response_hash: Option<String>,
    pub failure_reason: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy)]
pub struct TranslationStartInsert<'a> {
    pub translation_id: &'a str,
    pub request_hash: &'a str,
    pub state: &'a str,
    pub start_response_json: &'a str,
}

#[derive(Debug, Clone, Copy)]
pub struct TranslationFinalUpdate<'a> {
    pub translation_id: &'a str,
    pub state: &'a str,
    pub final_response_json: &'a str,
    pub poll_state_json: &'a str,
    pub result_external_data_ids_json: &'a str,
    pub result_element_ids_json: &'a str,
    pub response_hash: Option<&'a str>,
    pub failure_reason: Option<&'a str>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct RawPayloadRecord {
    pub raw_payload_hash: String,
    pub object_key: String,
    pub content_type: Option<String>,
    pub byte_len: i64,
    pub headers_json: String,
    pub original_filename: Option<String>,
    pub filename_source: Option<String>,
    pub detected_kind: String,
    pub zip_manifest_json: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy)]
pub struct RawPayloadInsert<'a> {
    pub raw_payload_hash: &'a str,
    pub object_key: &'a str,
    pub content_type: Option<&'a str>,
    pub byte_len: i64,
    pub headers_json: &'a str,
    pub original_filename: Option<&'a str>,
    pub filename_source: Option<&'a str>,
    pub detected_kind: &'a str,
    pub zip_manifest_json: Option<&'a str>,
}

#[derive(Debug, Clone, Copy)]
pub struct RawPayloadSourceInsert<'a> {
    pub request_hash: &'a str,
    pub translation_id: Option<&'a str>,
    pub external_data_id: Option<&'a str>,
    pub result_index: Option<i64>,
    pub response_headers_json: &'a str,
    pub etag: Option<&'a str>,
    pub raw_payload_hash: &'a str,
}

#[allow(dead_code)]
#[cfg(test)]
#[derive(Debug, Clone)]
pub struct PostprocessRunRecord {
    pub postprocess_hash: String,
    pub raw_payload_hash: String,
    pub processor_name: String,
    pub processor_version: String,
    pub policy_json: String,
    pub status: String,
    pub log_json: String,
    pub derived_files_json: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy)]
pub struct PostprocessRunInsert<'a> {
    pub postprocess_hash: &'a str,
    pub raw_payload_hash: &'a str,
    pub processor_name: &'a str,
    pub processor_version: &'a str,
    pub policy_json: &'a str,
    pub status: &'a str,
    pub log_json: &'a str,
    pub derived_files_json: &'a str,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ArtifactSetRecord {
    pub artifact_set_hash: String,
    pub source_hash: String,
    pub config_hash: String,
    pub options_hash: String,
    pub request_hash: Option<String>,
    pub raw_payload_hash: Option<String>,
    pub postprocess_hash: Option<String>,
    pub output_kind: String,
    pub format: String,
    pub status: String,
    pub primary_object_key: Option<String>,
    pub metadata_json: String,
    pub created_at: String,
    pub updated_at: String,
    pub superseded_at: Option<String>,
    pub superseded_by: Option<String>,
    pub supersession_reason: Option<String>,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub struct ArtifactSetInsert<'a> {
    pub artifact_set_hash: &'a str,
    pub source_hash: &'a str,
    pub config_hash: &'a str,
    pub options_hash: &'a str,
    pub request_hash: Option<&'a str>,
    pub raw_payload_hash: Option<&'a str>,
    pub postprocess_hash: Option<&'a str>,
    pub output_kind: &'a str,
    pub format: &'a str,
    pub status: &'a str,
    pub primary_object_key: Option<&'a str>,
    pub metadata_json: &'a str,
}

#[derive(Debug, Clone, Copy)]
pub struct ArtifactFileInsert<'a> {
    pub artifact_set_hash: &'a str,
    pub role: &'a str,
    pub logical_path: &'a str,
    pub original_path: Option<&'a str>,
    pub object_key: &'a str,
    pub content_type: &'a str,
    pub byte_len: i64,
    pub sha256: &'a str,
    pub metadata_json: &'a str,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArtifactRecord {
    pub artifact_key: String,
    pub model_slug: String,
    pub config_hash: String,
    pub output_kind: String,
    pub status: String,
    pub object_key: String,
    pub content_type: String,
    pub byte_len: Option<i64>,
    pub sha256: Option<String>,
    pub producing_job_key: Option<String>,
    pub source_hash: Option<String>,
    pub options_hash: Option<String>,
    pub parameter_schema_version: Option<i64>,
    pub config_values_json: Option<String>,
    pub created_at: String,
    pub superseded_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct JobRecord {
    pub work_key: String,
    pub job_kind: String,
    pub status: String,
    pub error_summary: Option<String>,
    pub attempt: i64,
    pub max_attempts: i64,
    pub next_retry_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct JobLease {
    pub work_key: String,
    pub job_kind: String,
    pub payload_json: String,
    pub attempt: i64,
    pub max_attempts: i64,
}

#[derive(Debug, Clone)]
pub struct JobMetric {
    pub job_kind: String,
    pub status: String,
    pub count: i64,
}

#[derive(Debug, Clone)]
pub struct ArtifactMetric {
    pub output_kind: String,
    pub count: i64,
    pub byte_len: i64,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub struct ArtifactUpsert<'a> {
    pub artifact_key: &'a str,
    pub model_slug: &'a str,
    pub config_hash: &'a str,
    pub output_kind: &'a str,
    pub format: &'a str,
    pub object_key: &'a str,
    pub content_type: &'a str,
    pub byte_len: i64,
    pub sha256: &'a str,
    pub producing_job_key: Option<&'a str>,
    pub source_hash: &'a str,
    pub options_hash: &'a str,
    pub request_hash: Option<&'a str>,
    pub raw_payload_hash: Option<&'a str>,
    pub postprocess_hash: Option<&'a str>,
    pub parameter_schema_version: i64,
    pub config_values_json: &'a str,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ArtifactMetadata {
    model_slug: String,
    #[serde(default)]
    producing_job_key: Option<String>,
    #[serde(default)]
    parameter_schema_version: Option<i64>,
    #[serde(default)]
    config_values_json: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Database {
    pool: SqlitePool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeletedTableRows {
    pub table: &'static str,
    pub rows: u64,
}

impl Database {
    pub async fn connect(database_url: &str) -> sqlx::Result<Self> {
        let db = Self::connect_without_migrations(database_url).await?;
        sqlx::migrate!().run(&db.pool).await?;

        Ok(db)
    }

    pub async fn connect_without_migrations(database_url: &str) -> sqlx::Result<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await?;

        Self::apply_pragmas(&pool).await?;
        Ok(Self { pool })
    }

    pub async fn ping(&self) -> sqlx::Result<()> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }

    pub async fn backup_to_path(&self, path: &Path) -> anyhow::Result<()> {
        anyhow::ensure!(
            !path.exists(),
            "backup destination already exists: {}",
            path.display()
        );
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            anyhow::ensure!(
                parent.exists(),
                "backup destination parent does not exist: {}",
                parent.display()
            );
        }

        sqlx::query("VACUUM main INTO ?")
            .bind(path.to_string_lossy().as_ref())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn clear_generated_state(&self) -> sqlx::Result<Vec<DeletedTableRows>> {
        const GENERATED_TABLES: &[&str] = &[
            "artifact_files",
            "artifact_sets",
            "postprocess_runs",
            "raw_payload_sources",
            "raw_payloads",
            "translations",
            "export_requests",
            "configuration_encodings",
            "configuration_selections",
            "parameter_metadata",
            "source_resolution_aliases",
            "source_resolutions",
            "artifacts",
            "jobs",
        ];

        let mut tx = self.pool.begin().await?;
        let mut deleted = Vec::with_capacity(GENERATED_TABLES.len());
        for &table in GENERATED_TABLES {
            let sql = format!("DELETE FROM {table}");
            // Table names come only from GENERATED_TABLES above; SQLite cannot bind identifiers.
            let result = sqlx::query(sqlx::AssertSqlSafe(sql))
                .execute(&mut *tx)
                .await?;
            deleted.push(DeletedTableRows {
                table,
                rows: result.rows_affected(),
            });
        }
        tx.commit().await?;
        Ok(deleted)
    }

    pub async fn catalog(&self) -> anyhow::Result<Catalog> {
        let mut tx = self.pool.begin().await?;
        let rows = sqlx::query(
            r#"
            SELECT id, catalog_schema_version, entry_version, slug, name, description,
                   published, tags_json, thumbnail, document_id, version_id, element_id,
                   element_kind, link_document_id, downloads_json, preview_format,
                   preview_options_json, download_options_json, parameter_source,
                   parameter_allow_unknown, parameter_auto_refresh
            FROM catalog_models
            ORDER BY display_order, slug
            "#,
        )
        .fetch_all(&mut *tx)
        .await?;

        let mut models = Vec::with_capacity(rows.len());
        for row in rows {
            models.push(Self::catalog_model_from_row(&mut tx, row).await?);
        }

        let catalog = Catalog::from_models(models)?;
        tx.commit().await?;
        Ok(catalog)
    }

    pub async fn published_catalog_model(&self, slug: &str) -> anyhow::Result<Option<Model>> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            r#"
            SELECT id, catalog_schema_version, entry_version, slug, name, description,
                   published, tags_json, thumbnail, document_id, version_id, element_id,
                   element_kind, link_document_id, downloads_json, preview_format,
                   preview_options_json, download_options_json, parameter_source,
                   parameter_allow_unknown, parameter_auto_refresh
            FROM catalog_models
            WHERE slug = ? AND published = 1
            "#,
        )
        .bind(slug)
        .fetch_optional(&mut *tx)
        .await?;

        let model = match row {
            Some(row) => {
                let model = Self::catalog_model_from_row(&mut tx, row).await?;
                let catalog = Catalog::from_models(vec![model])?;
                Some(catalog.models()[0].clone())
            }
            None => None,
        };

        tx.commit().await?;
        Ok(model)
    }

    pub async fn replace_catalog(&self, catalog: &Catalog) -> anyhow::Result<()> {
        catalog.validate()?;

        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM catalog_parameter_preset_values")
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM catalog_parameter_presets")
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM catalog_parameter_overrides")
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM catalog_models")
            .execute(&mut *tx)
            .await?;

        for (display_order, model) in catalog.models().iter().enumerate() {
            let tags_json = serde_json::to_string(&model.tags)?;
            let downloads_json = serde_json::to_string(&model.exports.downloads)?;
            let preview_options_json = serde_json::to_string(&model.exports.preview_options)?;
            let download_options_json = serde_json::to_string(&model.exports.download_options)?;
            let result = sqlx::query(
                r#"
                INSERT INTO catalog_models (
                    display_order, catalog_schema_version, entry_version, slug, name,
                    description, published, tags_json, thumbnail, document_id, version_id,
                    element_id, element_kind, link_document_id, downloads_json,
                    preview_format, preview_options_json, download_options_json,
                    parameter_source, parameter_allow_unknown, parameter_auto_refresh
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(i64::try_from(display_order).context("catalog display order overflow")?)
            .bind(i64::from(model.catalog_schema_version))
            .bind(i64::from(model.entry_version))
            .bind(&model.slug)
            .bind(&model.name)
            .bind(&model.description)
            .bind(bool_to_i64(model.published))
            .bind(tags_json)
            .bind(&model.thumbnail)
            .bind(&model.onshape.document_id)
            .bind(&model.onshape.version_id)
            .bind(&model.onshape.element_id)
            .bind(model.onshape.element_kind.key())
            .bind(&model.onshape.link_document_id)
            .bind(downloads_json)
            .bind(preview_format_key(&model.exports.preview))
            .bind(preview_options_json)
            .bind(download_options_json)
            .bind(parameter_source_key(&model.parameter_policy.source))
            .bind(bool_to_i64(model.parameter_policy.allow_unknown))
            .bind(bool_to_i64(model.parameter_policy.auto_refresh))
            .execute(&mut *tx)
            .await?;
            let model_id = result.last_insert_rowid();

            for (parameter_id, override_) in &model.parameter_overrides {
                sqlx::query(
                    r#"
                    INSERT INTO catalog_parameter_overrides (
                        model_id, parameter_id, label, description, hidden, precision, widget
                    )
                    VALUES (?, ?, ?, ?, ?, ?, ?)
                    "#,
                )
                .bind(model_id)
                .bind(parameter_id)
                .bind(&override_.label)
                .bind(&override_.description)
                .bind(bool_to_i64(override_.hidden))
                .bind(override_.precision.map(i64::from))
                .bind(&override_.widget)
                .execute(&mut *tx)
                .await?;
            }

            for (preset_order, preset) in model.parameter_presets.iter().enumerate() {
                let result = sqlx::query(
                    r#"
                    INSERT INTO catalog_parameter_presets (model_id, display_order, slug, name)
                    VALUES (?, ?, ?, ?)
                    "#,
                )
                .bind(model_id)
                .bind(i64::try_from(preset_order).context("catalog preset order overflow")?)
                .bind(&preset.slug)
                .bind(&preset.name)
                .execute(&mut *tx)
                .await?;
                let preset_id = result.last_insert_rowid();

                for (parameter_id, value) in &preset.values {
                    sqlx::query(
                        r#"
                        INSERT INTO catalog_parameter_preset_values (preset_id, parameter_id, value)
                        VALUES (?, ?, ?)
                        "#,
                    )
                    .bind(preset_id)
                    .bind(parameter_id)
                    .bind(value)
                    .execute(&mut *tx)
                    .await?;
                }
            }
        }

        tx.commit().await?;
        Ok(())
    }

    async fn catalog_parameter_overrides(
        tx: &mut sqlx::Transaction<'_, Sqlite>,
        model_id: i64,
        model_slug: &str,
    ) -> anyhow::Result<HashMap<String, ParameterOverride>> {
        let rows = sqlx::query(
            r#"
            SELECT parameter_id, label, description, hidden, precision, widget
            FROM catalog_parameter_overrides
            WHERE model_id = ?
            ORDER BY parameter_id
            "#,
        )
        .bind(model_id)
        .fetch_all(&mut **tx)
        .await?;

        let mut overrides = HashMap::with_capacity(rows.len());
        for row in rows {
            let parameter_id: String = row.get("parameter_id");
            let precision = row
                .get::<Option<i64>, _>("precision")
                .map(|value| {
                    u32::try_from(value).with_context(|| {
                        format!("invalid override precision for {model_slug}:{parameter_id}")
                    })
                })
                .transpose()?;
            overrides.insert(
                parameter_id,
                ParameterOverride {
                    label: row.get("label"),
                    description: row.get("description"),
                    hidden: bool_column(&row, "hidden"),
                    precision,
                    widget: row.get("widget"),
                },
            );
        }

        Ok(overrides)
    }

    async fn catalog_model_from_row(
        tx: &mut sqlx::Transaction<'_, Sqlite>,
        row: sqlx::sqlite::SqliteRow,
    ) -> anyhow::Result<Model> {
        let model_id: i64 = row.get("id");
        let slug: String = row.get("slug");
        let parameter_overrides = Self::catalog_parameter_overrides(tx, model_id, &slug)
            .await
            .with_context(|| format!("loading catalog parameter overrides for {slug}"))?;
        let parameter_presets = Self::catalog_parameter_presets(tx, model_id, &slug)
            .await
            .with_context(|| format!("loading catalog parameter presets for {slug}"))?;

        Ok(Model {
            catalog_schema_version: u32_column(&row, "catalog_schema_version")?,
            entry_version: u32_column(&row, "entry_version")?,
            slug,
            name: row.get("name"),
            description: row.get("description"),
            published: bool_column(&row, "published"),
            tags: json_column(&row, "tags_json")?,
            thumbnail: row.get("thumbnail"),
            onshape: OnshapeSource {
                document_id: row.get("document_id"),
                version_id: row.get("version_id"),
                element_id: row.get("element_id"),
                element_kind: enum_text_column(&row, "element_kind")?,
                link_document_id: row.get("link_document_id"),
            },
            exports: ExportConfig {
                downloads: json_column(&row, "downloads_json")?,
                preview: enum_text_column(&row, "preview_format")?,
                preview_options: json_column(&row, "preview_options_json")?,
                download_options: json_column(&row, "download_options_json")?,
            },
            parameter_policy: ParameterPolicy {
                source: enum_text_column(&row, "parameter_source")?,
                allow_unknown: bool_column(&row, "parameter_allow_unknown"),
                auto_refresh: bool_column(&row, "parameter_auto_refresh"),
            },
            parameter_presets,
            parameter_overrides,
        })
    }

    async fn catalog_parameter_presets(
        tx: &mut sqlx::Transaction<'_, Sqlite>,
        model_id: i64,
        model_slug: &str,
    ) -> anyhow::Result<Vec<ParameterPreset>> {
        let rows = sqlx::query(
            r#"
            SELECT id, slug, name
            FROM catalog_parameter_presets
            WHERE model_id = ?
            ORDER BY display_order, slug
            "#,
        )
        .bind(model_id)
        .fetch_all(&mut **tx)
        .await?;

        let mut presets = Vec::with_capacity(rows.len());
        for row in rows {
            let preset_id: i64 = row.get("id");
            let slug: String = row.get("slug");
            let value_rows = sqlx::query(
                r#"
                SELECT parameter_id, value
                FROM catalog_parameter_preset_values
                WHERE preset_id = ?
                ORDER BY parameter_id
                "#,
            )
            .bind(preset_id)
            .fetch_all(&mut **tx)
            .await
            .with_context(|| format!("loading catalog preset values for {model_slug}:{slug}"))?;

            let mut values = HashMap::with_capacity(value_rows.len());
            for value_row in value_rows {
                values.insert(value_row.get("parameter_id"), value_row.get("value"));
            }

            presets.push(ParameterPreset {
                slug,
                name: row.get("name"),
                values,
            });
        }

        Ok(presets)
    }

    #[cfg(test)]
    pub async fn source_resolution(
        &self,
        source_hash: &str,
    ) -> sqlx::Result<Option<SourceResolutionRecord>> {
        sqlx::query(
            r#"
            SELECT source_hash, model_slug, document_id, version_id, microversion_id,
                   element_id, element_kind, link_document_id, diagnostics_json,
                   created_at, updated_at
            FROM source_resolutions
            WHERE source_hash = ?
            "#,
        )
        .bind(source_hash)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(source_resolution_record_from_row))
    }

    pub async fn source_resolution_for_version(
        &self,
        document_id: &str,
        version_id: &str,
        element_id: &str,
        element_kind: &str,
        link_document_id: Option<&str>,
    ) -> sqlx::Result<Option<SourceResolutionRecord>> {
        sqlx::query(
            r#"
            SELECT source_resolutions.source_hash AS source_hash,
                   source_resolutions.model_slug AS model_slug,
                   source_resolution_aliases.document_id AS document_id,
                   source_resolution_aliases.version_id AS version_id,
                   source_resolutions.microversion_id AS microversion_id,
                   source_resolution_aliases.element_id AS element_id,
                   source_resolution_aliases.element_kind AS element_kind,
                   source_resolution_aliases.link_document_id AS link_document_id,
                   source_resolutions.diagnostics_json AS diagnostics_json,
                   source_resolutions.created_at AS created_at,
                   source_resolutions.updated_at AS updated_at
            FROM source_resolution_aliases
            JOIN source_resolutions USING (source_hash)
            WHERE source_resolution_aliases.document_id = ?
              AND source_resolution_aliases.version_id = ?
              AND source_resolution_aliases.element_id = ?
              AND source_resolution_aliases.element_kind = ?
              AND ifnull(source_resolution_aliases.link_document_id, '') = ifnull(?, '')
            "#,
        )
        .bind(document_id)
        .bind(version_id)
        .bind(element_id)
        .bind(element_kind)
        .bind(link_document_id)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(source_resolution_record_from_row))
    }

    pub async fn upsert_source_resolution(
        &self,
        resolution: SourceResolutionUpsert<'_>,
    ) -> sqlx::Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            r#"
            INSERT INTO source_resolutions (
                source_hash, model_slug, document_id, version_id, microversion_id,
                element_id, element_kind, link_document_id, diagnostics_json
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(source_hash) DO UPDATE SET
                model_slug = excluded.model_slug,
                document_id = excluded.document_id,
                microversion_id = excluded.microversion_id,
                element_id = excluded.element_id,
                element_kind = excluded.element_kind,
                link_document_id = excluded.link_document_id,
                diagnostics_json = excluded.diagnostics_json,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            "#,
        )
        .bind(resolution.source_hash)
        .bind(resolution.model_slug)
        .bind(resolution.document_id)
        .bind(resolution.version_id)
        .bind(resolution.microversion_id)
        .bind(resolution.element_id)
        .bind(resolution.element_kind)
        .bind(resolution.link_document_id)
        .bind(resolution.diagnostics_json)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO source_resolution_aliases (
                document_id, version_id, element_id, element_kind, link_document_id, source_hash
            )
            VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT DO UPDATE SET
                source_hash = excluded.source_hash,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            "#,
        )
        .bind(resolution.document_id)
        .bind(resolution.version_id)
        .bind(resolution.element_id)
        .bind(resolution.element_kind)
        .bind(resolution.link_document_id)
        .bind(resolution.source_hash)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn parameter_metadata(
        &self,
        source_hash: &str,
    ) -> sqlx::Result<Option<ParameterMetadataRecord>> {
        sqlx::query(
            "SELECT source_hash, raw_object_key, normalized_object_key, schema_hash, schema_version FROM parameter_metadata WHERE source_hash = ?",
        )
            .bind(source_hash)
            .fetch_optional(&self.pool)
            .await
            .map(|row| {
                row.map(|row| ParameterMetadataRecord {
                    source_hash: row.get("source_hash"),
                    raw_object_key: row.get("raw_object_key"),
                    normalized_object_key: row.get("normalized_object_key"),
                    schema_hash: row.get("schema_hash"),
                    schema_version: row.get("schema_version"),
                })
            })
    }

    pub async fn upsert_parameter_metadata(
        &self,
        source_hash: &str,
        raw_object_key: &str,
        normalized_object_key: &str,
        schema_hash: &str,
        schema_version: i64,
    ) -> sqlx::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO parameter_metadata (
                source_hash,
                raw_object_key,
                normalized_object_key,
                schema_hash,
                schema_version
            )
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(source_hash) DO UPDATE SET
                raw_object_key = excluded.raw_object_key,
                normalized_object_key = excluded.normalized_object_key,
                schema_hash = excluded.schema_hash,
                schema_version = excluded.schema_version,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            "#,
        )
        .bind(source_hash)
        .bind(raw_object_key)
        .bind(normalized_object_key)
        .bind(schema_hash)
        .bind(schema_version)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    #[cfg(test)]
    pub async fn configuration_selection(
        &self,
        source_hash: &str,
        config_hash: &str,
    ) -> sqlx::Result<Option<ConfigurationSelectionRecord>> {
        sqlx::query(
            r#"
            SELECT source_hash, config_hash, values_json, validation_json, created_at, updated_at
            FROM configuration_selections
            WHERE source_hash = ? AND config_hash = ?
            "#,
        )
        .bind(source_hash)
        .bind(config_hash)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(configuration_selection_record_from_row))
    }

    pub async fn upsert_configuration_selection(
        &self,
        selection: ConfigurationSelectionUpsert<'_>,
    ) -> sqlx::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO configuration_selections (
                source_hash, config_hash, values_json, validation_json
            )
            VALUES (?, ?, ?, ?)
            ON CONFLICT(source_hash, config_hash) DO UPDATE SET
                values_json = excluded.values_json,
                validation_json = excluded.validation_json,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            "#,
        )
        .bind(selection.source_hash)
        .bind(selection.config_hash)
        .bind(selection.values_json)
        .bind(selection.validation_json)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn configuration_encoding(
        &self,
        source_hash: &str,
        config_hash: &str,
    ) -> sqlx::Result<Option<ConfigurationEncodingRecord>> {
        sqlx::query(
            r#"
            SELECT source_hash, config_hash, encoded_id, query_param, request_json,
                   response_json, created_at, updated_at
            FROM configuration_encodings
            WHERE source_hash = ? AND config_hash = ?
            "#,
        )
        .bind(source_hash)
        .bind(config_hash)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(configuration_encoding_record_from_row))
    }

    pub async fn upsert_configuration_encoding(
        &self,
        encoding: ConfigurationEncodingUpsert<'_>,
    ) -> sqlx::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO configuration_encodings (
                source_hash, config_hash, encoded_id, query_param, request_json, response_json
            )
            VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT(source_hash, config_hash) DO UPDATE SET
                encoded_id = excluded.encoded_id,
                query_param = excluded.query_param,
                request_json = excluded.request_json,
                response_json = excluded.response_json,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            "#,
        )
        .bind(encoding.source_hash)
        .bind(encoding.config_hash)
        .bind(encoding.encoded_id)
        .bind(encoding.query_param)
        .bind(encoding.request_json)
        .bind(encoding.response_json)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    #[cfg(test)]
    pub async fn export_request(
        &self,
        request_hash: &str,
    ) -> sqlx::Result<Option<ExportRequestRecord>> {
        sqlx::query(
            r#"
            SELECT request_hash, source_hash, config_hash, options_hash, output_kind, format,
                   endpoint, method, path, request_json, defaults_policy_version,
                   request_builder_version, status, created_at, updated_at
            FROM export_requests
            WHERE request_hash = ?
            "#,
        )
        .bind(request_hash)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(export_request_record_from_row))
    }

    pub async fn insert_export_request_if_absent(
        &self,
        request: ExportRequestInsert<'_>,
    ) -> sqlx::Result<bool> {
        let result = sqlx::query(
            r#"
            INSERT INTO export_requests (
                request_hash, source_hash, config_hash, options_hash, output_kind, format,
                endpoint, method, path, request_json, defaults_policy_version,
                request_builder_version, status
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(request_hash) DO NOTHING
            "#,
        )
        .bind(request.request_hash)
        .bind(request.source_hash)
        .bind(request.config_hash)
        .bind(request.options_hash)
        .bind(request.output_kind)
        .bind(request.format)
        .bind(request.endpoint)
        .bind(request.method)
        .bind(request.path)
        .bind(request.request_json)
        .bind(request.defaults_policy_version)
        .bind(request.request_builder_version)
        .bind(request.status)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    #[cfg(test)]
    pub async fn latest_export_request_for_output(
        &self,
        source_hash: &str,
        config_hash: &str,
        options_hash: &str,
        output_kind: &str,
        format: &str,
    ) -> sqlx::Result<Option<ExportRequestRecord>> {
        sqlx::query(
            r#"
            SELECT request_hash, source_hash, config_hash, options_hash, output_kind, format,
                   endpoint, method, path, request_json, defaults_policy_version,
                   request_builder_version, status, created_at, updated_at
            FROM export_requests
            WHERE source_hash = ? AND config_hash = ? AND options_hash = ?
              AND output_kind = ? AND format = ?
            ORDER BY updated_at DESC, created_at DESC, request_hash DESC
            LIMIT 1
            "#,
        )
        .bind(source_hash)
        .bind(config_hash)
        .bind(options_hash)
        .bind(output_kind)
        .bind(format)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(export_request_record_from_row))
    }

    pub async fn latest_ready_artifact_for_request(
        &self,
        request_hash: &str,
    ) -> sqlx::Result<Option<ArtifactRecord>> {
        sqlx::query(
            r#"
            SELECT artifact_sets.artifact_set_hash AS artifact_key,
                   artifact_sets.config_hash,
                   artifact_sets.output_kind,
                   artifact_sets.status,
                   artifact_sets.primary_object_key AS object_key,
                   artifact_sets.source_hash,
                   artifact_sets.options_hash,
                   artifact_sets.metadata_json,
                   artifact_sets.created_at,
                   artifact_sets.superseded_at,
                   artifact_files.content_type,
                   artifact_files.byte_len,
                   artifact_files.sha256
            FROM artifact_sets
            LEFT JOIN artifact_files ON artifact_files.object_key = artifact_sets.primary_object_key
            WHERE artifact_sets.request_hash = ?
              AND artifact_sets.status = 'ready'
            ORDER BY artifact_sets.created_at DESC, artifact_sets.artifact_set_hash DESC
            LIMIT 1
            "#,
        )
        .bind(request_hash)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(artifact_record_from_v2_row))
    }

    pub async fn latest_artifact_set_for_request(
        &self,
        request_hash: &str,
    ) -> sqlx::Result<Option<ArtifactSetRecord>> {
        sqlx::query(
            r#"
            SELECT artifact_set_hash, source_hash, config_hash, options_hash, request_hash,
                   raw_payload_hash, postprocess_hash, output_kind, format, status,
                   primary_object_key, metadata_json, created_at, updated_at,
                   superseded_at, superseded_by, supersession_reason
            FROM artifact_sets
            WHERE request_hash = ?
            ORDER BY updated_at DESC, created_at DESC, artifact_set_hash DESC
            LIMIT 1
            "#,
        )
        .bind(request_hash)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(artifact_set_record_from_row))
    }

    #[cfg(test)]
    pub async fn translation(
        &self,
        translation_id: &str,
    ) -> sqlx::Result<Option<TranslationRecord>> {
        sqlx::query(
            r#"
            SELECT translation_id, request_hash, state, start_response_json, final_response_json,
                   poll_state_json, result_external_data_ids_json, result_element_ids_json,
                   response_hash, failure_reason, created_at, updated_at
            FROM translations
            WHERE translation_id = ?
            "#,
        )
        .bind(translation_id)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(translation_record_from_row))
    }

    pub async fn latest_translation_for_request(
        &self,
        request_hash: &str,
    ) -> sqlx::Result<Option<TranslationRecord>> {
        sqlx::query(
            r#"
            SELECT translation_id, request_hash, state, start_response_json, final_response_json,
                   poll_state_json, result_external_data_ids_json, result_element_ids_json,
                   response_hash, failure_reason, created_at, updated_at
            FROM translations
            WHERE request_hash = ?
            ORDER BY updated_at DESC, created_at DESC, translation_id DESC
            LIMIT 1
            "#,
        )
        .bind(request_hash)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(translation_record_from_row))
    }

    pub async fn latest_completed_translation_for_request(
        &self,
        request_hash: &str,
    ) -> sqlx::Result<Option<TranslationRecord>> {
        sqlx::query(
            r#"
            SELECT translation_id, request_hash, state, start_response_json, final_response_json,
                   poll_state_json, result_external_data_ids_json, result_element_ids_json,
                   response_hash, failure_reason, created_at, updated_at
            FROM translations
            WHERE request_hash = ? AND state = 'DONE' AND final_response_json IS NOT NULL
            ORDER BY updated_at DESC, created_at DESC, translation_id DESC
            LIMIT 1
            "#,
        )
        .bind(request_hash)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(translation_record_from_row))
    }

    pub async fn insert_translation_start(
        &self,
        translation: TranslationStartInsert<'_>,
    ) -> sqlx::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO translations (translation_id, request_hash, state, start_response_json)
            VALUES (?, ?, ?, ?)
            ON CONFLICT(translation_id) DO UPDATE SET
                request_hash = excluded.request_hash,
                state = excluded.state,
                start_response_json = excluded.start_response_json,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            "#,
        )
        .bind(translation.translation_id)
        .bind(translation.request_hash)
        .bind(translation.state)
        .bind(translation.start_response_json)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_translation_final(
        &self,
        translation: TranslationFinalUpdate<'_>,
    ) -> sqlx::Result<bool> {
        let result = sqlx::query(
            r#"
            UPDATE translations
            SET state = ?,
                final_response_json = ?,
                poll_state_json = ?,
                result_external_data_ids_json = ?,
                result_element_ids_json = ?,
                response_hash = ?,
                failure_reason = ?,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            WHERE translation_id = ?
            "#,
        )
        .bind(translation.state)
        .bind(translation.final_response_json)
        .bind(translation.poll_state_json)
        .bind(translation.result_external_data_ids_json)
        .bind(translation.result_element_ids_json)
        .bind(translation.response_hash)
        .bind(translation.failure_reason)
        .bind(translation.translation_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn raw_payload(
        &self,
        raw_payload_hash: &str,
    ) -> sqlx::Result<Option<RawPayloadRecord>> {
        sqlx::query(
            r#"
            SELECT raw_payload_hash, object_key, content_type, byte_len, headers_json,
                   original_filename, filename_source, detected_kind, zip_manifest_json,
                   created_at, updated_at
            FROM raw_payloads
            WHERE raw_payload_hash = ?
            "#,
        )
        .bind(raw_payload_hash)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(raw_payload_record_from_row))
    }

    pub async fn insert_raw_payload_if_absent(
        &self,
        payload: RawPayloadInsert<'_>,
    ) -> sqlx::Result<bool> {
        let result = sqlx::query(
            r#"
            INSERT INTO raw_payloads (
                raw_payload_hash, object_key, content_type, byte_len, headers_json,
                original_filename, filename_source, detected_kind, zip_manifest_json
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(raw_payload_hash) DO NOTHING
            "#,
        )
        .bind(payload.raw_payload_hash)
        .bind(payload.object_key)
        .bind(payload.content_type)
        .bind(payload.byte_len)
        .bind(payload.headers_json)
        .bind(payload.original_filename)
        .bind(payload.filename_source)
        .bind(payload.detected_kind)
        .bind(payload.zip_manifest_json)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn link_raw_payload_source(
        &self,
        source: RawPayloadSourceInsert<'_>,
    ) -> sqlx::Result<bool> {
        let result = sqlx::query(
            r#"
            INSERT INTO raw_payload_sources (
                request_hash, translation_id, external_data_id, result_index,
                response_headers_json, etag, raw_payload_hash
            )
            VALUES (?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(source.request_hash)
        .bind(source.translation_id)
        .bind(source.external_data_id)
        .bind(source.result_index)
        .bind(source.response_headers_json)
        .bind(source.etag)
        .bind(source.raw_payload_hash)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn raw_payload_hash_for_source(
        &self,
        request_hash: &str,
        translation_id: Option<&str>,
        external_data_id: Option<&str>,
        result_index: Option<i64>,
    ) -> sqlx::Result<Option<String>> {
        sqlx::query_scalar(
            r#"
            SELECT raw_payload_hash
            FROM raw_payload_sources
            WHERE request_hash = ?
              AND ifnull(translation_id, '') = ifnull(?, '')
              AND ifnull(external_data_id, '') = ifnull(?, '')
              AND ifnull(result_index, -1) = ifnull(?, -1)
            "#,
        )
        .bind(request_hash)
        .bind(translation_id)
        .bind(external_data_id)
        .bind(result_index)
        .fetch_optional(&self.pool)
        .await
    }

    #[cfg(test)]
    pub async fn postprocess_run(
        &self,
        postprocess_hash: &str,
    ) -> sqlx::Result<Option<PostprocessRunRecord>> {
        sqlx::query(
            r#"
            SELECT postprocess_hash, raw_payload_hash, processor_name, processor_version,
                   policy_json, status, log_json, derived_files_json, created_at, updated_at
            FROM postprocess_runs
            WHERE postprocess_hash = ?
            "#,
        )
        .bind(postprocess_hash)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(postprocess_run_record_from_row))
    }

    pub async fn insert_postprocess_run_if_absent(
        &self,
        run: PostprocessRunInsert<'_>,
    ) -> sqlx::Result<bool> {
        let result = sqlx::query(
            r#"
            INSERT INTO postprocess_runs (
                postprocess_hash, raw_payload_hash, processor_name, processor_version,
                policy_json, status, log_json, derived_files_json
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(postprocess_hash) DO NOTHING
            "#,
        )
        .bind(run.postprocess_hash)
        .bind(run.raw_payload_hash)
        .bind(run.processor_name)
        .bind(run.processor_version)
        .bind(run.policy_json)
        .bind(run.status)
        .bind(run.log_json)
        .bind(run.derived_files_json)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn transition_postprocess_run_status(
        &self,
        postprocess_hash: &str,
        status: &str,
        log_json: &str,
        derived_files_json: &str,
    ) -> sqlx::Result<bool> {
        let result = sqlx::query(
            r#"
            UPDATE postprocess_runs
            SET status = ?,
                log_json = ?,
                derived_files_json = ?,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            WHERE postprocess_hash = ?
            "#,
        )
        .bind(status)
        .bind(log_json)
        .bind(derived_files_json)
        .bind(postprocess_hash)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    #[cfg(test)]
    pub async fn artifact_set(
        &self,
        artifact_set_hash: &str,
    ) -> sqlx::Result<Option<ArtifactSetRecord>> {
        sqlx::query(
            r#"
            SELECT artifact_set_hash, source_hash, config_hash, options_hash, request_hash,
                   raw_payload_hash, postprocess_hash, output_kind, format, status,
                   primary_object_key, metadata_json, created_at, updated_at,
                   superseded_at, superseded_by, supersession_reason
            FROM artifact_sets
            WHERE artifact_set_hash = ?
            "#,
        )
        .bind(artifact_set_hash)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(artifact_set_record_from_row))
    }

    #[cfg(test)]
    pub async fn insert_artifact_set_if_absent(
        &self,
        artifact_set: ArtifactSetInsert<'_>,
    ) -> sqlx::Result<bool> {
        let result = sqlx::query(
            r#"
            INSERT INTO artifact_sets (
                artifact_set_hash, source_hash, config_hash, options_hash, request_hash,
                raw_payload_hash, postprocess_hash, output_kind, format, status,
                primary_object_key, metadata_json
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(artifact_set_hash) DO NOTHING
            "#,
        )
        .bind(artifact_set.artifact_set_hash)
        .bind(artifact_set.source_hash)
        .bind(artifact_set.config_hash)
        .bind(artifact_set.options_hash)
        .bind(artifact_set.request_hash)
        .bind(artifact_set.raw_payload_hash)
        .bind(artifact_set.postprocess_hash)
        .bind(artifact_set.output_kind)
        .bind(artifact_set.format)
        .bind(artifact_set.status)
        .bind(artifact_set.primary_object_key)
        .bind(artifact_set.metadata_json)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    #[cfg(test)]
    pub async fn replace_artifact_files(
        &self,
        artifact_set_hash: &str,
        files: &[ArtifactFileInsert<'_>],
    ) -> sqlx::Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM artifact_files WHERE artifact_set_hash = ?")
            .bind(artifact_set_hash)
            .execute(&mut *tx)
            .await?;

        for file in files {
            sqlx::query(
                r#"
                INSERT INTO artifact_files (
                    artifact_set_hash, role, logical_path, original_path, object_key,
                    content_type, byte_len, sha256, metadata_json
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(file.artifact_set_hash)
            .bind(file.role)
            .bind(file.logical_path)
            .bind(file.original_path)
            .bind(file.object_key)
            .bind(file.content_type)
            .bind(file.byte_len)
            .bind(file.sha256)
            .bind(file.metadata_json)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn mark_artifact_set_ready(
        &self,
        artifact_set_hash: &str,
        primary_object_key: &str,
    ) -> sqlx::Result<bool> {
        let result = sqlx::query(
            r#"
            UPDATE artifact_sets
            SET status = 'ready',
                primary_object_key = ?,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            WHERE artifact_set_hash = ?
            "#,
        )
        .bind(primary_object_key)
        .bind(artifact_set_hash)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn transition_artifact_set_status(
        &self,
        artifact_set_hash: &str,
        status: &str,
    ) -> sqlx::Result<bool> {
        let result = sqlx::query(
            r#"
            UPDATE artifact_sets
            SET status = ?,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            WHERE artifact_set_hash = ?
            "#,
        )
        .bind(status)
        .bind(artifact_set_hash)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn supersede_artifact_set(
        &self,
        artifact_set_hash: &str,
        superseded_by: Option<&str>,
        supersession_reason: Option<&str>,
    ) -> sqlx::Result<bool> {
        let result = sqlx::query(
            r#"
            UPDATE artifact_sets
            SET status = 'superseded',
                superseded_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                superseded_by = ?,
                supersession_reason = ?,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            WHERE artifact_set_hash = ? AND status <> 'superseded'
            "#,
        )
        .bind(superseded_by)
        .bind(supersession_reason)
        .bind(artifact_set_hash)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn supersede_ready_artifacts_for_output(
        &self,
        source_hash: &str,
        config_hash: &str,
        options_hash: &str,
        output_kind: &str,
        retained_artifact_set_hash: &str,
        supersession_reason: Option<&str>,
    ) -> sqlx::Result<u64> {
        let result = sqlx::query(
            r#"
            UPDATE artifact_sets
            SET status = 'superseded',
                superseded_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                superseded_by = ?,
                supersession_reason = ?,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            WHERE source_hash = ?
              AND config_hash = ?
              AND options_hash = ?
              AND output_kind = ?
              AND status = 'ready'
              AND artifact_set_hash <> ?
            "#,
        )
        .bind(retained_artifact_set_hash)
        .bind(supersession_reason)
        .bind(source_hash)
        .bind(config_hash)
        .bind(options_hash)
        .bind(output_kind)
        .bind(retained_artifact_set_hash)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn enqueue_job(
        &self,
        work_key: &str,
        job_kind: &str,
        payload_json: &str,
    ) -> sqlx::Result<bool> {
        self.enqueue_job_inner(work_key, job_kind, payload_json, false)
            .await
    }

    pub async fn force_enqueue_job(
        &self,
        work_key: &str,
        job_kind: &str,
        payload_json: &str,
    ) -> sqlx::Result<bool> {
        self.enqueue_job_inner(work_key, job_kind, payload_json, true)
            .await
    }

    async fn enqueue_job_inner(
        &self,
        work_key: &str,
        job_kind: &str,
        payload_json: &str,
        force_ready: bool,
    ) -> sqlx::Result<bool> {
        let result = sqlx::query(
            r#"
            INSERT INTO jobs (work_key, job_kind, status, payload_json)
            VALUES (?, ?, 'queued', ?)
            ON CONFLICT(work_key) DO UPDATE SET
                status = 'queued',
                job_kind = excluded.job_kind,
                payload_json = excluded.payload_json,
                error_summary = NULL,
                attempt = 0,
                lease_until = NULL,
                next_retry_at = NULL,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            WHERE jobs.status IN ('failed', 'superseded')
               OR (? AND jobs.status = 'ready')
            "#,
        )
        .bind(work_key)
        .bind(job_kind)
        .bind(payload_json)
        .bind(force_ready)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() == 1)
    }

    pub async fn claim_next_job(&self, lease_seconds: i64) -> sqlx::Result<Option<JobLease>> {
        sqlx::query(
            r#"
            UPDATE jobs
            SET status = 'running',
                attempt = attempt + 1,
                lease_until = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '+' || ? || ' seconds'),
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            WHERE id = (
                SELECT id
                FROM jobs
                WHERE (status = 'queued' AND (next_retry_at IS NULL OR next_retry_at <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now')))
                   OR (status = 'running' AND lease_until <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                ORDER BY CASE WHEN status = 'queued' THEN 0 ELSE 1 END,
                         COALESCE(next_retry_at, created_at),
                         created_at
                LIMIT 1
            )
            RETURNING work_key, job_kind, payload_json, attempt, max_attempts
            "#,
        )
        .bind(lease_seconds)
        .fetch_optional(&self.pool)
        .await
        .map(|row| {
            row.map(|row| JobLease {
                work_key: row.get("work_key"),
                job_kind: row.get("job_kind"),
                payload_json: row.get("payload_json"),
                attempt: row.get("attempt"),
                max_attempts: row.get("max_attempts"),
            })
        })
    }

    pub async fn finish_job(
        &self,
        work_key: &str,
        attempt: i64,
        status: &str,
        error_summary: Option<&str>,
    ) -> sqlx::Result<bool> {
        let result = sqlx::query(
            r#"
            UPDATE jobs
            SET status = ?,
                error_summary = ?,
                lease_until = NULL,
                next_retry_at = NULL,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            WHERE work_key = ? AND attempt = ? AND status = 'running'
            "#,
        )
        .bind(status)
        .bind(error_summary)
        .bind(work_key)
        .bind(attempt)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn record_job_failure(
        &self,
        work_key: &str,
        attempt: i64,
        error_summary: &str,
        retry_delay_seconds: i64,
    ) -> sqlx::Result<bool> {
        let result = sqlx::query(
            r#"
            UPDATE jobs
            SET status = CASE WHEN attempt >= max_attempts THEN 'failed' ELSE 'queued' END,
                error_summary = ?,
                lease_until = NULL,
                next_retry_at = CASE
                    WHEN attempt >= max_attempts THEN NULL
                    ELSE strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '+' || ? || ' seconds')
                END,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            WHERE work_key = ? AND attempt = ? AND status = 'running'
            "#,
        )
        .bind(error_summary)
        .bind(retry_delay_seconds.max(0))
        .bind(work_key)
        .bind(attempt)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn supersede_ready_job(&self, work_key: &str) -> sqlx::Result<bool> {
        let result = sqlx::query(
            r#"
            UPDATE jobs
            SET status = 'superseded',
                lease_until = NULL,
                next_retry_at = NULL,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            WHERE work_key = ? AND status = 'ready'
            "#,
        )
        .bind(work_key)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn artifact(&self, artifact_key: &str) -> sqlx::Result<Option<ArtifactRecord>> {
        sqlx::query(
            r#"
            SELECT artifact_sets.artifact_set_hash AS artifact_key,
                   artifact_sets.config_hash,
                   artifact_sets.output_kind,
                   artifact_sets.status,
                   artifact_sets.primary_object_key AS object_key,
                   artifact_sets.source_hash,
                   artifact_sets.options_hash,
                   artifact_sets.metadata_json,
                   artifact_sets.created_at,
                   artifact_sets.superseded_at,
                   artifact_files.content_type,
                   artifact_files.byte_len,
                   artifact_files.sha256
            FROM artifact_sets
            LEFT JOIN artifact_files ON artifact_files.object_key = artifact_sets.primary_object_key
            WHERE artifact_sets.artifact_set_hash = ? AND artifact_sets.status = 'ready'
            "#,
        )
        .bind(artifact_key)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(artifact_record_from_v2_row))
    }

    pub async fn artifacts_for_model(&self, model_slug: &str) -> sqlx::Result<Vec<ArtifactRecord>> {
        sqlx::query(
            r#"
            SELECT artifact_sets.artifact_set_hash AS artifact_key,
                   artifact_sets.config_hash,
                   artifact_sets.output_kind,
                   artifact_sets.status,
                   artifact_sets.primary_object_key AS object_key,
                   artifact_sets.source_hash,
                   artifact_sets.options_hash,
                   artifact_sets.metadata_json,
                   artifact_sets.created_at,
                   artifact_sets.superseded_at,
                   artifact_files.content_type,
                   artifact_files.byte_len,
                   artifact_files.sha256
            FROM artifact_sets
            LEFT JOIN artifact_files ON artifact_files.object_key = artifact_sets.primary_object_key
            WHERE json_extract(artifact_sets.metadata_json, '$.modelSlug') = ? AND artifact_sets.status = 'ready'
            ORDER BY artifact_sets.created_at DESC, artifact_sets.output_kind
            "#,
        )
        .bind(model_slug)
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(artifact_record_from_v2_row).collect())
    }

    pub async fn artifacts_older_than_days(
        &self,
        model_slug: &str,
        days: i64,
    ) -> sqlx::Result<Vec<ArtifactRecord>> {
        sqlx::query(
            r#"
            SELECT artifact_sets.artifact_set_hash AS artifact_key,
                   artifact_sets.config_hash,
                   artifact_sets.output_kind,
                   artifact_sets.status,
                   artifact_sets.primary_object_key AS object_key,
                   artifact_sets.source_hash,
                   artifact_sets.options_hash,
                   artifact_sets.metadata_json,
                   artifact_sets.created_at,
                   artifact_sets.superseded_at,
                   artifact_files.content_type,
                   artifact_files.byte_len,
                   artifact_files.sha256
            FROM artifact_sets
            LEFT JOIN artifact_files ON artifact_files.object_key = artifact_sets.primary_object_key
            WHERE json_extract(artifact_sets.metadata_json, '$.modelSlug') = ?
              AND artifact_sets.status = 'ready'
              AND artifact_sets.created_at < strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-' || ? || ' days')
            ORDER BY artifact_sets.created_at, artifact_sets.output_kind
            "#,
        )
        .bind(model_slug)
        .bind(days)
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(artifact_record_from_v2_row).collect())
    }

    #[cfg(test)]
    pub async fn latest_ready_artifact_for_output(
        &self,
        source_hash: &str,
        config_hash: &str,
        options_hash: &str,
        output_kind: &str,
    ) -> sqlx::Result<Option<ArtifactRecord>> {
        sqlx::query(
            r#"
            SELECT artifact_sets.artifact_set_hash AS artifact_key,
                   artifact_sets.config_hash,
                   artifact_sets.output_kind,
                   artifact_sets.status,
                   artifact_sets.primary_object_key AS object_key,
                   artifact_sets.source_hash,
                   artifact_sets.options_hash,
                   artifact_sets.metadata_json,
                   artifact_sets.created_at,
                   artifact_sets.superseded_at,
                   artifact_files.content_type,
                   artifact_files.byte_len,
                   artifact_files.sha256
            FROM artifact_sets
            LEFT JOIN artifact_files ON artifact_files.object_key = artifact_sets.primary_object_key
            WHERE artifact_sets.source_hash = ?
              AND artifact_sets.config_hash = ?
              AND artifact_sets.options_hash = ?
              AND artifact_sets.output_kind = ?
              AND artifact_sets.status = 'ready'
            ORDER BY artifact_sets.created_at DESC
            LIMIT 1
            "#,
        )
        .bind(source_hash)
        .bind(config_hash)
        .bind(options_hash)
        .bind(output_kind)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(artifact_record_from_v2_row))
    }

    pub async fn supersede_artifact(&self, artifact_key: &str) -> sqlx::Result<bool> {
        self.supersede_artifact_set(artifact_key, None, None).await
    }

    pub async fn stage_artifact(
        &self,
        artifact: ArtifactUpsert<'_>,
        files: &[ArtifactFileInsert<'_>],
    ) -> sqlx::Result<()> {
        let metadata_json = serde_json::to_string(&ArtifactMetadata {
            model_slug: artifact.model_slug.to_owned(),
            producing_job_key: artifact.producing_job_key.map(ToOwned::to_owned),
            parameter_schema_version: Some(artifact.parameter_schema_version),
            config_values_json: Some(artifact.config_values_json.to_owned()),
        })
        .expect("artifact metadata serializes");

        let mut tx = self.pool.begin().await?;
        sqlx::query(
            r#"
            INSERT INTO artifact_sets (
                artifact_set_hash, source_hash, config_hash, options_hash, request_hash,
                raw_payload_hash, postprocess_hash, output_kind, format, status,
                primary_object_key, metadata_json
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'staged', ?, ?)
            ON CONFLICT(artifact_set_hash) DO UPDATE SET
                source_hash = excluded.source_hash,
                config_hash = excluded.config_hash,
                options_hash = excluded.options_hash,
                request_hash = excluded.request_hash,
                raw_payload_hash = excluded.raw_payload_hash,
                postprocess_hash = excluded.postprocess_hash,
                output_kind = excluded.output_kind,
                format = excluded.format,
                status = 'staged',
                primary_object_key = excluded.primary_object_key,
                metadata_json = excluded.metadata_json,
                created_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                superseded_at = NULL,
                superseded_by = NULL,
                supersession_reason = NULL
            "#,
        )
        .bind(artifact.artifact_key)
        .bind(artifact.source_hash)
        .bind(artifact.config_hash)
        .bind(artifact.options_hash)
        .bind(artifact.request_hash)
        .bind(artifact.raw_payload_hash)
        .bind(artifact.postprocess_hash)
        .bind(artifact.output_kind)
        .bind(artifact.format)
        .bind(artifact.object_key)
        .bind(&metadata_json)
        .execute(&mut *tx)
        .await?;

        sqlx::query("DELETE FROM artifact_files WHERE artifact_set_hash = ?")
            .bind(artifact.artifact_key)
            .execute(&mut *tx)
            .await?;

        for file in files {
            sqlx::query(
                r#"
                INSERT INTO artifact_files (
                    artifact_set_hash, role, logical_path, original_path, object_key,
                    content_type, byte_len, sha256, metadata_json
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(file.artifact_set_hash)
            .bind(file.role)
            .bind(file.logical_path)
            .bind(file.original_path)
            .bind(file.object_key)
            .bind(file.content_type)
            .bind(file.byte_len)
            .bind(file.sha256)
            .bind(file.metadata_json)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    #[cfg(test)]
    pub async fn publish_artifact(
        &self,
        artifact: ArtifactUpsert<'_>,
        files: &[ArtifactFileInsert<'_>],
    ) -> sqlx::Result<()> {
        self.stage_artifact(artifact, files).await?;
        self.mark_artifact_set_ready(artifact.artifact_key, artifact.object_key)
            .await?;
        Ok(())
    }

    #[cfg(test)]
    pub async fn upsert_artifact(&self, artifact: ArtifactUpsert<'_>) -> sqlx::Result<()> {
        let logical_path = artifact
            .object_key
            .rsplit('/')
            .next()
            .unwrap_or(artifact.object_key);
        self.publish_artifact(
            artifact,
            &[ArtifactFileInsert {
                artifact_set_hash: artifact.artifact_key,
                role: if artifact.output_kind == "preview_glb" {
                    "viewer_entry"
                } else {
                    "download"
                },
                logical_path,
                original_path: Some(logical_path),
                object_key: artifact.object_key,
                content_type: artifact.content_type,
                byte_len: artifact.byte_len,
                sha256: artifact.sha256,
                metadata_json: "{}",
            }],
        )
        .await
    }

    pub async fn failed_jobs(&self, limit: i64) -> sqlx::Result<Vec<JobRecord>> {
        sqlx::query(
            r#"
            SELECT work_key, job_kind, status, error_summary, attempt, max_attempts,
                   next_retry_at, created_at, updated_at
            FROM jobs
            WHERE status = 'failed' AND payload_json <> '{}'
            ORDER BY updated_at DESC
            LIMIT ?
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(job_record_from_row).collect())
    }

    pub async fn jobs(&self, limit: i64) -> sqlx::Result<Vec<JobRecord>> {
        sqlx::query(
            r#"
            SELECT work_key, job_kind, status, error_summary, attempt, max_attempts,
                   next_retry_at, created_at, updated_at
            FROM jobs
            WHERE payload_json <> '{}'
            ORDER BY updated_at DESC
            LIMIT ?
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(job_record_from_row).collect())
    }

    pub async fn job(&self, work_key: &str) -> sqlx::Result<Option<JobRecord>> {
        sqlx::query(
            r#"
            SELECT work_key, job_kind, status, error_summary, attempt, max_attempts,
                   next_retry_at, created_at, updated_at
            FROM jobs
            WHERE work_key = ?
            "#,
        )
        .bind(work_key)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(job_record_from_row))
    }

    pub async fn job_metrics(&self) -> sqlx::Result<Vec<JobMetric>> {
        sqlx::query(
            r#"
            SELECT job_kind, status, COUNT(*) AS count
            FROM jobs
            GROUP BY job_kind, status
            ORDER BY job_kind, status
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(job_metric_from_row).collect())
    }

    pub async fn artifact_metrics(&self) -> sqlx::Result<Vec<ArtifactMetric>> {
        sqlx::query(
            r#"
            SELECT artifact_sets.output_kind, COUNT(*) AS count, COALESCE(SUM(artifact_files.byte_len), 0) AS byte_len
            FROM artifact_sets
            LEFT JOIN artifact_files ON artifact_files.object_key = artifact_sets.primary_object_key
            WHERE artifact_sets.status = 'ready'
            GROUP BY artifact_sets.output_kind
            ORDER BY artifact_sets.output_kind
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(artifact_metric_from_row).collect())
    }

    pub async fn retry_failed_jobs(&self) -> sqlx::Result<u64> {
        let result = sqlx::query(
            r#"
            UPDATE jobs
            SET status = 'queued',
                error_summary = NULL,
                attempt = 0,
                lease_until = NULL,
                next_retry_at = NULL,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            WHERE status = 'failed' AND payload_json <> '{}'
            "#,
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn retry_failed_job(&self, work_key: &str) -> sqlx::Result<bool> {
        let result = sqlx::query(
            r#"
            UPDATE jobs
            SET status = 'queued',
                error_summary = NULL,
                attempt = 0,
                lease_until = NULL,
                next_retry_at = NULL,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            WHERE work_key = ? AND status = 'failed' AND payload_json <> '{}'
            "#,
        )
        .bind(work_key)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn retry_failed_jobs_by_kind(&self, job_kind: &str) -> sqlx::Result<u64> {
        let result = sqlx::query(
            r#"
            UPDATE jobs
            SET status = 'queued',
                error_summary = NULL,
                attempt = 0,
                lease_until = NULL,
                next_retry_at = NULL,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            WHERE job_kind = ? AND status = 'failed' AND payload_json <> '{}'
            "#,
        )
        .bind(job_kind)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    async fn apply_pragmas(pool: &SqlitePool) -> sqlx::Result<()> {
        pool.execute("PRAGMA journal_mode = WAL").await?;
        pool.execute("PRAGMA synchronous = FULL").await?;
        pool.execute("PRAGMA foreign_keys = ON").await?;
        pool.execute("PRAGMA busy_timeout = 5000").await?;
        Ok(())
    }
}

fn bool_to_i64(value: bool) -> i64 {
    if value { 1 } else { 0 }
}

fn bool_column(row: &sqlx::sqlite::SqliteRow, column: &str) -> bool {
    row.get::<i64, _>(column) != 0
}

fn u32_column(row: &sqlx::sqlite::SqliteRow, column: &str) -> anyhow::Result<u32> {
    let value: i64 = row.get(column);
    u32::try_from(value).with_context(|| format!("invalid unsigned integer in {column}"))
}

fn json_column<T>(row: &sqlx::sqlite::SqliteRow, column: &str) -> anyhow::Result<T>
where
    T: DeserializeOwned,
{
    let value: String = row.get(column);
    serde_json::from_str(&value).with_context(|| format!("parsing catalog JSON column {column}"))
}

fn enum_text_column<T>(row: &sqlx::sqlite::SqliteRow, column: &str) -> anyhow::Result<T>
where
    T: DeserializeOwned,
{
    let value: String = row.get(column);
    serde_json::from_value(serde_json::Value::String(value.clone()))
        .with_context(|| format!("parsing catalog enum column {column}: {value}"))
}

fn preview_format_key(format: &PreviewFormat) -> &'static str {
    match format {
        PreviewFormat::Glb => "glb",
    }
}

fn parameter_source_key(source: &ParameterSource) -> &'static str {
    match source {
        ParameterSource::Onshape => "onshape",
    }
}

fn artifact_record_from_v2_row(row: sqlx::sqlite::SqliteRow) -> ArtifactRecord {
    let metadata = row
        .try_get::<String, _>("metadata_json")
        .ok()
        .and_then(|value| serde_json::from_str::<ArtifactMetadata>(&value).ok())
        .unwrap_or_default();

    ArtifactRecord {
        artifact_key: row.get("artifact_key"),
        model_slug: metadata.model_slug,
        config_hash: row.get("config_hash"),
        output_kind: row.get("output_kind"),
        status: row.get("status"),
        object_key: row.get("object_key"),
        content_type: row.get("content_type"),
        byte_len: row.get("byte_len"),
        sha256: row.get("sha256"),
        producing_job_key: metadata.producing_job_key,
        source_hash: row.get("source_hash"),
        options_hash: row.get("options_hash"),
        parameter_schema_version: metadata.parameter_schema_version,
        config_values_json: metadata.config_values_json,
        created_at: row.get("created_at"),
        superseded_at: row.get("superseded_at"),
    }
}

fn source_resolution_record_from_row(row: sqlx::sqlite::SqliteRow) -> SourceResolutionRecord {
    SourceResolutionRecord {
        source_hash: row.get("source_hash"),
        model_slug: row.get("model_slug"),
        document_id: row.get("document_id"),
        version_id: row.get("version_id"),
        microversion_id: row.get("microversion_id"),
        element_id: row.get("element_id"),
        element_kind: row.get("element_kind"),
        link_document_id: row.get("link_document_id"),
        diagnostics_json: row.get("diagnostics_json"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

#[cfg(test)]
fn configuration_selection_record_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> ConfigurationSelectionRecord {
    ConfigurationSelectionRecord {
        source_hash: row.get("source_hash"),
        config_hash: row.get("config_hash"),
        values_json: row.get("values_json"),
        validation_json: row.get("validation_json"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn configuration_encoding_record_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> ConfigurationEncodingRecord {
    ConfigurationEncodingRecord {
        source_hash: row.get("source_hash"),
        config_hash: row.get("config_hash"),
        encoded_id: row.get("encoded_id"),
        query_param: row.get("query_param"),
        request_json: row.get("request_json"),
        response_json: row.get("response_json"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

#[cfg(test)]
fn export_request_record_from_row(row: sqlx::sqlite::SqliteRow) -> ExportRequestRecord {
    ExportRequestRecord {
        request_hash: row.get("request_hash"),
        source_hash: row.get("source_hash"),
        config_hash: row.get("config_hash"),
        options_hash: row.get("options_hash"),
        output_kind: row.get("output_kind"),
        format: row.get("format"),
        endpoint: row.get("endpoint"),
        method: row.get("method"),
        path: row.get("path"),
        request_json: row.get("request_json"),
        defaults_policy_version: row.get("defaults_policy_version"),
        request_builder_version: row.get("request_builder_version"),
        status: row.get("status"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn translation_record_from_row(row: sqlx::sqlite::SqliteRow) -> TranslationRecord {
    TranslationRecord {
        translation_id: row.get("translation_id"),
        request_hash: row.get("request_hash"),
        state: row.get("state"),
        start_response_json: row.get("start_response_json"),
        final_response_json: row.get("final_response_json"),
        poll_state_json: row.get("poll_state_json"),
        result_external_data_ids_json: row.get("result_external_data_ids_json"),
        result_element_ids_json: row.get("result_element_ids_json"),
        response_hash: row.get("response_hash"),
        failure_reason: row.get("failure_reason"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn raw_payload_record_from_row(row: sqlx::sqlite::SqliteRow) -> RawPayloadRecord {
    RawPayloadRecord {
        raw_payload_hash: row.get("raw_payload_hash"),
        object_key: row.get("object_key"),
        content_type: row.get("content_type"),
        byte_len: row.get("byte_len"),
        headers_json: row.get("headers_json"),
        original_filename: row.get("original_filename"),
        filename_source: row.get("filename_source"),
        detected_kind: row.get("detected_kind"),
        zip_manifest_json: row.get("zip_manifest_json"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

#[cfg(test)]
fn postprocess_run_record_from_row(row: sqlx::sqlite::SqliteRow) -> PostprocessRunRecord {
    PostprocessRunRecord {
        postprocess_hash: row.get("postprocess_hash"),
        raw_payload_hash: row.get("raw_payload_hash"),
        processor_name: row.get("processor_name"),
        processor_version: row.get("processor_version"),
        policy_json: row.get("policy_json"),
        status: row.get("status"),
        log_json: row.get("log_json"),
        derived_files_json: row.get("derived_files_json"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn artifact_set_record_from_row(row: sqlx::sqlite::SqliteRow) -> ArtifactSetRecord {
    ArtifactSetRecord {
        artifact_set_hash: row.get("artifact_set_hash"),
        source_hash: row.get("source_hash"),
        config_hash: row.get("config_hash"),
        options_hash: row.get("options_hash"),
        request_hash: row.get("request_hash"),
        raw_payload_hash: row.get("raw_payload_hash"),
        postprocess_hash: row.get("postprocess_hash"),
        output_kind: row.get("output_kind"),
        format: row.get("format"),
        status: row.get("status"),
        primary_object_key: row.get("primary_object_key"),
        metadata_json: row.get("metadata_json"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        superseded_at: row.get("superseded_at"),
        superseded_by: row.get("superseded_by"),
        supersession_reason: row.get("supersession_reason"),
    }
}

fn job_record_from_row(row: sqlx::sqlite::SqliteRow) -> JobRecord {
    JobRecord {
        work_key: row.get("work_key"),
        job_kind: row.get("job_kind"),
        status: row.get("status"),
        error_summary: row.get("error_summary"),
        attempt: row.get("attempt"),
        max_attempts: row.get("max_attempts"),
        next_retry_at: row.get("next_retry_at"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn job_metric_from_row(row: sqlx::sqlite::SqliteRow) -> JobMetric {
    JobMetric {
        job_kind: row.get("job_kind"),
        status: row.get("status"),
        count: row.get("count"),
    }
}

fn artifact_metric_from_row(row: sqlx::sqlite::SqliteRow) -> ArtifactMetric {
    ArtifactMetric {
        output_kind: row.get("output_kind"),
        count: row.get("count"),
        byte_len: row.get("byte_len"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{
        DownloadFormat, DownloadOptions, ElementKind, MeshResolution, PreviewOptions,
        PreviewResolution, StepVersionString, StlDownloadOptions, StlMode, ThreeMfDownloadOptions,
    };
    use std::time::Duration;

    #[tokio::test]
    async fn catalog_sql_round_trip_preserves_live_fields() {
        let db = test_database().await;
        let mut model = test_catalog_model("demo", "did");
        model.published = false;
        model.tags = vec!["example".to_owned(), "fixture".to_owned()];
        model.thumbnail = Some("thumbs/demo.png".to_owned());
        model.exports.preview_options.resolution = PreviewResolution::Fine;
        model.exports.download_options.step_version_string = StepVersionString::Ap242;
        model.exports.download_options.stl = StlDownloadOptions {
            resolution: MeshResolution::Fine,
            stl_mode: StlMode::Binary,
        };
        model.exports.download_options.three_mf = ThreeMfDownloadOptions {
            resolution: MeshResolution::Medium,
        };
        model.parameter_policy.allow_unknown = true;
        model.parameter_policy.auto_refresh = false;
        model.parameter_presets = vec![ParameterPreset {
            slug: "small".to_owned(),
            name: "Small".to_owned(),
            values: HashMap::from([("width".to_owned(), "10".to_owned())]),
        }];
        model.parameter_overrides.insert(
            "width".to_owned(),
            ParameterOverride {
                label: Some("Width".to_owned()),
                description: Some("Visible width".to_owned()),
                hidden: true,
                precision: Some(3),
                widget: Some("number".to_owned()),
            },
        );
        let catalog = Catalog::from_models(vec![model]).unwrap();

        db.replace_catalog(&catalog).await.unwrap();
        let loaded = db.catalog().await.unwrap();
        let loaded_model = loaded.find("demo").unwrap();

        assert!(!loaded_model.published);
        assert_eq!(loaded_model.tags, ["example", "fixture"]);
        assert_eq!(loaded_model.thumbnail.as_deref(), Some("thumbs/demo.png"));
        assert_eq!(
            loaded_model.exports.preview_options.resolution,
            PreviewResolution::Fine
        );
        assert_eq!(
            loaded_model.exports.download_options.step_version_string,
            StepVersionString::Ap242
        );
        assert_eq!(
            loaded_model.exports.download_options.stl.resolution,
            MeshResolution::Fine
        );
        assert_eq!(
            loaded_model.exports.download_options.stl.stl_mode,
            StlMode::Binary
        );
        assert_eq!(
            loaded_model.exports.download_options.three_mf.resolution,
            MeshResolution::Medium
        );
        assert_eq!(loaded_model.parameter_presets[0].slug, "small");
        assert_eq!(loaded_model.parameter_presets[0].values["width"], "10");
        let override_ = &loaded_model.parameter_overrides["width"];
        assert_eq!(override_.label.as_deref(), Some("Width"));
        assert!(override_.hidden);
        assert_eq!(override_.precision, Some(3));
        assert_eq!(override_.widget.as_deref(), Some("number"));
    }

    #[tokio::test]
    async fn published_catalog_model_loads_only_published_slug() {
        let db = test_database().await;
        let mut published = test_catalog_model("demo", "did");
        published.tags = vec!["example".to_owned()];
        published.parameter_presets = vec![ParameterPreset {
            slug: "small".to_owned(),
            name: "Small".to_owned(),
            values: HashMap::from([("width".to_owned(), "10".to_owned())]),
        }];
        let mut unpublished = test_catalog_model("draft", "draft-did");
        unpublished.published = false;
        let catalog = Catalog::from_models(vec![published, unpublished]).unwrap();

        db.replace_catalog(&catalog).await.unwrap();

        let loaded = db.published_catalog_model("demo").await.unwrap().unwrap();
        assert_eq!(loaded.slug, "demo");
        assert!(loaded.published);
        assert_eq!(loaded.tags, ["example"]);
        assert_eq!(loaded.parameter_presets[0].slug, "small");
        assert_eq!(loaded.parameter_presets[0].values["width"], "10");
        assert!(db.published_catalog_model("draft").await.unwrap().is_none());
        assert!(
            db.published_catalog_model("missing")
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn catalog_sql_imports_current_json_seed() {
        let db = test_database().await;
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("catalog/v1/models.json");
        let catalog = Catalog::load(path).unwrap();

        db.replace_catalog(&catalog).await.unwrap();
        let loaded = db.catalog().await.unwrap();

        assert_eq!(loaded.models().len(), catalog.models().len());
        assert!(loaded.find("onshape-model").is_some());
        assert!(loaded.find("box-slide-print").is_some());
    }

    #[tokio::test]
    async fn catalog_sql_load_rejects_invalid_rows() {
        let db = test_database().await;
        sqlx::query(
            r#"
            INSERT INTO catalog_models (
                display_order, catalog_schema_version, entry_version, slug, name,
                description, published, tags_json, document_id, version_id,
                element_id, element_kind, downloads_json, preview_format,
                preview_options_json, download_options_json, parameter_source,
                parameter_allow_unknown, parameter_auto_refresh
            )
            VALUES (0, 1, 1, 'Bad/Slug', 'Name', 'Description', 1, '[]',
                    'did', 'vid', 'eid', 'part_studio', '["step"]', 'glb',
                    '{"resolution":"FINE"}',
                    '{"stepVersionString":"AP242","stl":{"resolution":"fine","stlMode":"BINARY"},"3mf":{"resolution":"fine"}}',
                    'onshape', 0, 1)
            "#,
        )
        .execute(&db.pool)
        .await
        .unwrap();

        let error = db.catalog().await.unwrap_err();

        assert!(error.to_string().contains("slug"));
    }

    #[tokio::test]
    async fn catalog_sql_rejects_invalid_preview_format() {
        let db = test_database().await;
        let error = sqlx::query(
            r#"
            INSERT INTO catalog_models (
                display_order, catalog_schema_version, entry_version, slug, name,
                description, published, tags_json, document_id, version_id,
                element_id, element_kind, downloads_json, preview_format,
                preview_options_json, download_options_json, parameter_source,
                parameter_allow_unknown, parameter_auto_refresh
            )
            VALUES (0, 1, 1, 'demo', 'Name', 'Description', 1, '[]',
                    'did', 'vid', 'eid', 'part_studio', '["step"]', 'stl',
                    '{}', '{}', 'onshape', 0, 1)
            "#,
        )
        .execute(&db.pool)
        .await
        .unwrap_err();

        assert!(error.to_string().contains("preview_format"));
    }

    #[tokio::test]
    async fn catalog_sql_import_rejects_duplicate_slugs_and_sources() {
        let db = test_database().await;
        let duplicate_slugs: Catalog = serde_json::from_value(serde_json::json!({
            "catalogSchemaVersion": 1,
            "models": [
                serde_json::to_value(test_catalog_model("same", "first-did")).unwrap(),
                serde_json::to_value(test_catalog_model("same", "second-did")).unwrap()
            ]
        }))
        .unwrap();
        let duplicate_slug_error = db.replace_catalog(&duplicate_slugs).await.unwrap_err();

        assert!(
            duplicate_slug_error
                .to_string()
                .contains("duplicate catalog model slug")
        );

        let duplicate_sources: Catalog = serde_json::from_value(serde_json::json!({
            "catalogSchemaVersion": 1,
            "models": [
                serde_json::to_value(test_catalog_model("first", "same-did")).unwrap(),
                serde_json::to_value(test_catalog_model("second", "same-did")).unwrap()
            ]
        }))
        .unwrap();
        let duplicate_source_error = db.replace_catalog(&duplicate_sources).await.unwrap_err();

        assert!(
            duplicate_source_error
                .to_string()
                .contains("duplicate catalog source identity")
        );
    }

    #[tokio::test]
    async fn ready_jobs_are_not_requeued_unless_forced() {
        let db = test_database().await;
        let payload = r#"{"kind":"parameter_refresh","model_slug":"demo"}"#;

        assert!(
            db.enqueue_job("work", "parameter_refresh", payload)
                .await
                .unwrap()
        );
        let job = db.claim_next_job(60).await.unwrap().unwrap();
        assert_eq!(job.work_key, "work");
        assert!(
            db.finish_job("work", job.attempt, "ready", None)
                .await
                .unwrap()
        );

        assert!(
            !db.enqueue_job("work", "parameter_refresh", payload)
                .await
                .unwrap()
        );
        assert!(db.claim_next_job(60).await.unwrap().is_none());

        assert!(
            db.force_enqueue_job("work", "parameter_refresh", payload)
                .await
                .unwrap()
        );
        let job = db.claim_next_job(60).await.unwrap().unwrap();
        assert_eq!(job.work_key, "work");
    }

    #[tokio::test]
    async fn superseded_ready_jobs_can_be_requeued_normally() {
        let db = test_database().await;
        let payload = r#"{"kind":"parameter_refresh","model_slug":"demo"}"#;

        db.enqueue_job("work", "parameter_refresh", payload)
            .await
            .unwrap();
        let job = db.claim_next_job(60).await.unwrap().unwrap();
        db.finish_job("work", job.attempt, "ready", None)
            .await
            .unwrap();

        assert!(db.supersede_ready_job("work").await.unwrap());
        assert!(
            db.enqueue_job("work", "parameter_refresh", payload)
                .await
                .unwrap()
        );
        assert_eq!(db.claim_next_job(60).await.unwrap().unwrap().attempt, 1);
    }

    #[tokio::test]
    async fn jobs_can_be_looked_up_by_work_key() {
        let db = test_database().await;
        let payload = r#"{"kind":"parameter_refresh","model_slug":"demo"}"#;

        assert!(db.job("work").await.unwrap().is_none());
        db.enqueue_job("work", "parameter_refresh", payload)
            .await
            .unwrap();

        let job = db.job("work").await.unwrap().unwrap();
        assert_eq!(job.work_key, "work");
        assert_eq!(job.status, "queued");
    }

    #[tokio::test]
    async fn jobs_can_be_listed_for_operations() {
        let db = test_database().await;
        let payload = r#"{"kind":"parameter_refresh","model_slug":"demo"}"#;

        db.enqueue_job("first", "parameter_refresh", payload)
            .await
            .unwrap();
        db.enqueue_job("second", "parameter_refresh", payload)
            .await
            .unwrap();

        let jobs = db.jobs(10).await.unwrap();

        assert_eq!(jobs.len(), 2);
        assert!(jobs.iter().any(|job| job.work_key == "first"));
        assert!(jobs.iter().any(|job| job.work_key == "second"));
    }

    #[tokio::test]
    async fn artifacts_can_be_selected_by_age() {
        let db = test_database().await;
        db.upsert_artifact(test_artifact_upsert(
            "old",
            "abc",
            "preview_glb",
            "previews/demo/old.glb",
            "model/gltf-binary",
            10,
        ))
        .await
        .unwrap();
        db.upsert_artifact(test_artifact_upsert(
            "new",
            "def",
            "step",
            "artifacts/demo/new.step",
            "model/step",
            20,
        ))
        .await
        .unwrap();
        sqlx::query(
            "UPDATE artifact_sets SET created_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-10 days') WHERE artifact_set_hash = 'old'",
        )
        .execute(&db.pool)
        .await
        .unwrap();

        let artifacts = db.artifacts_older_than_days("demo", 7).await.unwrap();

        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].artifact_key, "old");
    }

    #[tokio::test]
    async fn latest_ready_artifact_for_output_uses_logical_output_identity() {
        let db = test_database().await;
        db.upsert_artifact(test_artifact_upsert(
            "first-set",
            "abc",
            "step",
            "artifacts/v2/first-set/demo-step.step",
            "model/step",
            10,
        ))
        .await
        .unwrap();
        db.upsert_artifact(test_artifact_upsert(
            "second-set",
            "abc",
            "step",
            "artifacts/v2/second-set/demo-step.step",
            "model/step",
            12,
        ))
        .await
        .unwrap();

        let artifact = db
            .latest_ready_artifact_for_output("sourcehash", "abc", "optionshash", "step")
            .await
            .unwrap()
            .unwrap();

        assert_eq!(artifact.artifact_key, "second-set");
        assert_eq!(
            artifact.object_key,
            "artifacts/v2/second-set/demo-step.step"
        );
    }

    #[tokio::test]
    async fn latest_ready_artifact_for_request_uses_exact_request_hash() {
        let db = test_database().await;
        db.upsert_artifact(ArtifactUpsert {
            artifact_key: "first-set",
            model_slug: "demo",
            config_hash: "abc",
            output_kind: "step",
            format: "step",
            object_key: "artifacts/v2/first-set/demo-step.step",
            content_type: "model/step",
            byte_len: 10,
            sha256: "sha-first",
            producing_job_key: Some("work-v2:export:older-request"),
            source_hash: "sourcehash",
            options_hash: "optionshash",
            request_hash: Some("older-request"),
            raw_payload_hash: Some("raw-first"),
            postprocess_hash: Some("post-first"),
            parameter_schema_version: 2,
            config_values_json: "{}",
        })
        .await
        .unwrap();
        db.upsert_artifact(ArtifactUpsert {
            artifact_key: "second-set",
            model_slug: "demo",
            config_hash: "abc",
            output_kind: "step",
            format: "step",
            object_key: "artifacts/v2/second-set/demo-step.step",
            content_type: "model/step",
            byte_len: 12,
            sha256: "sha-second",
            producing_job_key: Some("work-v2:export:newer-request"),
            source_hash: "sourcehash",
            options_hash: "optionshash",
            request_hash: Some("newer-request"),
            raw_payload_hash: Some("raw-second"),
            postprocess_hash: Some("post-second"),
            parameter_schema_version: 2,
            config_values_json: "{}",
        })
        .await
        .unwrap();

        let artifact = db
            .latest_ready_artifact_for_request("older-request")
            .await
            .unwrap()
            .unwrap();

        assert_eq!(artifact.artifact_key, "first-set");
        assert_eq!(artifact.object_key, "artifacts/v2/first-set/demo-step.step");
    }

    #[tokio::test]
    async fn supersede_ready_artifacts_for_output_retires_all_other_ready_sets() {
        let db = test_database().await;
        for artifact_key in ["first-set", "second-set", "kept-set"] {
            db.upsert_artifact(test_artifact_upsert(
                artifact_key,
                "abc",
                "step",
                &format!("artifacts/v2/{artifact_key}/demo-step.step"),
                "model/step",
                10,
            ))
            .await
            .unwrap();
        }

        let superseded = db
            .supersede_ready_artifacts_for_output(
                "sourcehash",
                "abc",
                "optionshash",
                "step",
                "kept-set",
                Some("replaced"),
            )
            .await
            .unwrap();

        assert_eq!(superseded, 2);
        assert_eq!(
            db.artifact("kept-set").await.unwrap().unwrap().status,
            "ready"
        );
        assert!(db.artifact("first-set").await.unwrap().is_none());
        assert!(db.artifact("second-set").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn superseded_artifacts_are_hidden_but_retained() {
        let db = test_database().await;
        db.upsert_artifact(test_artifact_upsert(
            "artifact",
            "abc",
            "preview_glb",
            "previews/demo/artifact.glb",
            "model/gltf-binary",
            10,
        ))
        .await
        .unwrap();

        assert!(db.artifact("artifact").await.unwrap().is_some());
        assert!(db.supersede_artifact("artifact").await.unwrap());

        assert!(db.artifact("artifact").await.unwrap().is_none());
        let retained_status: String = sqlx::query_scalar(
            "SELECT status FROM artifact_sets WHERE artifact_set_hash = 'artifact'",
        )
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(retained_status, "superseded");
    }

    #[tokio::test]
    async fn superseded_artifacts_can_be_republished_cleanly() {
        let db = test_database().await;
        db.upsert_artifact(test_artifact_upsert(
            "artifact",
            "abc",
            "preview_glb",
            "previews/demo/first.glb",
            "model/gltf-binary",
            10,
        ))
        .await
        .unwrap();
        db.supersede_artifact("artifact").await.unwrap();

        db.upsert_artifact(test_artifact_upsert(
            "artifact",
            "abc",
            "preview_glb",
            "previews/demo/second.glb",
            "model/gltf-binary",
            11,
        ))
        .await
        .unwrap();

        let artifact = db.artifact("artifact").await.unwrap().unwrap();
        assert_eq!(artifact.object_key, "previews/demo/second.glb");
        let record = db.artifact_set("artifact").await.unwrap().unwrap();
        assert_eq!(record.status, "ready");
        assert_eq!(record.superseded_at, None);
        assert_eq!(record.superseded_by, None);
        assert_eq!(record.supersession_reason, None);
    }

    #[tokio::test]
    async fn failed_jobs_wait_for_next_retry_and_stop_at_max_attempts() {
        let db = test_database().await;
        let payload = r#"{"kind":"parameter_refresh","model_slug":"demo"}"#;

        db.enqueue_job("work", "parameter_refresh", payload)
            .await
            .unwrap();
        let first = db.claim_next_job(60).await.unwrap().unwrap();
        assert_eq!(first.max_attempts, 3);
        assert!(
            db.record_job_failure("work", first.attempt, "boom", 60)
                .await
                .unwrap()
        );

        let job = db.job("work").await.unwrap().unwrap();
        assert_eq!(job.status, "queued");
        assert_eq!(job.attempt, 1);
        assert!(job.next_retry_at.is_some());
        assert!(db.claim_next_job(60).await.unwrap().is_none());

        sqlx::query(
            "UPDATE jobs SET next_retry_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-1 seconds') WHERE work_key = 'work'",
        )
        .execute(&db.pool)
        .await
        .unwrap();
        let second = db.claim_next_job(60).await.unwrap().unwrap();
        assert_eq!(second.attempt, 2);
        db.record_job_failure("work", second.attempt, "boom again", 0)
            .await
            .unwrap();
        let third = db.claim_next_job(60).await.unwrap().unwrap();
        assert_eq!(third.attempt, 3);
        db.record_job_failure("work", third.attempt, "terminal", 0)
            .await
            .unwrap();

        let job = db.job("work").await.unwrap().unwrap();
        assert_eq!(job.status, "failed");
        assert_eq!(job.next_retry_at, None);
        assert_eq!(job.error_summary.as_deref(), Some("terminal"));
    }

    #[tokio::test]
    async fn failed_jobs_can_be_retried_by_work_key() {
        let db = test_database().await;
        let payload = r#"{"kind":"parameter_refresh","model_slug":"demo"}"#;

        db.enqueue_job("work", "parameter_refresh", payload)
            .await
            .unwrap();
        let job = db.claim_next_job(60).await.unwrap().unwrap();
        db.finish_job("work", job.attempt, "failed", Some("boom"))
            .await
            .unwrap();

        assert!(db.retry_failed_job("work").await.unwrap());
        let job = db.job("work").await.unwrap().unwrap();
        assert_eq!(job.status, "queued");
        assert_eq!(job.error_summary, None);
        assert!(!db.retry_failed_job("missing").await.unwrap());
    }

    #[tokio::test]
    async fn failed_jobs_can_be_retried_by_kind() {
        let db = test_database().await;
        let payload = r#"{"kind":"parameter_refresh","model_slug":"demo"}"#;
        for (work_key, job_kind) in [
            ("parameters", "parameter_refresh"),
            ("preview", "preview_export"),
            ("download", "download_export"),
        ] {
            db.enqueue_job(work_key, job_kind, payload).await.unwrap();
            let job = db.claim_next_job(60).await.unwrap().unwrap();
            db.finish_job(work_key, job.attempt, "failed", Some("boom"))
                .await
                .unwrap();
        }

        let count = db
            .retry_failed_jobs_by_kind("preview_export")
            .await
            .unwrap();

        assert_eq!(count, 1);
        assert_eq!(db.job("preview").await.unwrap().unwrap().status, "queued");
        assert_eq!(
            db.job("parameters").await.unwrap().unwrap().status,
            "failed"
        );
        assert_eq!(db.job("download").await.unwrap().unwrap().status, "failed");
    }

    #[tokio::test]
    async fn running_jobs_are_reclaimed_after_lease_expiry() {
        let db = test_database().await;
        let payload = r#"{"kind":"parameter_refresh","model_slug":"demo"}"#;

        assert!(
            db.enqueue_job("work", "parameter_refresh", payload)
                .await
                .unwrap()
        );
        let first_lease = db.claim_next_job(0).await.unwrap().unwrap();
        assert_eq!(first_lease.work_key, "work");

        let second_lease = db.claim_next_job(60).await.unwrap().unwrap();
        assert_eq!(second_lease.work_key, "work");
        assert_eq!(second_lease.attempt, first_lease.attempt + 1);

        assert!(
            !db.finish_job("work", first_lease.attempt, "ready", None)
                .await
                .unwrap()
        );
        assert!(
            db.finish_job("work", second_lease.attempt, "ready", None)
                .await
                .unwrap()
        );

        assert!(db.claim_next_job(0).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn running_job_leases_do_not_block_other_writes() {
        let db = test_database().await;
        let payload = r#"{"kind":"parameter_refresh","model_slug":"demo"}"#;

        db.enqueue_job("slow", "parameter_refresh", payload)
            .await
            .unwrap();
        let slow_lease = db.claim_next_job(60).await.unwrap().unwrap();
        let slow_finish = {
            let db = db.clone();
            let work_key = slow_lease.work_key.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(200)).await;
                db.finish_job(&work_key, slow_lease.attempt, "ready", None)
                    .await
                    .unwrap()
            })
        };

        let enqueued = tokio::time::timeout(Duration::from_millis(500), async {
            db.enqueue_job("other", "parameter_refresh", payload).await
        })
        .await
        .expect("enqueue blocked while slow job work was in progress")
        .unwrap();

        assert!(enqueued);
        assert_eq!(
            db.claim_next_job(60).await.unwrap().unwrap().work_key,
            "other"
        );
        assert!(slow_finish.await.unwrap());
    }

    #[tokio::test]
    async fn database_can_be_backed_up_to_new_file() {
        let db = test_database().await;
        db.enqueue_job(
            "work",
            "parameter_refresh",
            r#"{"kind":"parameter_refresh","model_slug":"demo"}"#,
        )
        .await
        .unwrap();
        let backup_directory = tempfile::tempdir().unwrap();
        let backup_path = backup_directory.path().join("backup.db");

        db.backup_to_path(&backup_path).await.unwrap();

        let backup_url = format!("sqlite://{}?mode=rw", backup_path.display());
        let backup = Database::connect(&backup_url).await.unwrap();
        let job = backup.job("work").await.unwrap().unwrap();
        assert_eq!(job.status, "queued");
    }

    #[tokio::test]
    async fn source_resolutions_round_trip_by_version_identity() {
        let db = test_database().await;
        db.upsert_source_resolution(SourceResolutionUpsert {
            source_hash: "sourcehash",
            model_slug: "demo",
            document_id: "did",
            version_id: "vid",
            microversion_id: "mid",
            element_id: "eid",
            element_kind: "part_studio",
            link_document_id: None,
            diagnostics_json: r#"{"microversionId":"mid"}"#,
        })
        .await
        .unwrap();

        let by_hash = db.source_resolution("sourcehash").await.unwrap().unwrap();
        assert_eq!(by_hash.microversion_id, "mid");

        let by_version = db
            .source_resolution_for_version("did", "vid", "eid", "part_studio", None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(by_version.source_hash, "sourcehash");

        db.upsert_source_resolution(SourceResolutionUpsert {
            source_hash: "sourcehash",
            model_slug: "demo",
            document_id: "did",
            version_id: "vid-alias",
            microversion_id: "mid",
            element_id: "eid",
            element_kind: "part_studio",
            link_document_id: None,
            diagnostics_json: r#"{"microversionId":"mid"}"#,
        })
        .await
        .unwrap();

        let first_alias = db
            .source_resolution_for_version("did", "vid", "eid", "part_studio", None)
            .await
            .unwrap()
            .unwrap();
        let second_alias = db
            .source_resolution_for_version("did", "vid-alias", "eid", "part_studio", None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first_alias.source_hash, "sourcehash");
        assert_eq!(second_alias.source_hash, "sourcehash");
    }

    #[tokio::test]
    async fn parameter_metadata_is_keyed_by_source_hash() {
        let db = test_database().await;
        db.upsert_parameter_metadata(
            "sourcehash",
            "onshape/source/v2/sourcehash/configuration.raw.json",
            "onshape/source/v2/sourcehash/parameters.normalized/schemahash.json",
            "schemahash",
            2,
        )
        .await
        .unwrap();

        let record = db.parameter_metadata("sourcehash").await.unwrap().unwrap();
        assert_eq!(record.schema_hash, "schemahash");
        assert_eq!(record.schema_version, 2);
    }

    #[tokio::test]
    async fn configuration_selections_round_trip_typed_values() {
        let db = test_database().await;
        db.upsert_configuration_selection(ConfigurationSelectionUpsert {
            source_hash: "sourcehash",
            config_hash: "confighash",
            values_json: r#"{"enabled":{"kind":"boolean","value":true}}"#,
            validation_json: r#"{"parameterSchemaVersion":3,"requestValues":{"enabled":"true"}}"#,
        })
        .await
        .unwrap();

        let record = db
            .configuration_selection("sourcehash", "confighash")
            .await
            .unwrap()
            .unwrap();
        assert!(record.values_json.contains(r#""kind":"boolean""#));
        assert!(record.validation_json.contains("parameterSchemaVersion"));
    }

    #[tokio::test]
    async fn configuration_encodings_round_trip_by_source_and_config_hash() {
        let db = test_database().await;
        db.upsert_configuration_encoding(ConfigurationEncodingUpsert {
            source_hash: "sourcehash",
            config_hash: "confighash",
            encoded_id: "encoded-1",
            query_param: "configuration=encoded-1",
            request_json: r#"{"parameters":[{"parameterId":"enabled","parameterValue":"true"}]}"#,
            response_json: r#"{"encodedId":"encoded-1","queryParam":"configuration=encoded-1"}"#,
        })
        .await
        .unwrap();

        let record = db
            .configuration_encoding("sourcehash", "confighash")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(record.encoded_id, "encoded-1");
        assert_eq!(record.query_param, "configuration=encoded-1");

        db.upsert_configuration_encoding(ConfigurationEncodingUpsert {
            source_hash: "sourcehash",
            config_hash: "confighash",
            encoded_id: "encoded-2",
            query_param: "configuration=encoded-2",
            request_json: r#"{"parameters":[{"parameterId":"enabled","parameterValue":"false"}]}"#,
            response_json: r#"{"encodedId":"encoded-2","queryParam":"configuration=encoded-2"}"#,
        })
        .await
        .unwrap();

        let updated = db
            .configuration_encoding("sourcehash", "confighash")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.encoded_id, "encoded-2");
        assert!(updated.request_json.contains("false"));
    }

    #[tokio::test]
    async fn export_requests_dedupe_by_request_hash() {
        let db = test_database().await;
        let inserted = db
            .insert_export_request_if_absent(ExportRequestInsert {
                request_hash: "requesthash",
                source_hash: "sourcehash",
                config_hash: "confighash",
                options_hash: "optionshash",
                output_kind: "preview",
                format: "glb",
                endpoint: "createPartStudioExportGltf",
                method: "POST",
                path: "/api/partstudios/d/did/v/vid/e/eid/export/gltf",
                request_json: "{}",
                defaults_policy_version: "v1",
                request_builder_version: "v1",
                status: "queued",
            })
            .await
            .unwrap();
        assert!(inserted);
        assert!(
            !db.insert_export_request_if_absent(ExportRequestInsert {
                request_hash: "requesthash",
                source_hash: "sourcehash",
                config_hash: "confighash",
                options_hash: "different-optionshash",
                output_kind: "preview",
                format: "glb",
                endpoint: "createPartStudioExportGltf",
                method: "POST",
                path: "/api/partstudios/d/did/v/vid/e/eid/export/gltf",
                request_json: r#"{"different":true}"#,
                defaults_policy_version: "v2",
                request_builder_version: "v2",
                status: "ready",
            })
            .await
            .unwrap()
        );

        let record = db.export_request("requesthash").await.unwrap().unwrap();
        assert_eq!(record.options_hash, "optionshash");
        assert_eq!(record.request_json, "{}");
        assert_eq!(record.defaults_policy_version, "v1");
        assert_eq!(record.request_builder_version, "v1");
        assert_eq!(record.status, "queued");
    }

    #[tokio::test]
    async fn latest_export_request_for_output_prefers_newest_request() {
        let db = test_database().await;

        db.insert_export_request_if_absent(ExportRequestInsert {
            request_hash: "older-requesthash",
            source_hash: "sourcehash",
            config_hash: "confighash",
            options_hash: "optionshash",
            output_kind: "preview",
            format: "glb",
            endpoint: "createPartStudioExportGltf",
            method: "POST",
            path: "/api/partstudios/d/did/v/vid/e/eid/export/gltf",
            request_json: "{}",
            defaults_policy_version: "v1",
            request_builder_version: "v1",
            status: "staged",
        })
        .await
        .unwrap();
        sqlx::query(
            "UPDATE export_requests SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-1 day') WHERE request_hash = 'older-requesthash'",
        )
        .execute(&db.pool)
        .await
        .unwrap();

        db.insert_export_request_if_absent(ExportRequestInsert {
            request_hash: "newer-requesthash",
            source_hash: "sourcehash",
            config_hash: "confighash",
            options_hash: "optionshash-2",
            output_kind: "preview",
            format: "glb",
            endpoint: "createPartStudioExportGltf",
            method: "POST",
            path: "/api/partstudios/d/did/v/vid/e/eid/export/gltf",
            request_json: "{}",
            defaults_policy_version: "v2",
            request_builder_version: "v2",
            status: "staged",
        })
        .await
        .unwrap();

        let record = db
            .latest_export_request_for_output(
                "sourcehash",
                "confighash",
                "optionshash-2",
                "preview",
                "glb",
            )
            .await
            .unwrap()
            .unwrap();

        assert_eq!(record.request_hash, "newer-requesthash");
        assert_eq!(record.request_builder_version, "v2");
    }

    #[tokio::test]
    async fn latest_export_request_for_output_filters_by_options_hash() {
        let db = test_database().await;

        for (request_hash, options_hash) in [
            ("preview-old", "optionshash-1"),
            ("preview-new", "optionshash-2"),
        ] {
            db.insert_export_request_if_absent(ExportRequestInsert {
                request_hash,
                source_hash: "sourcehash",
                config_hash: "confighash",
                options_hash,
                output_kind: "preview",
                format: "glb",
                endpoint: "createPartStudioExportGltf",
                method: "POST",
                path: "/api/partstudios/d/did/v/vid/e/eid/export/gltf",
                request_json: "{}",
                defaults_policy_version: "v1",
                request_builder_version: "v1",
                status: "staged",
            })
            .await
            .unwrap();
        }

        let record = db
            .latest_export_request_for_output(
                "sourcehash",
                "confighash",
                "optionshash-2",
                "preview",
                "glb",
            )
            .await
            .unwrap()
            .unwrap();

        assert_eq!(record.request_hash, "preview-new");
    }

    #[tokio::test]
    async fn raw_payload_insert_dedupes_by_hash() {
        let db = test_database().await;

        assert!(
            db.insert_raw_payload_if_absent(RawPayloadInsert {
                raw_payload_hash: "rawhash",
                object_key: "onshape/raw/v2/ra/rawhash/payload.bin",
                content_type: Some("application/octet-stream"),
                byte_len: 4,
                headers_json: "{}",
                original_filename: None,
                filename_source: None,
                detected_kind: "binary",
                zip_manifest_json: None,
            })
            .await
            .unwrap()
        );

        assert!(
            !db.insert_raw_payload_if_absent(RawPayloadInsert {
                raw_payload_hash: "rawhash",
                object_key: "onshape/raw/v2/ra/rawhash/payload.bin",
                content_type: Some("application/octet-stream"),
                byte_len: 4,
                headers_json: "{}",
                original_filename: None,
                filename_source: None,
                detected_kind: "binary",
                zip_manifest_json: None,
            })
            .await
            .unwrap()
        );

        assert_eq!(
            db.raw_payload("rawhash")
                .await
                .unwrap()
                .unwrap()
                .raw_payload_hash,
            "rawhash"
        );
    }

    #[tokio::test]
    async fn translation_raw_payload_and_artifact_set_round_trip() {
        let db = test_database().await;

        db.insert_translation_start(TranslationStartInsert {
            translation_id: "tid",
            request_hash: "requesthash",
            state: "ACTIVE",
            start_response_json: r#"{"id":"tid"}"#,
        })
        .await
        .unwrap();
        assert!(
            db.update_translation_final(TranslationFinalUpdate {
                translation_id: "tid",
                state: "DONE",
                final_response_json: r#"{"requestState":"DONE"}"#,
                poll_state_json: r#"{"requestState":"DONE"}"#,
                result_external_data_ids_json: r#"["fid"]"#,
                result_element_ids_json: "[]",
                response_hash: Some("responsehash"),
                failure_reason: None,
            })
            .await
            .unwrap()
        );
        assert_eq!(db.translation("tid").await.unwrap().unwrap().state, "DONE");

        assert!(
            db.insert_raw_payload_if_absent(RawPayloadInsert {
                raw_payload_hash: "rawhash",
                object_key: "onshape/raw/v2/ra/rawhash/payload.bin",
                content_type: Some("application/zip"),
                byte_len: 42,
                headers_json: "{}",
                original_filename: Some("payload.zip"),
                filename_source: Some("content-disposition"),
                detected_kind: "zip",
                zip_manifest_json: Some("[]"),
            })
            .await
            .unwrap()
        );
        assert!(
            db.link_raw_payload_source(RawPayloadSourceInsert {
                request_hash: "requesthash",
                translation_id: Some("tid"),
                external_data_id: Some("fid"),
                result_index: Some(0),
                response_headers_json: "{}",
                etag: Some("etag"),
                raw_payload_hash: "rawhash",
            })
            .await
            .unwrap()
        );
        assert_eq!(
            db.raw_payload_hash_for_source("requesthash", Some("tid"), Some("fid"), Some(0))
                .await
                .unwrap()
                .as_deref(),
            Some("rawhash")
        );

        assert!(
            db.insert_postprocess_run_if_absent(PostprocessRunInsert {
                postprocess_hash: "posthash",
                raw_payload_hash: "rawhash",
                processor_name: "preview_extract",
                processor_version: "1",
                policy_json: r#"{"acceptedInputShapes":["direct_glb"]}"#,
                status: "staged",
                log_json: "[]",
                derived_files_json: "[]",
            })
            .await
            .unwrap()
        );
        assert!(
            db.transition_postprocess_run_status(
                "posthash",
                "ready",
                r#"[{"level":"info","message":"done"}]"#,
                r#"[{"role":"viewer_entry","logicalPath":"preview.glb"}]"#,
            )
            .await
            .unwrap()
        );
        let postprocess = db.postprocess_run("posthash").await.unwrap().unwrap();
        assert_eq!(postprocess.status, "ready");
        assert!(postprocess.derived_files_json.contains("preview.glb"));

        assert!(
            db.insert_artifact_set_if_absent(ArtifactSetInsert {
                artifact_set_hash: "artifactsethash",
                source_hash: "sourcehash",
                config_hash: "confighash",
                options_hash: "optionshash",
                request_hash: Some("requesthash"),
                raw_payload_hash: Some("rawhash"),
                postprocess_hash: Some("posthash"),
                output_kind: "preview",
                format: "gltf_asset_set",
                status: "staged",
                primary_object_key: None,
                metadata_json: "{}",
            })
            .await
            .unwrap()
        );
        db.replace_artifact_files(
            "artifactsethash",
            &[ArtifactFileInsert {
                artifact_set_hash: "artifactsethash",
                role: "viewer_entry",
                logical_path: "scene/model.gltf",
                original_path: Some("scene/model.gltf"),
                object_key: "previews/v2/artifactsethash/scene/model.gltf",
                content_type: "model/gltf+json",
                byte_len: 12,
                sha256: "filehash",
                metadata_json: "{}",
            }],
        )
        .await
        .unwrap();
        assert!(
            db.mark_artifact_set_ready(
                "artifactsethash",
                "previews/v2/artifactsethash/scene/model.gltf"
            )
            .await
            .unwrap()
        );
        assert!(
            db.supersede_artifact_set("artifactsethash", Some("newset"), Some("replaced"))
                .await
                .unwrap()
        );
        assert_eq!(
            db.artifact_set("artifactsethash")
                .await
                .unwrap()
                .unwrap()
                .status,
            "superseded"
        );
    }

    #[tokio::test]
    async fn database_backup_refuses_existing_file() {
        let db = test_database().await;
        let backup_directory = tempfile::tempdir().unwrap();
        let backup_path = backup_directory.path().join("backup.db");

        db.backup_to_path(&backup_path).await.unwrap();
        let error = db.backup_to_path(&backup_path).await.unwrap_err();

        assert!(error.to_string().contains("already exists"));
    }

    #[tokio::test]
    async fn latest_translation_for_request_prefers_most_recent_record() {
        let db = test_database().await;

        db.insert_translation_start(TranslationStartInsert {
            translation_id: "older",
            request_hash: "requesthash",
            state: "ACTIVE",
            start_response_json: r#"{"id":"older"}"#,
        })
        .await
        .unwrap();
        sqlx::query(
            "UPDATE translations SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-1 day') WHERE translation_id = 'older'",
        )
        .execute(&db.pool)
        .await
        .unwrap();

        db.insert_translation_start(TranslationStartInsert {
            translation_id: "newer",
            request_hash: "requesthash",
            state: "ACTIVE",
            start_response_json: r#"{"id":"newer"}"#,
        })
        .await
        .unwrap();

        let record = db
            .latest_translation_for_request("requesthash")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(record.translation_id, "newer");
    }

    #[tokio::test]
    async fn latest_completed_translation_for_request_prefers_done_record() {
        let db = test_database().await;

        db.insert_translation_start(TranslationStartInsert {
            translation_id: "done",
            request_hash: "requesthash",
            state: "ACTIVE",
            start_response_json: r#"{"id":"done"}"#,
        })
        .await
        .unwrap();
        db.update_translation_final(TranslationFinalUpdate {
            translation_id: "done",
            state: "DONE",
            final_response_json: r#"{"requestState":"DONE"}"#,
            poll_state_json: r#"{"requestState":"DONE"}"#,
            result_external_data_ids_json: r#"["fid"]"#,
            result_element_ids_json: "[]",
            response_hash: Some("responsehash"),
            failure_reason: None,
        })
        .await
        .unwrap();

        db.insert_translation_start(TranslationStartInsert {
            translation_id: "active",
            request_hash: "requesthash",
            state: "ACTIVE",
            start_response_json: r#"{"id":"active"}"#,
        })
        .await
        .unwrap();

        let record = db
            .latest_completed_translation_for_request("requesthash")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(record.translation_id, "done");
    }

    #[tokio::test]
    async fn staged_artifact_sets_are_not_ready_until_explicitly_marked_ready() {
        let db = test_database().await;

        db.stage_artifact(
            test_artifact_upsert(
                "artifact",
                "abc",
                "preview_glb",
                "previews/v2/artifact/preview.glb",
                "model/gltf-binary",
                10,
            ),
            &[ArtifactFileInsert {
                artifact_set_hash: "artifact",
                role: "viewer_entry",
                logical_path: "preview.glb",
                original_path: Some("preview.glb"),
                object_key: "previews/v2/artifact/preview.glb",
                content_type: "model/gltf-binary",
                byte_len: 10,
                sha256: "abc123",
                metadata_json: "{}",
            }],
        )
        .await
        .unwrap();

        assert!(db.artifact("artifact").await.unwrap().is_none());
        assert_eq!(
            db.artifact_set("artifact").await.unwrap().unwrap().status,
            "staged"
        );

        assert!(
            db.mark_artifact_set_ready("artifact", "previews/v2/artifact/preview.glb")
                .await
                .unwrap()
        );

        assert_eq!(
            db.artifact("artifact").await.unwrap().unwrap().status,
            "ready"
        );
    }

    #[tokio::test]
    async fn upload_failed_artifact_sets_remain_non_ready() {
        let db = test_database().await;

        db.stage_artifact(
            test_artifact_upsert(
                "artifact",
                "abc",
                "step",
                "artifacts/v2/artifact/demo.step",
                "model/step",
                10,
            ),
            &[ArtifactFileInsert {
                artifact_set_hash: "artifact",
                role: "download",
                logical_path: "demo.step",
                original_path: Some("demo.step"),
                object_key: "artifacts/v2/artifact/demo.step",
                content_type: "model/step",
                byte_len: 10,
                sha256: "abc123",
                metadata_json: "{}",
            }],
        )
        .await
        .unwrap();

        assert!(
            db.transition_artifact_set_status("artifact", "upload_failed")
                .await
                .unwrap()
        );

        assert!(db.artifact("artifact").await.unwrap().is_none());
        assert_eq!(
            db.artifact_set("artifact").await.unwrap().unwrap().status,
            "upload_failed"
        );
    }

    #[tokio::test]
    async fn clearing_generated_state_preserves_catalog() {
        let db = test_database().await;
        let catalog = Catalog::from_models(vec![test_catalog_model("demo", "did")]).unwrap();
        db.replace_catalog(&catalog).await.unwrap();
        db.enqueue_job(
            "work",
            "parameter_refresh",
            r#"{"kind":"parameter_refresh","model_slug":"demo"}"#,
        )
        .await
        .unwrap();
        db.upsert_source_resolution(SourceResolutionUpsert {
            source_hash: "sourcehash",
            model_slug: "demo",
            document_id: "did",
            version_id: "vid",
            microversion_id: "mid",
            element_id: "eid",
            element_kind: "part_studio",
            link_document_id: None,
            diagnostics_json: r#"{"microversionId":"mid"}"#,
        })
        .await
        .unwrap();
        db.upsert_parameter_metadata(
            "sourcehash",
            "onshape/source/v2/sourcehash/configuration.raw.json",
            "onshape/source/v2/sourcehash/parameters.normalized/schemahash.json",
            "schemahash",
            2,
        )
        .await
        .unwrap();
        db.upsert_artifact(test_artifact_upsert(
            "artifact",
            "confighash",
            "step",
            "artifacts/v2/artifact/demo.step",
            "model/step",
            10,
        ))
        .await
        .unwrap();

        let deleted = db.clear_generated_state().await.unwrap();

        assert!(deleted.iter().any(|entry| entry.rows > 0));
        assert_eq!(db.catalog().await.unwrap().models().len(), 1);
        assert!(db.job("work").await.unwrap().is_none());
        assert!(db.parameter_metadata("sourcehash").await.unwrap().is_none());
        assert!(db.artifact("artifact").await.unwrap().is_none());
        assert!(
            db.source_resolution_for_version("did", "vid", "eid", "part_studio", None)
                .await
                .unwrap()
                .is_none()
        );
    }

    async fn test_database() -> Database {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("test.db");
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let db = Database::connect(&url).await.unwrap();
        std::mem::forget(directory);
        db
    }

    fn test_catalog_model(slug: &str, document_id: &str) -> Model {
        Model {
            catalog_schema_version: crate::catalog::CATALOG_SCHEMA_VERSION,
            entry_version: crate::catalog::CATALOG_ENTRY_VERSION,
            slug: slug.to_owned(),
            name: "Name".to_owned(),
            description: "Description".to_owned(),
            published: true,
            tags: Vec::new(),
            thumbnail: None,
            onshape: OnshapeSource {
                document_id: document_id.to_owned(),
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
                preview_options: PreviewOptions {
                    resolution: PreviewResolution::Fine,
                },
                download_options: DownloadOptions {
                    step_version_string: StepVersionString::Ap242,
                    stl: StlDownloadOptions {
                        resolution: MeshResolution::Fine,
                        stl_mode: StlMode::Binary,
                    },
                    three_mf: ThreeMfDownloadOptions {
                        resolution: MeshResolution::Fine,
                    },
                },
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

    fn test_artifact_upsert<'a>(
        artifact_key: &'a str,
        config_hash: &'a str,
        output_kind: &'a str,
        object_key: &'a str,
        content_type: &'a str,
        byte_len: i64,
    ) -> ArtifactUpsert<'a> {
        ArtifactUpsert {
            artifact_key,
            model_slug: "demo",
            config_hash,
            output_kind,
            format: if output_kind == "preview_glb" {
                "glb"
            } else {
                output_kind
            },
            object_key,
            content_type,
            byte_len,
            sha256: "abc123",
            producing_job_key: Some("work"),
            source_hash: "sourcehash",
            options_hash: "optionshash",
            request_hash: None,
            raw_payload_hash: None,
            postprocess_hash: None,
            parameter_schema_version: 1,
            config_values_json: r#"{"width":"10"}"#,
        }
    }
}
