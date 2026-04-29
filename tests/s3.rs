//! S3 integration tests against MinIO.
//!
//! Requires: `docker compose up -d`
//! Run: `cargo test --test s3 --features async`

use std::io::{self, Read};

use aqueduct::{BundleAsyncReader, BundleBuilder, Eid, MemoryRetention, S3Ops, S3Retention};
use async_trait::async_trait;
use aws_sdk_s3::primitives::ByteStream;

/// S3Ops implementation using aws-sdk-s3, backed by MinIO.
struct MinioClient {
    client: aws_sdk_s3::Client,
}

impl MinioClient {
    async fn new() -> Self {
        let creds =
            aws_sdk_s3::config::Credentials::new("minioadmin", "minioadmin", None, None, "static");
        let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .endpoint_url("http://localhost:9000")
            .region(aws_config::Region::new("us-east-1"))
            .credentials_provider(creds)
            .load()
            .await;
        let s3_config = aws_sdk_s3::config::Builder::from(&config)
            .force_path_style(true)
            .build();
        let client = aws_sdk_s3::Client::from_conf(s3_config);
        Self { client }
    }
}

#[async_trait]
impl S3Ops for MinioClient {
    async fn create_multipart_upload(&self, bucket: &str, key: &str) -> io::Result<String> {
        let resp = self
            .client
            .create_multipart_upload()
            .bucket(bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| io::Error::other(e.to_string()))?;
        resp.upload_id()
            .map(String::from)
            .ok_or_else(|| io::Error::other("no upload_id"))
    }

    async fn upload_part(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
        part_number: i32,
        data: &[u8],
    ) -> io::Result<String> {
        let resp = self
            .client
            .upload_part()
            .bucket(bucket)
            .key(key)
            .upload_id(upload_id)
            .part_number(part_number)
            .body(ByteStream::from(data.to_vec()))
            .send()
            .await
            .map_err(|e| io::Error::other(e.to_string()))?;
        resp.e_tag()
            .map(String::from)
            .ok_or_else(|| io::Error::other("no etag"))
    }

    async fn complete_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
        parts: &[(i32, String)],
    ) -> io::Result<()> {
        let completed_parts: Vec<_> = parts
            .iter()
            .map(|(num, etag)| {
                aws_sdk_s3::types::CompletedPart::builder()
                    .part_number(*num)
                    .e_tag(etag)
                    .build()
            })
            .collect();

        self.client
            .complete_multipart_upload()
            .bucket(bucket)
            .key(key)
            .upload_id(upload_id)
            .multipart_upload(
                aws_sdk_s3::types::CompletedMultipartUpload::builder()
                    .set_parts(Some(completed_parts))
                    .build(),
            )
            .send()
            .await
            .map_err(|e| io::Error::other(e.to_string()))?;
        Ok(())
    }

    async fn abort_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
    ) -> io::Result<()> {
        self.client
            .abort_multipart_upload()
            .bucket(bucket)
            .key(key)
            .upload_id(upload_id)
            .send()
            .await
            .map_err(|e| io::Error::other(e.to_string()))?;
        Ok(())
    }

    async fn get_object_range(
        &self,
        bucket: &str,
        key: &str,
        offset: u64,
        len: u64,
    ) -> io::Result<Vec<u8>> {
        let range = format!("bytes={}-{}", offset, offset + len - 1);
        let resp = self
            .client
            .get_object()
            .bucket(bucket)
            .key(key)
            .range(range)
            .send()
            .await
            .map_err(|e| io::Error::other(e.to_string()))?;
        let data = resp
            .body
            .collect()
            .await
            .map_err(|e| io::Error::other(e.to_string()))?;
        Ok(data.to_vec())
    }
}

const BUCKET: &str = "aqueduct-bundles";

#[tokio::test]
async fn s3_small_bundle() {
    let client = MinioClient::new().await;
    let payload = b"hello from s3";

    let bundle = BundleBuilder::new(
        Eid::Ipn {
            allocator_id: 0,
            node_number: 1,
            service_number: 1,
        },
        Eid::Null,
        1000,
        payload,
        MemoryRetention::new(),
    )
    .unwrap()
    .build();

    let encoded = bundle.encode().unwrap();

    let retention = S3Retention::new(client, BUCKET, "test/small")
        .await
        .unwrap();
    let decoded = BundleAsyncReader::new()
        .read_from(futures::io::Cursor::new(&encoded), retention)
        .await
        .unwrap();

    assert_eq!(decoded.primary().dest_eid, bundle.primary().dest_eid);
    assert_eq!(decoded.payload_len(), payload.len() as u64);

    let mut buf = Vec::new();
    decoded
        .async_payload_reader()
        .await
        .unwrap()
        .read_to_end(&mut buf)
        .unwrap();
    assert_eq!(buf, payload);
}

