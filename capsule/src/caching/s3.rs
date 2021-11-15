use anyhow::anyhow;
use anyhow::{Context, Result};
use async_trait::async_trait;
use hyperx::header::CacheDirective;
use rusoto_core::region::Region;
use rusoto_s3::{GetObjectOutput, GetObjectRequest, PutObjectRequest, S3Client, S3 as _};
use serde::{Serialize, Deserialize};
use serde_cbor;

use crate::caching::backend::CachingBackend;
use crate::config::Config;
use crate::iohashing::{HashBundle, OutputHashBundle};

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

#[derive(Serialize, Deserialize)]
struct InputOutputBundle {
    inputs: HashBundle,
    outputs: OutputHashBundle,
}

#[async_trait]
impl CachingBackend for S3Backend {
    fn name(&self) -> &'static str {
        "s3"
    }

    async fn write(&self, inputs: HashBundle, outputs: OutputHashBundle) -> Result<()> {
        let io_bundle = InputOutputBundle {
            inputs,
            outputs,
        };
        let key = format!("{}:{}", self.capsule_id, &io_bundle.inputs.hash);
        // Write to S3
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
        self.client
            .put_object(request)
            .await
            .context("failed to put cache entry")?;
        Ok(())
    }
}

