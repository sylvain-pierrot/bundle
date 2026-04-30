//! S3 integration tests against MinIO.
//!
//! Requires: `docker compose up -d`
//! Run: `cargo test --test s3 --features async`

use std::time::Instant;

use aqueduct::{BundleAsyncReader, BundleBuilder, MemoryRetention, S3Ops, S3Retention};
use aqueduct_bpv7::Eid;
use aqueduct_io::{Error as IoError, Read};
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
    async fn put_object(&self, bucket: &str, key: &str, data: &[u8]) -> Result<(), IoError> {
        self.client
            .put_object()
            .bucket(bucket)
            .key(key)
            .body(ByteStream::from(data.to_vec()))
            .send()
            .await
            .map_err(|e| IoError::Io(std::io::Error::other(e.to_string())))?;
        Ok(())
    }

    async fn create_multipart_upload(&self, bucket: &str, key: &str) -> Result<String, IoError> {
        let resp = self
            .client
            .create_multipart_upload()
            .bucket(bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| IoError::Io(std::io::Error::other(e.to_string())))?;
        resp.upload_id()
            .map(String::from)
            .ok_or_else(|| IoError::Io(std::io::Error::other("no upload_id")))
    }

    async fn upload_part(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
        part_number: i32,
        data: &[u8],
    ) -> Result<String, IoError> {
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
            .map_err(|e| IoError::Io(std::io::Error::other(e.to_string())))?;
        resp.e_tag()
            .map(String::from)
            .ok_or_else(|| IoError::Io(std::io::Error::other("no etag")))
    }

    async fn complete_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
        parts: &[(i32, String)],
    ) -> Result<(), IoError> {
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
            .map_err(|e| IoError::Io(std::io::Error::other(e.to_string())))?;
        Ok(())
    }

    async fn abort_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
    ) -> Result<(), IoError> {
        self.client
            .abort_multipart_upload()
            .bucket(bucket)
            .key(key)
            .upload_id(upload_id)
            .send()
            .await
            .map_err(|e| IoError::Io(std::io::Error::other(e.to_string())))?;
        Ok(())
    }

    async fn get_object_range(
        &self,
        bucket: &str,
        key: &str,
        offset: u64,
        len: u64,
    ) -> Result<Vec<u8>, IoError> {
        let range = format!("bytes={}-{}", offset, offset + len - 1);
        let resp = self
            .client
            .get_object()
            .bucket(bucket)
            .key(key)
            .range(range)
            .send()
            .await
            .map_err(|e| IoError::Io(std::io::Error::other(e.to_string())))?;
        let data = resp
            .body
            .collect()
            .await
            .map_err(|e| IoError::Io(std::io::Error::other(e.to_string())))?;
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
    .build()
    .unwrap();

    let encoded = bundle.encode().unwrap();

    let retention = S3Retention::new(client, BUCKET, "test/small");
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
    .build()
    .unwrap();

    let encoded = bundle.encode().unwrap();

    let retention = S3Retention::new(client, BUCKET, "test/large-multipart");
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

    let retention = S3Retention::new(client, BUCKET, "test/garbage");
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
    let retention = S3Retention::new(MinioClient::new().await, BUCKET, "test/streamed");
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
    .build()
    .unwrap();

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

// -- Throughput benchmarks ---------------------------------------------------

fn mb(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

fn throughput(label: &str, bytes: u64, elapsed: std::time::Duration) {
    let secs = elapsed.as_secs_f64();
    eprintln!(
        "  {label}: {:.1} MB in {:.2}s = {:.1} MB/s",
        mb(bytes),
        secs,
        mb(bytes) / secs,
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_throughput_100mb() {
    s3_throughput_test(100).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_throughput_500mb() {
    s3_throughput_test(500).await;
}

async fn s3_throughput_test(size_mb: usize) {
    let payload_size = size_mb * 1024 * 1024;
    eprintln!("\n--- S3 throughput: {size_mb} MB payload ---");

    // Phase 1: build bundle in memory + encode
    let t = Instant::now();
    let payload = vec![0xBBu8; payload_size];
    let bundle = BundleBuilder::new(
        Eid::Ipn {
            allocator_id: 0,
            node_number: 99,
            service_number: 1,
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
    .build()
    .unwrap();
    let mut encoded = Vec::new();
    bundle.encode_to(&mut encoded).unwrap();
    throughput("encode (memory)", encoded.len() as u64, t.elapsed());

    // Phase 2: async receive into S3
    let key = format!("bench/{size_mb}mb");
    let t = Instant::now();
    let retention = S3Retention::new(MinioClient::new().await, BUCKET, &key);
    let decoded = BundleAsyncReader::new()
        .read_from(futures::io::Cursor::new(&encoded), retention)
        .await
        .unwrap();
    throughput("receive → S3", encoded.len() as u64, t.elapsed());

    // Phase 3: read payload back from S3
    let t = Instant::now();
    let mut buf = Vec::new();
    decoded
        .async_payload_reader()
        .await
        .unwrap()
        .read_to_end(&mut buf)
        .unwrap();
    throughput("read payload ← S3", payload_size as u64, t.elapsed());

    assert_eq!(buf.len(), payload_size);
    assert!(buf.iter().all(|&b| b == 0xBB));

    // Phase 4: async encode back out (S3 → wire)
    let t = Instant::now();
    let mut out = Vec::new();
    decoded
        .async_encode_to(futures::io::Cursor::new(&mut out))
        .await
        .unwrap();
    throughput("encode S3 → wire", out.len() as u64, t.elapsed());

    assert_eq!(out.len(), encoded.len());
    eprintln!();
}

#[tokio::test(flavor = "multi_thread")]
async fn s3_throughput_many_small() {
    let count: u64 = 1000;
    let concurrency = 50;
    eprintln!("\n--- S3 throughput: {count} small bundles ({concurrency} concurrent) ---");

    let payload = b"small bundle payload for throughput test";
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
    .build()
    .unwrap();
    let mut encoded = Vec::new();
    bundle.encode_to(&mut encoded).unwrap();
    let encoded = std::sync::Arc::new(encoded);

    let t = Instant::now();
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(concurrency));
    let mut handles = Vec::new();

    for i in 0..count {
        let permit = sem.clone().acquire_owned().await.unwrap();
        let encoded = encoded.clone();
        let payload_len = payload.len() as u64;
        handles.push(tokio::spawn(async move {
            let key = format!("bench/small/{i}");
            let retention = S3Retention::new(MinioClient::new().await, BUCKET, &key);
            let decoded = BundleAsyncReader::new()
                .read_from(futures::io::Cursor::new(encoded.as_slice()), retention)
                .await
                .unwrap();
            assert_eq!(decoded.payload_len(), payload_len);
            drop(permit);
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    let elapsed = t.elapsed();
    let total_bytes = (encoded.len() as u64) * count;
    eprintln!(
        "  {count} bundles in {:.2}s = {:.0} bundles/s ({:.1} MB/s)",
        elapsed.as_secs_f64(),
        count as f64 / elapsed.as_secs_f64(),
        mb(total_bytes) / elapsed.as_secs_f64(),
    );
    eprintln!();
}
