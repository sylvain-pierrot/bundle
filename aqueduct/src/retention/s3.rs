//! S3-backed retention with multipart upload.
//!
//! Small payloads use a single `put_object`. Larger payloads
//! automatically switch to multipart upload (created lazily on
//! the first part boundary). The caller provides an [`S3Ops`]
//! implementation for their S3 client.

use std::io::Cursor;

use aqueduct_io::{Error as IoError, Read};
use async_trait::async_trait;

use super::AsyncRetention;

/// Async S3 operations trait. Implement this for your S3 client.
#[async_trait]
pub trait S3Ops: Send + Sync {
    async fn put_object(&self, bucket: &str, key: &str, data: &[u8]) -> Result<(), IoError>;

    async fn create_multipart_upload(&self, bucket: &str, key: &str) -> Result<String, IoError>;

    async fn upload_part(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
        part_number: i32,
        data: &[u8],
    ) -> Result<String, IoError>;

    async fn complete_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
        parts: &[(i32, String)],
    ) -> Result<(), IoError>;

    async fn abort_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
    ) -> Result<(), IoError>;

    async fn get_object_range(
        &self,
        bucket: &str,
        key: &str,
        offset: u64,
        len: u64,
    ) -> Result<Vec<u8>, IoError>;
}

const PART_SIZE: usize = 8 * 1024 * 1024;

/// S3-backed retention.
///
/// Uses `put_object` for payloads that fit in a single buffer,
/// multipart upload for larger payloads (created lazily).
pub struct S3Retention<C: S3Ops> {
    client: C,
    bucket: String,
    key: String,
    upload_id: Option<String>,
    buffer: Vec<u8>,
    part_number: i32,
    parts: Vec<(i32, String)>,
    completed: bool,
}

impl<C: S3Ops> S3Retention<C> {
    pub fn new(client: C, bucket: impl Into<String>, key: impl Into<String>) -> Self {
        Self {
            client,
            bucket: bucket.into(),
            key: key.into(),
            upload_id: None,
            buffer: Vec::with_capacity(PART_SIZE),
            part_number: 0,
            parts: Vec::new(),
            completed: false,
        }
    }

    async fn ensure_multipart(&mut self) -> Result<(), IoError> {
        if self.upload_id.is_none() {
            let id = self
                .client
                .create_multipart_upload(&self.bucket, &self.key)
                .await?;
            self.upload_id = Some(id);
        }
        Ok(())
    }

    async fn upload_buffer(&mut self) -> Result<(), IoError> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        self.ensure_multipart().await?;
        let upload_id = self.upload_id.as_ref().unwrap();

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
        if !self.completed && (self.upload_id.is_some() || !self.buffer.is_empty()) {
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
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        aqueduct_io::Read::read(&mut self.data, buf)
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), IoError> {
        aqueduct_io::Read::read_exact(&mut self.data, buf)
    }
}

#[async_trait]
impl<C: S3Ops> AsyncRetention for S3Retention<C> {
    type Reader<'a>
        = S3Reader
    where
        C: 'a;

    async fn write(&mut self, data: &[u8]) -> Result<usize, IoError> {
        self.buffer.extend_from_slice(data);
        if self.buffer.len() >= PART_SIZE {
            self.upload_buffer().await?;
        }
        Ok(data.len())
    }

    async fn flush(&mut self) -> Result<(), IoError> {
        if self.upload_id.is_some() {
            self.upload_buffer().await?;
            let upload_id = self.upload_id.take().unwrap();
            self.client
                .complete_multipart_upload(&self.bucket, &self.key, &upload_id, &self.parts)
                .await?;
        } else {
            self.client
                .put_object(&self.bucket, &self.key, &self.buffer)
                .await?;
            self.buffer.clear();
        }
        self.completed = true;
        Ok(())
    }

    async fn reader(&self, offset: u64, len: u64) -> Result<Self::Reader<'_>, IoError> {
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

    async fn discard(&mut self) -> Result<(), IoError> {
        if let Some(upload_id) = self.upload_id.take() {
            self.client
                .abort_multipart_upload(&self.bucket, &self.key, &upload_id)
                .await?;
        }
        self.buffer.clear();
        self.parts.clear();
        self.completed = true;
        Ok(())
    }
}
