use anyhow::anyhow;
use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::{future::try_join_all, TryStreamExt};
use hyperx::header::CacheDirective;
use rusoto_core::region::Region;
use rusoto_s3::{GetObjectRequest, PutObjectRequest, S3Client, S3 as _};
use serde_cbor;
use tempfile::NamedTempFile;
use std::fs as std_fs;
use tokio::fs as tokio_fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::codec;

use crate::caching::backend::CachingBackend;
use crate::config::Config;
use crate::iohashing::{FileOutput, HashBundle, InputOutputBundle, Output, OutputHashBundle};

pub struct S3Backend {
    /// S3 bucket for keys
    pub bucket: String,

    /// S3 bucket for objects,
    pub bucket_objects: String,

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
            bucket_objects: config
                .s3_bucket_objects
                .clone()
                .ok_or_else(|| anyhow!("S3 bucket for objects not specified"))?,
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

    fn normalize_key(&self, key: &str) -> String {
        format!(
            "{}/{}/{}/{}/{}",
            &self.capsule_id,
            &key[0..1],
            &key[1..2],
            &key[2..3],
            &key
        )
    }

    fn normalize_object_key(&self, key: &str) -> String {
        format!("{}/{}", &key[0..3], &key)
    }

    // Read a file object from S3, and place it at its output path by first reading asynchronously
    // from the S3 stream into a temp file, and then persisting (moving) into the destination name.
    async fn read_move_object_file(&self, fileoutput: &FileOutput, item_hash: &str) -> Result<()> {
        let key = self.normalize_object_key(item_hash);
        let request = GetObjectRequest {
            bucket: self.bucket_objects.clone(),
            key,
            ..Default::default()
        };
        let response = self.client.get_object(request).await?;
        let body = response.body.context("No reponse body")?;
        let dir = fileoutput.filename.parent().context("No parent directory")?;
        let file = NamedTempFile::new_in(dir)?;
        let (file, path) = file.into_parts();
        let mut file_stream = tokio::fs::File::from_std(file);
        let mut body_reader = body.into_async_read();
        tokio::io::copy(&mut body_reader, &mut file_stream).await?;
        file_stream.flush().await?;
        path.persist(&fileoutput.filename)?;
        Ok(())
    }
}

#[async_trait]
impl CachingBackend for S3Backend {
    fn name(&self) -> &'static str {
        "s3"
    }

    /// Lookup inputs in S3.
    async fn lookup(&self, inputs: &HashBundle) -> Result<Option<InputOutputBundle>> {
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
                let bundle = serde_cbor::from_slice(&body).context("Cannot deserialize output")?;
                Ok(Some(bundle))
            }
        }
    }

    /// Read all output files from S3, and place them into destination paths.
    async fn read_files(&self, outputs: &OutputHashBundle) -> Result<()> {
        let mut all_files_futures = Vec::new();
        for (item, item_hash) in &outputs.hash_details {
            if let Output::File(ref fileoutput) = item {
                if fileoutput.present {
                    let download_file_fut = self.read_move_object_file(fileoutput, item_hash);
                    all_files_futures.push(download_file_fut);
                } else {
                    std_fs::remove_file(&fileoutput.filename)
                        .unwrap_or_else(|err| eprintln!("Failed to remove file {}", err));
                }
            }
        }
        try_join_all(all_files_futures).await?;
        Ok(())
    }

    /// Write hashes of inputs and outputs into S3, keyed by hashes of inputs.
    async fn write(&self, inputs: &HashBundle, outputs: &OutputHashBundle) -> Result<()> {
        let io_bundle = InputOutputBundle {
            inputs: inputs.clone(),
            outputs: outputs.clone(),
        };
        let key = self.normalize_key(&io_bundle.inputs.hash);
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
        self.client.put_object(request).await?;
        Ok(())
    }

    /// Write output files into S3, keyed by their hash (content addressed).
    async fn write_files(&self, outputs: &OutputHashBundle) -> Result<()> {
        let mut all_files_futures = Vec::new();
        for (item, item_hash) in &outputs.hash_details {
            if let Output::File(ref fileoutput) = item {
                if fileoutput.present {
                    let tokio_file = tokio_fs::File::open(&fileoutput.filename).await?;
                    let byte_stream =
                        codec::FramedRead::new(tokio_file, codec::BytesCodec::new()).map_ok(|r| r.freeze());

                    let request = PutObjectRequest {
                        bucket: self.bucket_objects.clone(),
                        key: self.normalize_object_key(item_hash),
                        body: Some(rusoto_core::ByteStream::new(byte_stream)),
                        // Two weeks
                        cache_control: Some(CacheDirective::MaxAge(2_592_000).to_string()),
                        content_type: Some("application/octet-stream".to_owned()),
                        ..Default::default()
                    };
                    let put_future = self.client.put_object(request);
                    all_files_futures.push(put_future);
                }
            }
        }
        try_join_all(all_files_futures).await?;
        Ok(())
    }
}
