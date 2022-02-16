use anyhow::anyhow;
use anyhow::{Context, Result};
use async_compression::tokio::bufread::{GzipDecoder, GzipEncoder};
use async_trait::async_trait;
use futures::TryStreamExt;
use hyperx::header::CacheDirective;
use rusoto_core::region::Region;
use rusoto_s3::{GetObjectRequest, HeadObjectRequest, PutObjectRequest, S3Client, S3 as _};
use serde_json;
use std::pin::Pin;
use tempfile::tempfile;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeekExt, BufReader};
use tokio_util::codec;

use crate::caching::backend::CachingBackend;
use crate::config::Config;
use crate::iohashing::{InputHashBundle, InputOutputBundle, OutputHashBundle};

pub struct S3Backend {
    /// S3 bucket for keys
    pub bucket: String,

    /// S3 bucket for objects,
    pub bucket_objects: String,

    /// An Rusoto S3 client
    pub client: S3Client,

    /// An S3 client for uploads
    pub client_uploads: S3Client,

    /// An S3 client for downloads
    pub client_downloads: S3Client,

    /// Capsule ID
    pub capsule_id: String,
}

impl S3Backend {
    pub fn from_config(config: &Config) -> Result<Self> {
        let client = S3Client::new(Region::Custom {
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
        });
        let client_uploads = if config.s3_uploads_endpoint.is_some() || config.s3_uploads_region.is_some() {
            S3Client::new(Region::Custom {
                name: config
                    .s3_uploads_region
                    .as_ref()
                    .cloned()
                    .ok_or_else(|| anyhow!("S3 uploads region not specified"))?,
                endpoint: config
                    .s3_uploads_endpoint
                    .as_ref()
                    .cloned()
                    .ok_or_else(|| anyhow!("S3 uploads endpoint not specified"))?,
            })
        } else {
            client.clone()
        };
        let client_downloads = if config.s3_downloads_endpoint.is_some() || config.s3_downloads_region.is_some() {
            S3Client::new(Region::Custom {
                name: config
                    .s3_downloads_region
                    .as_ref()
                    .cloned()
                    .ok_or_else(|| anyhow!("S3 downloads region not specified"))?,
                endpoint: config
                    .s3_downloads_endpoint
                    .as_ref()
                    .cloned()
                    .ok_or_else(|| anyhow!("S3 downloads endpoint not specified"))?,
            })
        } else {
            client.clone()
        };
        Ok(Self {
            bucket: config
                .s3_bucket
                .clone()
                .ok_or_else(|| anyhow!("S3 bucket not specified"))?,
            bucket_objects: config
                .s3_bucket_objects
                .clone()
                .ok_or_else(|| anyhow!("S3 bucket for objects not specified"))?,
            client,
            client_uploads,
            client_downloads,
            capsule_id: config.capsule_id.as_deref().unwrap().to_string(),
        })
    }

    fn normalize_key(&self, key: &str) -> String {
        format!("{}/{}/{}", &self.capsule_id, &key[0..2], key)
    }

    fn normalize_object_key(&self, key: &str) -> String {
        format!("{}/{}", &key[0..2], key)
    }

    async fn object_exists(&self, request: HeadObjectRequest) -> Result<bool> {
        // We use the uploads client, since we have to check object existence before the upload.
        let result = self.client_uploads.head_object(request).await;
        match result {
            Ok(_) => Ok(true),
            Err(rusoto_core::RusotoError::Service(rusoto_s3::HeadObjectError::NoSuchKey(_))) => Ok(false),
            Err(rusoto_core::RusotoError::Unknown(resp)) if resp.status == 404 => {
                // No such bucket
                Ok(false)
            }
            Err(e) => {
                eprintln!("object_exists error: {}", e);
                Err(e.into())
            }
        }
    }
}

