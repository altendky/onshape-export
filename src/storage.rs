use aws_credential_types::Credentials;
use aws_sdk_s3::config::{Builder, Region, SharedCredentialsProvider};

use crate::config::StorageConfig;

#[derive(Debug, Clone)]
pub struct StorageClient {
    client: aws_sdk_s3::Client,
    bucket: String,
    public_base_url: Option<String>,
}

impl StorageClient {
    pub async fn new(config: StorageConfig) -> anyhow::Result<Self> {
        let mut builder = Builder::new().region(Region::new(config.region.clone()));

        if let Some(endpoint_url) = &config.endpoint_url {
            builder = builder.endpoint_url(endpoint_url);
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
}
