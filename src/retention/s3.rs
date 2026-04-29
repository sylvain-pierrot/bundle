//! S3-backed retention with multipart upload.
//!
//! Streams bytes to S3 via multipart upload during reception. Reads
//! download byte ranges from S3. The caller provides an [`S3Ops`]
//! implementation for their S3 client (aws-sdk-s3, rusoto, minio, etc.).

use std::io::{self, Cursor, Read};

use async_trait::async_trait;

use super::AsyncRetention;

/// Async S3 operations trait. Implement this for your S3 client.
#[async_trait]
pub trait S3Ops: Send + Sync {
    async fn create_multipart_upload(&self, bucket: &str, key: &str) -> io::Result<String>;

    async fn upload_part(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
        part_number: i32,
        data: &[u8],
    ) -> io::Result<String>;

    async fn complete_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
        parts: &[(i32, String)],
    ) -> io::Result<()>;

    async fn abort_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
    ) -> io::Result<()>;

    async fn get_object_range(
        &self,
        bucket: &str,
        key: &str,
        offset: u64,
        len: u64,
    ) -> io::Result<Vec<u8>>;
}

const MIN_PART_SIZE: usize = 5 * 1024 * 1024; // S3 minimum

/// S3-backed retention using multipart upload.
pub struct S3Retention<C: S3Ops> {
    client: C,
    bucket: String,
    key: String,
    upload_id: Option<String>,
    buffer: Vec<u8>,
    part_number: i32,
    parts: Vec<(i32, String)>,
}

impl<C: S3Ops> S3Retention<C> {
    pub async fn new(
        client: C,
        bucket: impl Into<String>,
        key: impl Into<String>,
    ) -> io::Result<Self> {
        let bucket = bucket.into();
        let key = key.into();
        let upload_id = client.create_multipart_upload(&bucket, &key).await?;

        Ok(Self {
            client,
            bucket,
            key,
            upload_id: Some(upload_id),
            buffer: Vec::with_capacity(MIN_PART_SIZE),
            part_number: 0,
            parts: Vec::new(),
        })
    }

    async fn upload_buffer(&mut self) -> io::Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        let upload_id = self
            .upload_id
            .as_ref()
            .ok_or_else(|| io::Error::other("upload already completed"))?;

        self.part_number += 1;
        let etag = self
            .client
            .upload_part(
                &self.bucket,
                &self.key,
                upload_id,
                self.part_number,
                &self.buffer,
            )
            .await?;
        self.parts.push((self.part_number, etag));
        self.buffer.clear();
        Ok(())
    }
}

impl<C: S3Ops> Drop for S3Retention<C> {
    fn drop(&mut self) {
        if self.upload_id.is_some() {
            eprintln!(
                "S3Retention dropped without flush() or discard(): bucket={}, key={}",
                self.bucket, self.key
            );
        }
    }
}

pub struct S3Reader {
    data: Cursor<Vec<u8>>,
}

impl Read for S3Reader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.data.read(buf)
    }
}

#[async_trait]
impl<C: S3Ops> AsyncRetention for S3Retention<C> {
    type Reader<'a>
        = S3Reader
    where
        C: 'a;

    async fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(data);
        if self.buffer.len() >= MIN_PART_SIZE {
            self.upload_buffer().await?;
        }
        Ok(data.len())
    }

    async fn flush(&mut self) -> io::Result<()> {
        self.upload_buffer().await?;
        if let Some(upload_id) = self.upload_id.take() {
            if self.parts.is_empty() {
                self.part_number += 1;
                let etag = self
                    .client
                    .upload_part(&self.bucket, &self.key, &upload_id, self.part_number, &[])
                    .await?;
                self.parts.push((self.part_number, etag));
            }
            self.client
                .complete_multipart_upload(&self.bucket, &self.key, &upload_id, &self.parts)
                .await?;
        }
        Ok(())
    }

    async fn reader(&self, offset: u64, len: u64) -> io::Result<Self::Reader<'_>> {
        if len == 0 {
            return Ok(S3Reader {
                data: Cursor::new(Vec::new()),
            });
        }
        let data = self
            .client
            .get_object_range(&self.bucket, &self.key, offset, len)
            .await?;
        Ok(S3Reader {
            data: Cursor::new(data),
        })
    }

    async fn discard(&mut self) -> io::Result<()> {
        if let Some(upload_id) = self.upload_id.take() {
            self.client
                .abort_multipart_upload(&self.bucket, &self.key, &upload_id)
                .await?;
        }
        self.buffer.clear();
        self.parts.clear();
        Ok(())
    }
}