#[async_trait]
impl CachingBackend for S3Backend {
    fn name(&self) -> &'static str {
        "s3"
    }

    /// Lookup inputs in S3.
    async fn lookup(&self, inputs: &InputHashBundle) -> Result<Option<InputOutputBundle>> {
        let key = self.normalize_key(&inputs.hash);
        let request = GetObjectRequest {
            bucket: self.bucket.clone(),
            key,
            ..Default::default()
        };
        let response = self.client.get_object(request).await;
        match response {
            Err(rusoto_core::RusotoError::Service(rusoto_s3::GetObjectError::NoSuchKey(_))) => {
                Ok(None) // Cache miss
            }
            Err(rusoto_core::RusotoError::Unknown(resp)) if resp.status == 404 => {
                // No such bucket
                Ok(None) // Cache miss
            }
            Err(e) => Err(e.into()),
            Ok(response) => {
                let body = response.body.context("No reponse body")?;
                let mut body_reader = body.into_async_read();
                let mut body = Vec::new();
                body_reader
                    .read_to_end(&mut body)
                    .await
                    .context("failed to read HTTP body")?;
                let bundle = serde_json::from_slice(&body).context("Cannot deserialize output")?;
                Ok(Some(bundle))
            }
        }
    }

    /// Read a file object from the storage, and return AsyncRead object for consuming by capsule.
    async fn download_object_file(&self, item_hash: &str) -> Result<Pin<Box<dyn AsyncRead>>> {
        let key = self.normalize_object_key(item_hash);
        let request = GetObjectRequest {
            bucket: self.bucket_objects.clone(),
            key,
            ..Default::default()
        };
        let response = self.client_downloads.get_object(request).await?;
        let body = response.body.context("No reponse body")?;
        if response.content_encoding.unwrap_or_default() == "gzip"
            || response.content_type.unwrap_or_default() == "application/gzip"
        {
            Ok(Box::pin(GzipDecoder::new(BufReader::new(body.into_async_read()))))
        } else {
            Ok(Box::pin(body.into_async_read()))
        }
    }

    async fn upload_object_file(
        &self,
        item_hash: &str,
        file: Pin<Box<dyn AsyncRead + Send>>,
        _content_length: u64,
    ) -> Result<()> {
        // Find the key under which we'll store the object in the bucket.
        let key = self.normalize_object_key(item_hash);

        let request = HeadObjectRequest {
            bucket: self.bucket_objects.clone(),
            key: key.clone(),
            ..Default::default()
        };

        // Objects in the content addresable storage are "immutable", so duplicate uploads can be skipped.
        if self.object_exists(request).await? {
            eprintln!("Skipping upload for existing object '{}'", item_hash);
            return Ok(());
        } else {
            eprintln!("Uploading the object to '{}'", item_hash);
        }

        // We cannot compress the file on the fly due to the need for specify Content-length.
        // So we'll create a temporary file with gzip'ed contents and upload it.
        let mut file = GzipEncoder::new(BufReader::new(file));
        let gzout = tempfile()?;
        let mut gzout = tokio::fs::File::from_std(gzout);
        tokio::io::copy(&mut file, &mut gzout).await?;
        let content_length = gzout.metadata().await?.len();
        gzout.seek(std::io::SeekFrom::Start(0)).await?;

        // Temporary file is ready, time to upload it.
        let byte_stream = codec::FramedRead::new(gzout, codec::BytesCodec::new()).map_ok(|r| r.freeze());
        let request = PutObjectRequest {
            bucket: self.bucket_objects.clone(),
            key,
            body: Some(rusoto_core::ByteStream::new(byte_stream)),
            content_length: Some(content_length as i64),
            // Two weeks - content addresable storage doesn't change, so CDNs can cache for long.
            cache_control: Some(CacheDirective::MaxAge(2_592_000).to_string()),
            content_type: Some("application/gzip".to_owned()),
            ..Default::default()
        };
        self.client_uploads.put_object(request).await?;
        Ok(())
    }

    /// Write hashes of inputs and outputs into S3, keyed by hashes of inputs.
    async fn write(&self, inputs: &InputHashBundle, outputs: &OutputHashBundle, source: String) -> Result<()> {
        let io_bundle = InputOutputBundle {
            inputs: inputs.clone(),
            outputs: outputs.clone(),
            source,
        };
        let key = self.normalize_key(&io_bundle.inputs.hash);
        // Prepare data for S3 writing.
        let data = serde_json::to_vec(&io_bundle)?;
        let data_len = data.len();
        let request = PutObjectRequest {
            bucket: self.bucket.clone(),
            body: Some(data.into()),
            cache_control: Some(CacheDirective::NoCache.to_string()),
            content_length: Some(data_len as i64),
            content_type: Some("application/json".to_owned()),
            key,
            ..Default::default()
        };

        // Write data to S3 (asynchronously).
        self.client.put_object(request).await?;
        Ok(())
    }
}
