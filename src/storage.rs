use aws_credential_types::Credentials;
use aws_sdk_s3::{
    config::{Builder, Region, SharedCredentialsProvider},
    primitives::ByteStream,
    types::{Delete, ObjectIdentifier},
};
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};

use crate::config::StorageConfig;

#[derive(Debug, Clone)]
pub struct StorageClient {
    client: aws_sdk_s3::Client,
    bucket: String,
    public_base_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ObjectMetadata {
    pub content_length: i64,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct DeletedPrefixSummary {
    pub objects: usize,
}

impl StorageClient {
    pub async fn new(config: StorageConfig) -> anyhow::Result<Self> {
        let mut builder = Builder::new().region(Region::new(config.region.clone()));

        if let Some(endpoint_url) = &config.endpoint_url {
            builder = builder.endpoint_url(endpoint_url);
        }
        if config.force_path_style {
            builder = builder.force_path_style(true);
        }

        if let (Some(access_key_id), Some(secret_access_key)) =
            (&config.access_key_id, &config.secret_access_key)
        {
            builder = builder.credentials_provider(SharedCredentialsProvider::new(
                Credentials::new(access_key_id, secret_access_key, None, None, "environment"),
            ));
        }

        let client = aws_sdk_s3::Client::from_conf(builder.build());

        Ok(Self {
            client,
            bucket: config.bucket,
            public_base_url: config.public_base_url,
        })
    }

    pub fn bucket(&self) -> &str {
        &self.bucket
    }

    pub fn public_base_url(&self) -> Option<&str> {
        self.public_base_url.as_deref()
    }

    pub fn client(&self) -> &aws_sdk_s3::Client {
        &self.client
    }

    pub async fn put_json<T: Serialize>(&self, key: &str, value: &T) -> anyhow::Result<()> {
        let body = serde_json::to_vec(value)?;
        self.put_bytes(key, body, "application/json").await
    }

    pub async fn put_bytes(
        &self,
        key: &str,
        body: Vec<u8>,
        content_type: &str,
    ) -> anyhow::Result<()> {
        self.put_bytes_with_headers(key, body, content_type, None, None)
            .await
    }

    pub async fn put_bytes_with_headers(
        &self,
        key: &str,
        body: Vec<u8>,
        content_type: &str,
        content_disposition: Option<&str>,
        cache_control: Option<&str>,
    ) -> anyhow::Result<()> {
        let mut request = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .content_type(content_type)
            .body(ByteStream::from(body));

        if let Some(content_disposition) = content_disposition {
            request = request.content_disposition(content_disposition);
        }
        if let Some(cache_control) = cache_control {
            request = request.cache_control(cache_control);
        }

        request.send().await?;
        Ok(())
    }

    pub async fn put_file(
        &self,
        key: &str,
        path: &std::path::Path,
        content_type: &str,
    ) -> anyhow::Result<()> {
        self.put_file_with_headers(key, path, content_type, None, None)
            .await
    }

    pub async fn put_file_with_headers(
        &self,
        key: &str,
        path: &std::path::Path,
        content_type: &str,
        content_disposition: Option<&str>,
        cache_control: Option<&str>,
    ) -> anyhow::Result<()> {
        let body = ByteStream::from_path(path).await?;
        let mut request = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .content_type(content_type)
            .body(body);
        if let Some(content_disposition) = content_disposition {
            request = request.content_disposition(content_disposition);
        }
        if let Some(cache_control) = cache_control {
            request = request.cache_control(cache_control);
        }
        request.send().await?;
        Ok(())
    }

    /// Verify metadata and the stored bytes without retaining the object in memory.
    pub async fn verify_exact_object(
        &self,
        key: &str,
        expected_content_type: &str,
        expected_len: u64,
        expected_sha256: &str,
    ) -> anyhow::Result<()> {
        let head = self.head_object(key).await?;
        verify_object_metadata(
            Some(head.content_length),
            head.content_type.as_deref(),
            expected_content_type,
            expected_len,
        )?;
        let output = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await?;
        verify_object_metadata(
            output.content_length(),
            output.content_type(),
            expected_content_type,
            expected_len,
        )?;
        verify_object_body(output.body, expected_len, expected_sha256).await
    }

    pub async fn get_json<T: DeserializeOwned>(&self, key: &str) -> anyhow::Result<T> {
        let output = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await?;
        let bytes = output.body.collect().await?.into_bytes();
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub async fn get_bytes(&self, key: &str) -> anyhow::Result<Vec<u8>> {
        let output = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await?;
        Ok(output.body.collect().await?.into_bytes().to_vec())
    }

    pub async fn head_object(&self, key: &str) -> anyhow::Result<ObjectMetadata> {
        let output = self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await?;
        let content_length = output.content_length.ok_or_else(|| {
            anyhow::anyhow!("missing content_length in HEAD response for key: {key}")
        })?;
        Ok(ObjectMetadata {
            content_length,
            content_type: output.content_type,
        })
    }

    pub fn public_url(&self, key: &str) -> Option<String> {
        self.public_base_url.as_ref().map(|base| {
            format!(
                "{}/{}",
                base.trim_end_matches('/'),
                key.split('/')
                    .map(url_path_segment)
                    .collect::<Vec<_>>()
                    .join("/")
            )
        })
    }

    pub async fn delete_prefix(&self, prefix: &str) -> anyhow::Result<DeletedPrefixSummary> {
        let mut continuation_token = None;
        let mut deleted = 0usize;

        loop {
            let output = self
                .client
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(prefix)
                .set_continuation_token(continuation_token)
                .send()
                .await?;

            let mut objects = Vec::new();
            for object in output.contents() {
                if let Some(key) = object.key() {
                    objects.push(ObjectIdentifier::builder().key(key).build()?);
                }
            }

            if !objects.is_empty() {
                let deleted_count = objects.len();
                let delete = Delete::builder()
                    .set_objects(Some(objects))
                    .quiet(true)
                    .build()?;
                let delete_output = self
                    .client
                    .delete_objects()
                    .bucket(&self.bucket)
                    .delete(delete)
                    .send()
                    .await?;
                let errors = delete_output.errors();
                if !errors.is_empty() {
                    anyhow::bail!(
                        "delete_objects returned {} error(s) for prefix {}",
                        errors.len(),
                        prefix
                    );
                }
                deleted += deleted_count;
            }

            if !output.is_truncated().unwrap_or(false) {
                break;
            }
            continuation_token = output.next_continuation_token().map(ToOwned::to_owned);
        }

        Ok(DeletedPrefixSummary { objects: deleted })
    }
}

fn verify_object_metadata(
    content_length: Option<i64>,
    content_type: Option<&str>,
    expected_content_type: &str,
    expected_len: u64,
) -> anyhow::Result<()> {
    if content_length.and_then(|length| u64::try_from(length).ok()) != Some(expected_len) {
        anyhow::bail!("stored object content length does not match the accepted bytes");
    }
    if content_type != Some(expected_content_type) {
        anyhow::bail!("stored object content type does not match the accepted output");
    }
    Ok(())
}

async fn verify_object_body(
    mut body: ByteStream,
    expected_len: u64,
    expected_sha256: &str,
) -> anyhow::Result<()> {
    let mut length = 0u64;
    let mut hasher = Sha256::new();
    while let Some(chunk) = body.try_next().await? {
        length = length
            .checked_add(u64::try_from(chunk.len())?)
            .ok_or_else(|| anyhow::anyhow!("stored object byte count overflow"))?;
        if length > expected_len {
            anyhow::bail!("stored object exceeds the accepted byte length");
        }
        hasher.update(&chunk);
    }
    if length != expected_len {
        anyhow::bail!("stored object bytes do not match the accepted byte length");
    }
    let sha256: String = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if sha256 != expected_sha256 {
        anyhow::bail!("stored object SHA-256 does not match the accepted bytes");
    }
    Ok(())
}

fn url_path_segment(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![byte as char]
            }
            byte => format!("%{byte:02X}").chars().collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_public_url_segments() {
        assert_eq!(url_path_segment("a b.glb"), "a%20b.glb");
    }

    #[test]
    fn exact_object_metadata_requires_both_length_and_content_type() {
        assert!(
            verify_object_metadata(Some(3), Some("application/zip"), "application/zip", 3).is_ok()
        );
        for length in [None, Some(-1), Some(2), Some(4)] {
            assert!(
                verify_object_metadata(length, Some("application/zip"), "application/zip", 3)
                    .is_err()
            );
        }
        for content_type in [None, Some("application/octet-stream")] {
            assert!(verify_object_metadata(Some(3), content_type, "application/zip", 3).is_err());
        }
    }

    #[tokio::test]
    async fn exact_object_bytes_require_length_and_digest() {
        let expected_sha256 = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert!(
            verify_object_body(ByteStream::from_static(b"abc"), 3, expected_sha256)
                .await
                .is_ok()
        );
        for bytes in [b"ab".as_slice(), b"abcd".as_slice(), b"bad".as_slice()] {
            assert!(
                verify_object_body(ByteStream::from(bytes.to_vec()), 3, expected_sha256)
                    .await
                    .is_err()
            );
        }
    }
}
