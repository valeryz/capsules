use anyhow::anyhow;
use anyhow::{Context, Result};
use async_trait::async_trait;
use hyperx::header::CacheDirective;
use rusoto_core::region::Region;
use rusoto_s3::{GetObjectRequest, PutObjectRequest, S3Client, S3 as _};
use serde_cbor;
use tokio::io::AsyncReadExt as _;

use crate::caching::backend::CachingBackend;
use crate::config::Config;
use crate::iohashing::{HashBundle, OutputHashBundle, InputOutputBundle};

pub struct S3Backend {
    /// S3 bucket
    pub bucket: String,

    /// An S3 client from Rusoto.
    pub client: S3Client,

    /// Capsule ID
    pub capsule_id: String,
}

impl S3Backend {
    pub fn from_config(config: &Config) -> Result<Self> {
        Ok(Self {
            bucket: config
                .s3_bucket
                .clone()
                .ok_or_else(|| anyhow!("S3 bucket not specified"))?,
            client: S3Client::new(Region::Custom {
                name: config
                    .s3_region
                    .as_ref()
                    .cloned()
                    .ok_or_else(|| anyhow!("S3 region not specified"))?,
                endpoint: config
                    .s3_endpoint
                    .as_ref()
                    .cloned()
                    .ok_or_else(|| anyhow!("S3 endpoint not specified"))?,
            }),
            capsule_id: config.capsule_id.as_deref().unwrap().to_string(),
        })
    }
}

#[async_trait]
impl CachingBackend for S3Backend {
    fn name(&self) -> &'static str {
        "s3"
    }

    async fn lookup(&self, inputs: &HashBundle) -> Result<Option<InputOutputBundle>> {
        let key = format!("{}:{}", self.capsule_id, inputs.hash);
        let request = GetObjectRequest {
            bucket: self.bucket.clone(),
            key,
            ..Default::default()
        };
        let response = self.client
            .get_object(request)
            .await;
        match response {
            Err(rusoto_core::RusotoError::Service(rusoto_s3::GetObjectError::NoSuchKey(_))) => {
                Ok(None)  // Cache miss
            },
            Err(e) => Err(e.into()),
            Ok(response) => {
                let body = response.body.context("No reponse body")?;
                let mut body_reader = body.into_async_read();
                let mut body = Vec::new();
                body_reader
                    .read_to_end(&mut body)
                    .await
                    .context("failed to read HTTP body")?;
                let bundle = serde_cbor::from_slice(&body).context("Cannot deserialize output")?;
                Ok(Some(bundle))
            }
        }
    }

    async fn write(&self, inputs: HashBundle, outputs: OutputHashBundle) -> Result<()> {
        let io_bundle = InputOutputBundle {
            inputs,
            outputs,
        };
        let key = format!("{}:{}", self.capsule_id, &io_bundle.inputs.hash);
        // Prepare data for S3 writing.
        let data = serde_cbor::to_vec(&io_bundle)?;
        let data_len = data.len();
        let request = PutObjectRequest {
            bucket: self.bucket.clone(),
            body: Some(data.into()),
            // Two weeks
            cache_control: Some(CacheDirective::MaxAge(1_296_000).to_string()),
            content_length: Some(data_len as i64),
            content_type: Some("application/octet-stream".to_owned()),
            key,
            ..Default::default()
        };

        // Write data to S3 (asynchronously).
        self.client
            .put_object(request)
            .await
            .context("failed to put cache entry")?;
        Ok(())
    }
}

