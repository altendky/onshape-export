use std::path::Path;

use serde::Serialize;
use sqlx::{Executor, Row, SqlitePool, sqlite::SqlitePoolOptions};

#[derive(Debug, Clone)]
pub struct ParameterMetadataRecord {
    pub raw_object_key: String,
    pub normalized_object_key: String,
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

#[derive(Debug, Clone, Copy)]
pub struct ArtifactUpsert<'a> {
    pub artifact_key: &'a str,
    pub model_slug: &'a str,
    pub config_hash: &'a str,
    pub output_kind: &'a str,
    pub object_key: &'a str,
    pub content_type: &'a str,
    pub byte_len: i64,
    pub sha256: &'a str,
    pub producing_job_key: Option<&'a str>,
    pub source_hash: &'a str,
    pub options_hash: &'a str,
    pub parameter_schema_version: i64,
    pub config_values_json: &'a str,
}

#[derive(Debug, Clone)]
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    pub async fn connect(database_url: &str) -> sqlx::Result<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await?;

        Self::apply_pragmas(&pool).await?;
        sqlx::migrate!().run(&pool).await?;

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

    pub async fn parameter_metadata(
        &self,
        model_slug: &str,
    ) -> sqlx::Result<Option<ParameterMetadataRecord>> {
        sqlx::query(
            "SELECT raw_object_key, normalized_object_key FROM parameter_metadata WHERE model_slug = ?",
        )
            .bind(model_slug)
            .fetch_optional(&self.pool)
            .await
            .map(|row| {
                row.map(|row| ParameterMetadataRecord {
                    raw_object_key: row.get("raw_object_key"),
                    normalized_object_key: row.get("normalized_object_key"),
                })
            })
    }

    pub async fn upsert_parameter_metadata(
        &self,
        model_slug: &str,
        raw_object_key: &str,
        normalized_object_key: &str,
    ) -> sqlx::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO parameter_metadata (model_slug, raw_object_key, normalized_object_key)
            VALUES (?, ?, ?)
            ON CONFLICT(model_slug) DO UPDATE SET
                raw_object_key = excluded.raw_object_key,
                normalized_object_key = excluded.normalized_object_key,
                refreshed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            "#,
        )
        .bind(model_slug)
        .bind(raw_object_key)
        .bind(normalized_object_key)
        .execute(&self.pool)
        .await?;
        Ok(())
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
            SELECT artifact_key, model_slug, config_hash, output_kind, status, object_key,
                   content_type, byte_len, sha256, producing_job_key, source_hash,
                   options_hash, parameter_schema_version, config_values_json, created_at,
                   superseded_at
            FROM artifacts
            WHERE artifact_key = ? AND status = 'ready'
            "#,
        )
        .bind(artifact_key)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(artifact_record_from_row))
    }

    pub async fn artifacts_for_model(&self, model_slug: &str) -> sqlx::Result<Vec<ArtifactRecord>> {
        sqlx::query(
            r#"
            SELECT artifact_key, model_slug, config_hash, output_kind, status, object_key,
                   content_type, byte_len, sha256, producing_job_key, source_hash,
                   options_hash, parameter_schema_version, config_values_json, created_at,
                   superseded_at
            FROM artifacts
            WHERE model_slug = ? AND status = 'ready'
            ORDER BY created_at DESC, output_kind
            "#,
        )
        .bind(model_slug)
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(artifact_record_from_row).collect())
    }

    pub async fn artifacts_older_than_days(
        &self,
        model_slug: &str,
        days: i64,
    ) -> sqlx::Result<Vec<ArtifactRecord>> {
        sqlx::query(
            r#"
            SELECT artifact_key, model_slug, config_hash, output_kind, status, object_key,
                   content_type, byte_len, sha256, producing_job_key, source_hash,
                   options_hash, parameter_schema_version, config_values_json, created_at,
                   superseded_at
            FROM artifacts
            WHERE model_slug = ?
              AND status = 'ready'
              AND created_at < strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-' || ? || ' days')
            ORDER BY created_at, output_kind
            "#,
        )
        .bind(model_slug)
        .bind(days)
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(artifact_record_from_row).collect())
    }

    pub async fn artifacts_for_configuration(
        &self,
        model_slug: &str,
        config_hash: &str,
    ) -> sqlx::Result<Vec<ArtifactRecord>> {
        sqlx::query(
            r#"
            SELECT artifact_key, model_slug, config_hash, output_kind, status, object_key,
                   content_type, byte_len, sha256, producing_job_key, source_hash,
                   options_hash, parameter_schema_version, config_values_json, created_at,
                   superseded_at
            FROM artifacts
            WHERE model_slug = ? AND config_hash = ? AND status = 'ready'
            ORDER BY output_kind
            "#,
        )
        .bind(model_slug)
        .bind(config_hash)
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(artifact_record_from_row).collect())
    }

    pub async fn supersede_artifact(&self, artifact_key: &str) -> sqlx::Result<bool> {
        let result = sqlx::query(
            r#"
            UPDATE artifacts
            SET status = 'superseded',
                superseded_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            WHERE artifact_key = ? AND status = 'ready'
            "#,
        )
        .bind(artifact_key)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn upsert_artifact(&self, artifact: ArtifactUpsert<'_>) -> sqlx::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO artifacts (
                artifact_key,
                model_slug,
                config_hash,
                output_kind,
                status,
                object_key,
                content_type,
                byte_len,
                sha256,
                producing_job_key,
                source_hash,
                options_hash,
                parameter_schema_version,
                config_values_json
            )
            VALUES (?, ?, ?, ?, 'ready', ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(artifact_key) DO UPDATE SET
                model_slug = excluded.model_slug,
                config_hash = excluded.config_hash,
                output_kind = excluded.output_kind,
                status = 'ready',
                object_key = excluded.object_key,
                content_type = excluded.content_type,
                byte_len = excluded.byte_len,
                sha256 = excluded.sha256,
                producing_job_key = excluded.producing_job_key,
                source_hash = excluded.source_hash,
                options_hash = excluded.options_hash,
                parameter_schema_version = excluded.parameter_schema_version,
                config_values_json = excluded.config_values_json,
                created_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                superseded_at = NULL
            "#,
        )
        .bind(artifact.artifact_key)
        .bind(artifact.model_slug)
        .bind(artifact.config_hash)
        .bind(artifact.output_kind)
        .bind(artifact.object_key)
        .bind(artifact.content_type)
        .bind(artifact.byte_len)
        .bind(artifact.sha256)
        .bind(artifact.producing_job_key)
        .bind(artifact.source_hash)
        .bind(artifact.options_hash)
        .bind(artifact.parameter_schema_version)
        .bind(artifact.config_values_json)
        .execute(&self.pool)
        .await?;
        Ok(())
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
            SELECT output_kind, COUNT(*) AS count, COALESCE(SUM(byte_len), 0) AS byte_len
            FROM artifacts
            WHERE status = 'ready'
            GROUP BY output_kind
            ORDER BY output_kind
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

fn artifact_record_from_row(row: sqlx::sqlite::SqliteRow) -> ArtifactRecord {
    ArtifactRecord {
        artifact_key: row.get("artifact_key"),
        model_slug: row.get("model_slug"),
        config_hash: row.get("config_hash"),
        output_kind: row.get("output_kind"),
        status: row.get("status"),
        object_key: row.get("object_key"),
        content_type: row.get("content_type"),
        byte_len: row.get("byte_len"),
        sha256: row.get("sha256"),
        producing_job_key: row.get("producing_job_key"),
        source_hash: row.get("source_hash"),
        options_hash: row.get("options_hash"),
        parameter_schema_version: row.get("parameter_schema_version"),
        config_values_json: row.get("config_values_json"),
        created_at: row.get("created_at"),
        superseded_at: row.get("superseded_at"),
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
    use std::time::Duration;

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
            "UPDATE artifacts SET created_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-10 days') WHERE artifact_key = 'old'",
        )
        .execute(&db.pool)
        .await
        .unwrap();

        let artifacts = db.artifacts_older_than_days("demo", 7).await.unwrap();

        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].artifact_key, "old");
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
        let retained_status: String =
            sqlx::query_scalar("SELECT status FROM artifacts WHERE artifact_key = 'artifact'")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(retained_status, "superseded");
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
    async fn database_backup_refuses_existing_file() {
        let db = test_database().await;
        let backup_directory = tempfile::tempdir().unwrap();
        let backup_path = backup_directory.path().join("backup.db");

        db.backup_to_path(&backup_path).await.unwrap();
        let error = db.backup_to_path(&backup_path).await.unwrap_err();

        assert!(error.to_string().contains("already exists"));
    }

    async fn test_database() -> Database {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("test.db");
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let db = Database::connect(&url).await.unwrap();
        std::mem::forget(directory);
        db
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
            object_key,
            content_type,
            byte_len,
            sha256: "abc123",
            producing_job_key: Some("work"),
            source_hash: "sourcehash",
            options_hash: "optionshash",
            parameter_schema_version: 1,
            config_values_json: r#"{"width":"10"}"#,
        }
    }
}
