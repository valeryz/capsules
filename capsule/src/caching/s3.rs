use anyhow::Result;
use rusoto_s3::{GetObjectOutput, GetObjectRequest, PutObjectRequest, S3Client, S3 as _};
use rusoto_core::region::Region;
use hyperx::header::CacheDirective;

use crate::config::Config;
use crate::caching::backend::CachingBackend;
use crate::iohashing::{HashBundle, OutputHashBundle, Input, Output};


pub struct S3Backend<'a> {
    /// S3 bucket
    pub bucket: String,

    /// Config instance.
    pub config: &'a Config,

    /// An S3 client from Rusoto.
    pub client: S3Client,
}

impl<'a> S3Backend<'_> {
    fn from_config(config: &'a Config) {
        Self {
            bucket: config
                .s3_bucket
                .clone()
                .ok_or_else(|| anyhow!("S3 bucket not specified"))?,
            config: config,
            client: S3Client::new(
                Region::Custom {
                    name: config.s3_region.ok_or_else(|| anyhow!("S3 region not specified"))?,
                    endpoint: config.s3_endpoint.ok_or_else(|| anyhow!("S3 endpoint not specified"))?,
                })
        }
    }
}

impl<'a> CachingBackend for S3Backend<'_> {
    
    fn name(&self) -> &'static str {
        "s3"
    }

    fn write(&self, inputs_bundle: &HashBundle, output_bundle: &OutputHashBundle) -> Result<()> {
        let key = format!("{}:{}", self.config.capsule_id.as_ref().unwrap(), inputs_bundle.hash);
        // Write to S3
        let request = PutObjectRequest {
            bucket: self.bucket.clone(),
            body: Some(data.into()),
            // Two weeks
            cache_control: Some(CacheDirective::MaxAge(1_296_000).to_string()),
            content_length: Some(data_length as i64),
            content_type: Some("application/octet-stream".to_owned()),
            key,
            ..Default::default()
        };
        self.client.as_ref().map(|client| client.pub_object(request).context("failed to put cache entry"))
    }
}