#[tokio::test]
async fn s3_large_bundle_multipart() {
    let client = MinioClient::new().await;
    // 12 MB payload — triggers at least 2 multipart parts (5 MB minimum)
    let payload = vec![0xABu8; 12 * 1024 * 1024];

    let bundle = BundleBuilder::new(
        Eid::Ipn {
            allocator_id: 0,
            node_number: 2,
            service_number: 0,
        },
        Eid::Ipn {
            allocator_id: 0,
            node_number: 1,
            service_number: 0,
        },
        3_600_000_000,
        &payload,
        MemoryRetention::new(),
    )
    .unwrap()
    .build();

    let encoded = bundle.encode().unwrap();

    let retention = S3Retention::new(client, BUCKET, "test/large-multipart")
        .await
        .unwrap();
    let decoded = BundleAsyncReader::new()
        .read_from(futures::io::Cursor::new(&encoded), retention)
        .await
        .unwrap();

    assert_eq!(decoded.payload_len(), payload.len() as u64);

    let mut buf = Vec::new();
    decoded
        .async_payload_reader()
        .await
        .unwrap()
        .read_to_end(&mut buf)
        .unwrap();
    assert_eq!(buf.len(), payload.len());
    assert!(buf.iter().all(|&b| b == 0xAB));
}

#[tokio::test]
async fn s3_discard_on_invalid_bundle() {
    let client = MinioClient::new().await;
    let garbage = &[0xDE, 0xAD, 0xBE, 0xEF];

    let retention = S3Retention::new(client, BUCKET, "test/garbage")
        .await
        .unwrap();
    let result = BundleAsyncReader::new()
        .read_from(futures::io::Cursor::new(garbage), retention)
        .await;

    // Parse fails — multipart upload should be aborted
    assert!(result.is_err());
}

#[tokio::test]
async fn s3_stream_from_http() {
    let setup_client = MinioClient::new().await;

    // Upload a 6 MB source payload to MinIO via SDK
    let source_data = vec![0x42u8; 6 * 1024 * 1024];

    // Allow anonymous GET so ureq can fetch without AWS auth
    let policy = format!(
        r#"{{"Version":"2012-10-17","Statement":[{{"Effect":"Allow","Principal":{{"AWS":["*"]}},"Action":["s3:GetObject"],"Resource":["arn:aws:s3:::{}/*"]}}]}}"#,
        BUCKET
    );
    setup_client
        .client
        .put_bucket_policy()
        .bucket(BUCKET)
        .policy(policy)
        .send()
        .await
        .unwrap();

    setup_client
        .client
        .put_object()
        .bucket(BUCKET)
        .key("source/stream-test")
        .body(ByteStream::from(source_data.clone()))
        .send()
        .await
        .unwrap();

    // Fetch via HTTP (anonymous download) — returns impl Read
    let url = "http://localhost:9000/aqueduct-bundles/source/stream-test";
    let resp = ureq::get(url).call().unwrap();
    let content_length: u64 = resp.header("content-length").unwrap().parse().unwrap();
    let body = resp.into_reader();

    // Wrap sync reader as AsyncRead, stream to S3 via from_stream
    let async_body = futures::io::AllowStdIo::new(body);
    let retention = S3Retention::new(MinioClient::new().await, BUCKET, "test/streamed")
        .await
        .unwrap();
    let bundle = BundleBuilder::from_async_stream(
        Eid::Ipn {
            allocator_id: 0,
            node_number: 10,
            service_number: 1,
        },
        Eid::Ipn {
            allocator_id: 0,
            node_number: 1,
            service_number: 0,
        },
        3600,
        content_length,
        async_body,
        retention,
    )
    .await
    .unwrap()
    .build();

    assert_eq!(bundle.payload_len(), source_data.len() as u64);

    // Read payload back from S3 and verify
    let mut buf = Vec::new();
    bundle
        .async_payload_reader()
        .await
        .unwrap()
        .read_to_end(&mut buf)
        .unwrap();
    assert_eq!(buf.len(), source_data.len());
    assert!(buf.iter().all(|&b| b == 0x42));
}
