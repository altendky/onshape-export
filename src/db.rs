use sqlx::{Executor, Row, SqlitePool, sqlite::SqlitePoolOptions};

#[derive(Debug, Clone)]
pub struct ParameterMetadataRecord {
    pub normalized_object_key: String,
}

#[derive(Debug, Clone)]
pub struct ArtifactRecord {
    pub object_key: String,
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

    pub async fn try_start_job(&self, work_key: &str, job_kind: &str) -> sqlx::Result<bool> {
        let result = sqlx::query(
            r#"
            INSERT INTO jobs (work_key, job_kind, status)
            VALUES (?, ?, 'running')
            ON CONFLICT(work_key) DO UPDATE SET
                status = 'running',
                error_summary = NULL,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            WHERE jobs.status IN ('failed', 'expired')
            "#,
        )
        .bind(work_key)
        .bind(job_kind)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() == 1)
    }

    pub async fn finish_job(
        &self,
        work_key: &str,
        status: &str,
        error_summary: Option<&str>,
    ) -> sqlx::Result<()> {
        sqlx::query(
            r#"
            UPDATE jobs
            SET status = ?, error_summary = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            WHERE work_key = ?
            "#,
        )
        .bind(status)
        .bind(error_summary)
        .bind(work_key)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn artifact(&self, artifact_key: &str) -> sqlx::Result<Option<ArtifactRecord>> {
        sqlx::query("SELECT object_key FROM artifacts WHERE artifact_key = ?")
            .bind(artifact_key)
            .fetch_optional(&self.pool)
            .await
            .map(|row| {
                row.map(|row| ArtifactRecord {
                    object_key: row.get("object_key"),
                })
            })
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

    async fn apply_pragmas(pool: &SqlitePool) -> sqlx::Result<()> {
        pool.execute("PRAGMA journal_mode = WAL").await?;
        pool.execute("PRAGMA synchronous = FULL").await?;
        pool.execute("PRAGMA foreign_keys = ON").await?;
        pool.execute("PRAGMA busy_timeout = 5000").await?;
        Ok(())
    }
}
