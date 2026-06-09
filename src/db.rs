use serde::Serialize;
use sqlx::{Executor, Row, SqlitePool, sqlite::SqlitePoolOptions};

#[derive(Debug, Clone)]
pub struct ParameterMetadataRecord {
    pub normalized_object_key: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArtifactRecord {
    pub artifact_key: String,
    pub model_slug: String,
    pub config_hash: String,
    pub output_kind: String,
    pub object_key: String,
    pub content_type: String,
    pub byte_len: Option<i64>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct JobRecord {
    pub work_key: String,
    pub job_kind: String,
    pub status: String,
    pub error_summary: Option<String>,
    pub attempt: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct JobLease {
    pub work_key: String,
    pub job_kind: String,
    pub payload_json: String,
    pub attempt: i64,
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

    pub async fn parameter_metadata(
        &self,
        model_slug: &str,
    ) -> sqlx::Result<Option<ParameterMetadataRecord>> {
        sqlx::query("SELECT normalized_object_key FROM parameter_metadata WHERE model_slug = ?")
            .bind(model_slug)
            .fetch_optional(&self.pool)
            .await
            .map(|row| {
                row.map(|row| ParameterMetadataRecord {
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
        let result = sqlx::query(
            r#"
            INSERT INTO jobs (work_key, job_kind, status, payload_json)
            VALUES (?, ?, 'queued', ?)
            ON CONFLICT(work_key) DO UPDATE SET
                status = 'queued',
                payload_json = excluded.payload_json,
                error_summary = NULL,
                lease_until = NULL,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            WHERE jobs.status IN ('ready', 'failed', 'expired')
            "#,
        )
        .bind(work_key)
        .bind(job_kind)
        .bind(payload_json)
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
                WHERE status IN ('queued', 'expired')
                   OR (status = 'running' AND lease_until <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                ORDER BY CASE WHEN status = 'queued' THEN 0 ELSE 1 END, created_at
                LIMIT 1
            )
            RETURNING work_key, job_kind, payload_json, attempt
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

    pub async fn artifact(&self, artifact_key: &str) -> sqlx::Result<Option<ArtifactRecord>> {
        sqlx::query(
            r#"
            SELECT artifact_key, model_slug, config_hash, output_kind, object_key,
                   content_type, byte_len, created_at
            FROM artifacts
            WHERE artifact_key = ?
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
            SELECT artifact_key, model_slug, config_hash, output_kind, object_key,
                   content_type, byte_len, created_at
            FROM artifacts
            WHERE model_slug = ?
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
            SELECT artifact_key, model_slug, config_hash, output_kind, object_key,
                   content_type, byte_len, created_at
            FROM artifacts
            WHERE model_slug = ?
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
            SELECT artifact_key, model_slug, config_hash, output_kind, object_key,
                   content_type, byte_len, created_at
            FROM artifacts
            WHERE model_slug = ? AND config_hash = ?
            ORDER BY output_kind
            "#,
        )
        .bind(model_slug)
        .bind(config_hash)
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(artifact_record_from_row).collect())
    }

    pub async fn delete_artifact(&self, artifact_key: &str) -> sqlx::Result<bool> {
        let result = sqlx::query("DELETE FROM artifacts WHERE artifact_key = ?")
            .bind(artifact_key)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn upsert_artifact(&self, artifact: ArtifactUpsert<'_>) -> sqlx::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO artifacts (
                artifact_key,
                model_slug,
                config_hash,
                output_kind,
                object_key,
                content_type,
                byte_len
            )
            VALUES (?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(artifact_key) DO UPDATE SET
                object_key = excluded.object_key,
                content_type = excluded.content_type,
                byte_len = excluded.byte_len
            "#,
        )
        .bind(artifact.artifact_key)
        .bind(artifact.model_slug)
        .bind(artifact.config_hash)
        .bind(artifact.output_kind)
        .bind(artifact.object_key)
        .bind(artifact.content_type)
        .bind(artifact.byte_len)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn failed_jobs(&self, limit: i64) -> sqlx::Result<Vec<JobRecord>> {
        sqlx::query(
            r#"
            SELECT work_key, job_kind, status, error_summary, attempt, created_at, updated_at
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

    pub async fn job(&self, work_key: &str) -> sqlx::Result<Option<JobRecord>> {
        sqlx::query(
            r#"
            SELECT work_key, job_kind, status, error_summary, attempt, created_at, updated_at
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
                lease_until = NULL,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            WHERE status = 'failed'
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
                lease_until = NULL,
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
                lease_until = NULL,
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
        object_key: row.get("object_key"),
        content_type: row.get("content_type"),
        byte_len: row.get("byte_len"),
        created_at: row.get("created_at"),
    }
}

fn job_record_from_row(row: sqlx::sqlite::SqliteRow) -> JobRecord {
    JobRecord {
        work_key: row.get("work_key"),
        job_kind: row.get("job_kind"),
        status: row.get("status"),
        error_summary: row.get("error_summary"),
        attempt: row.get("attempt"),
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

    #[tokio::test]
    async fn ready_jobs_can_be_requeued_after_artifact_invalidation() {
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
            db.enqueue_job("work", "parameter_refresh", payload)
                .await
                .unwrap()
        );
        let job = db.claim_next_job(60).await.unwrap().unwrap();
        assert_eq!(job.work_key, "work");
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
    async fn artifacts_can_be_selected_by_age() {
        let db = test_database().await;
        db.upsert_artifact(ArtifactUpsert {
            artifact_key: "old",
            model_slug: "demo",
            config_hash: "abc",
            output_kind: "preview_glb",
            object_key: "previews/demo/old.glb",
            content_type: "model/gltf-binary",
            byte_len: 10,
        })
        .await
        .unwrap();
        db.upsert_artifact(ArtifactUpsert {
            artifact_key: "new",
            model_slug: "demo",
            config_hash: "def",
            output_kind: "step",
            object_key: "artifacts/demo/new.step",
            content_type: "model/step",
            byte_len: 20,
        })
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
            ("preview", "preview_glb"),
            ("download", "download_export"),
        ] {
            db.enqueue_job(work_key, job_kind, payload).await.unwrap();
            let job = db.claim_next_job(60).await.unwrap().unwrap();
            db.finish_job(work_key, job.attempt, "failed", Some("boom"))
                .await
                .unwrap();
        }

        let count = db.retry_failed_jobs_by_kind("preview_glb").await.unwrap();

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

    async fn test_database() -> Database {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("test.db");
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let db = Database::connect(&url).await.unwrap();
        std::mem::forget(directory);
        db
    }
}
