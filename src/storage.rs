use aws_credential_types::Credentials;
use aws_sdk_s3::{
    config::{Builder, Region, SharedCredentialsProvider},
    primitives::ByteStream,
};
use serde::{Serialize, de::DeserializeOwned};

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
}
