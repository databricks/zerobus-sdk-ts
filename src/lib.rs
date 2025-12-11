// Zerobus TypeScript SDK - NAPI-RS Bindings
//
// This file provides the Node.js/TypeScript bindings for the Rust Zerobus SDK
// using NAPI-RS. It exposes a TypeScript-friendly API while leveraging the
// high-performance Rust implementation underneath.
//
// The binding layer handles:
// - Type conversions between JavaScript and Rust types
// - Async/await bridging (Rust futures → JavaScript Promises)
// - Memory management and thread safety
// - Error propagation

#![deny(clippy::all)]

use napi::bindgen_prelude::*;
use napi_derive::napi;

use databricks_zerobus_ingest_sdk::{
    RecordPayload as RustRecordPayload,
    StreamConfigurationOptions as RustStreamOptions,
    TableProperties as RustTableProperties, ZerobusSdk as RustZerobusSdk,
    ZerobusStream as RustZerobusStream,
};
use databricks_zerobus_ingest_sdk::databricks::zerobus::RecordType;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Configuration options for the Zerobus stream.
///
/// These options control stream behavior including recovery, timeouts, and inflight limits.
#[napi(object)]
#[derive(Debug, Clone)]
pub struct StreamConfigurationOptions {
    /// Maximum number of unacknowledged records that can be in flight.
    /// Default: 1,000,000
    pub max_inflight_records: Option<u32>,

    /// Enable automatic stream recovery on transient failures.
    /// Default: true
    pub recovery: Option<bool>,

    /// Timeout for recovery operations in milliseconds.
    /// Default: 15,000 (15 seconds)
    pub recovery_timeout_ms: Option<u32>,

    /// Delay between recovery retry attempts in milliseconds.
    /// Default: 2,000 (2 seconds)
    pub recovery_backoff_ms: Option<u32>,

    /// Maximum number of recovery attempts before giving up.
    /// Default: 4
    pub recovery_retries: Option<u32>,

    /// Timeout for flush operations in milliseconds.
    /// Default: 300,000 (5 minutes)
    pub flush_timeout_ms: Option<u32>,

    /// Timeout waiting for server acknowledgments in milliseconds.
    /// Default: 60,000 (1 minute)
    pub server_lack_of_ack_timeout_ms: Option<u32>,

    /// Record type: 0 = JSON, 1 = Proto
    /// Default: 0 (JSON)
    pub record_type: Option<i32>,
}

impl From<StreamConfigurationOptions> for RustStreamOptions {
    fn from(opts: StreamConfigurationOptions) -> Self {
        let default = RustStreamOptions::default();

        // Convert record_type: 0 = JSON, 1 = Proto, default to JSON
        let record_type = match opts.record_type {
            Some(0) => RecordType::Json,
            Some(1) => RecordType::Proto,
            _ => RecordType::Json, // Default to JSON if not specified
        };

        RustStreamOptions {
            max_inflight_records: opts.max_inflight_records.unwrap_or(default.max_inflight_records as u32) as usize,
            recovery: opts.recovery.unwrap_or(default.recovery),
            recovery_timeout_ms: opts.recovery_timeout_ms.map(|v| v as u64).unwrap_or(default.recovery_timeout_ms),
            recovery_backoff_ms: opts.recovery_backoff_ms.map(|v| v as u64).unwrap_or(default.recovery_backoff_ms),
            recovery_retries: opts.recovery_retries.unwrap_or(default.recovery_retries),
            flush_timeout_ms: opts.flush_timeout_ms.map(|v| v as u64).unwrap_or(default.flush_timeout_ms),
            server_lack_of_ack_timeout_ms: opts.server_lack_of_ack_timeout_ms.map(|v| v as u64).unwrap_or(default.server_lack_of_ack_timeout_ms),
            record_type,
        }
    }
}

/// Properties of the target Delta table for ingestion.
///
/// Specifies which Unity Catalog table to write to and optionally the schema descriptor
/// for Protocol Buffers encoding.
#[napi(object)]
#[derive(Debug, Clone)]
pub struct TableProperties {
    /// Full table name in Unity Catalog (e.g., "catalog.schema.table")
    pub table_name: String,

    /// Optional Protocol Buffer descriptor as a base64-encoded string.
    /// If not provided, JSON encoding will be used.
    pub descriptor_proto: Option<String>,
}

impl TableProperties {
    fn to_rust(&self) -> Result<RustTableProperties> {
        let descriptor: Option<prost_types::DescriptorProto> = if let Some(ref desc_str) = self.descriptor_proto {
            // Decode base64 descriptor
            let bytes = base64_decode(desc_str)
                .map_err(|e| Error::from_reason(format!("Failed to decode descriptor: {}", e)))?;
            let descriptor_proto: prost_types::DescriptorProto = prost::Message::decode(&bytes[..])
                .map_err(|e| Error::from_reason(format!("Failed to parse descriptor proto: {}", e)))?;
            Some(descriptor_proto)
        } else {
            None
        };

        Ok(RustTableProperties {
            table_name: self.table_name.clone(),
            descriptor_proto: descriptor,
        })
    }
}

/// Custom error type for Zerobus operations.
///
/// This error type includes information about whether the error is retryable,
/// which helps determine if automatic recovery can resolve the issue.
#[napi]
pub struct ZerobusError {
    message: String,
    is_retryable: bool,
}

#[napi]
impl ZerobusError {
    /// Returns true if this error can be automatically retried by the SDK.
    #[napi(getter)]
    pub fn is_retryable(&self) -> bool {
        self.is_retryable
    }

    /// Get the error message.
    #[napi(getter)]
    pub fn message(&self) -> String {
        self.message.clone()
    }
}


/// A stream for ingesting data into a Databricks Delta table.
///
/// The stream manages a bidirectional gRPC connection, handles acknowledgments,
/// and provides automatic recovery on transient failures.
///
/// # Example
///
/// ```typescript
/// const stream = await sdk.createStream(tableProps, clientId, clientSecret, options);
/// const ackPromise = await stream.ingestRecord(Buffer.from([1, 2, 3]));
/// const offset = await ackPromise;
/// await stream.close();
/// ```
#[napi]
pub struct ZerobusStream {
    inner: Arc<Mutex<Option<RustZerobusStream>>>,
}

#[napi]
impl ZerobusStream {
    /// Ingests a single record into the stream and waits for acknowledgment.
    ///
    /// This method accepts either:
    /// - A Protocol Buffer encoded record as a Buffer (Vec<u8>)
    /// - A JSON string
    ///
    /// Returns a Promise that resolves to the offset ID when the server
    /// acknowledges the record.
    ///
    /// # Arguments
    ///
    /// * `payload` - The record data as a Buffer (protobuf) or string (JSON)
    ///
    /// # Returns
    ///
    /// A Promise that resolves to the offset ID (number).
    ///
    /// # Errors
    ///
    /// - Stream closed error if the stream has been closed
    /// - Network errors if the connection fails
    /// - Validation errors if the payload doesn't match the table schema
    ///
    /// # Example
    ///
    /// ```typescript
    /// // With JSON
    /// const offset = await stream.ingestRecord('{"id": 1, "name": "test"}');
    ///
    /// // With Protocol Buffer
    /// const offset = await stream.ingestRecord(Buffer.from([...]));
    /// ```
    #[napi]
    pub async fn ingest_record(&self, payload: Either<Buffer, String>) -> Result<i64> {
        let mut guard = self.inner.lock().await;
        let stream = guard
            .as_mut()
            .ok_or_else(|| Error::from_reason("Stream has been closed"))?;

        // Convert payload to RecordPayload
        let record_payload: RustRecordPayload = match payload {
            Either::A(buffer) => RustRecordPayload::Proto(buffer.to_vec()),
            Either::B(json_string) => RustRecordPayload::Json(json_string),
        };

        // Call ingest_record and get the acknowledgment future
        let ack_future = stream
            .ingest_record(record_payload)
            .await
            .map_err(|e| Error::from_reason(format!("Failed to ingest record: {}", e)))?;

        // Await the acknowledgment
        let offset = ack_future
            .await
            .map_err(|e| Error::from_reason(format!("Acknowledgment failed: {}", e)))?;

        Ok(offset)
    }

    /// Flushes all pending records and waits for acknowledgments.
    ///
    /// This method ensures all previously ingested records have been sent to the server
    /// and acknowledged. It's useful for checkpointing or ensuring data durability.
    ///
    /// # Errors
    ///
    /// - Timeout errors if flush takes longer than configured timeout
    /// - Network errors if the connection fails during flush
    #[napi]
    pub async fn flush(&self) -> Result<()> {
        let guard = self.inner.lock().await;
        let stream = guard
            .as_ref()
            .ok_or_else(|| Error::from_reason("Stream has been closed"))?;

        stream
            .flush()
            .await
            .map_err(|e| Error::from_reason(format!("Failed to flush stream: {}", e)))
    }

    /// Closes the stream gracefully.
    ///
    /// This method flushes all pending records, waits for acknowledgments, and then
    /// closes the underlying gRPC connection. Always call this method when done with
    /// the stream to ensure data integrity.
    ///
    /// # Errors
    ///
    /// - Returns an error if some records could not be acknowledged
    /// - Network errors during the close operation
    #[napi]
    pub async fn close(&self) -> Result<()> {
        let mut guard = self.inner.lock().await;
        if let Some(mut stream) = guard.take() {
            stream
                .close()
                .await
                .map_err(|e| Error::from_reason(format!("Failed to close stream: {}", e)))?;
        }
        Ok(())
    }

    /// Gets the list of unacknowledged records.
    ///
    /// This method should only be called after a stream failure to retrieve records
    /// that were sent but not acknowledged by the server. These records can be
    /// re-ingested into a new stream.
    ///
    /// # Returns
    ///
    /// An array of Buffers containing the unacknowledged record payloads.
    #[napi]
    pub async fn get_unacked_records(&self) -> Result<Vec<Buffer>> {
        let guard = self.inner.lock().await;
        let stream = guard
            .as_ref()
            .ok_or_else(|| Error::from_reason("Stream has been closed"))?;

        let unacked = stream
            .get_unacked_records()
            .await
            .map_err(|e| Error::from_reason(format!("Failed to get unacked records: {}", e)))?;

        // Convert RecordPayload to Buffer (only return the bytes)
        Ok(unacked
            .into_iter()
            .map(|payload| match payload {
                RustRecordPayload::Proto(vec) => vec.into(),
                RustRecordPayload::Json(s) => s.into_bytes().into(),
            })
            .collect())
    }
}

/// The main SDK for interacting with the Databricks Zerobus service.
///
/// This is the entry point for creating ingestion streams to Delta tables.
///
/// # Example
///
/// ```typescript
/// const sdk = new ZerobusSdk(
///   "https://workspace-id.zerobus.region.cloud.databricks.com",
///   "https://workspace.cloud.databricks.com"
/// );
///
/// const stream = await sdk.createStream(
///   { tableName: "catalog.schema.table" },
///   "client-id",
///   "client-secret"
/// );
/// ```
#[napi]
pub struct ZerobusSdk {
    inner: RustZerobusSdk,
}

#[napi]
impl ZerobusSdk {
    /// Creates a new Zerobus SDK instance.
    ///
    /// # Arguments
    ///
    /// * `zerobus_endpoint` - The Zerobus API endpoint URL
    ///   (e.g., "https://workspace-id.zerobus.region.cloud.databricks.com")
    /// * `unity_catalog_url` - The Unity Catalog endpoint URL
    ///   (e.g., "https://workspace.cloud.databricks.com")
    ///
    /// # Errors
    ///
    /// - Invalid endpoint URLs
    /// - Failed to extract workspace ID from the endpoint
    #[napi(constructor)]
    pub fn new(zerobus_endpoint: String, unity_catalog_url: String) -> Result<Self> {
        let inner = RustZerobusSdk::new(zerobus_endpoint, unity_catalog_url)
            .map_err(|e| Error::from_reason(format!("Failed to create SDK: {}", e)))?;

        Ok(ZerobusSdk { inner })
    }

    /// Creates a new ingestion stream to a Delta table.
    ///
    /// This method establishes a bidirectional gRPC connection to the Zerobus service
    /// and prepares it for data ingestion. The stream handles authentication automatically
    /// using the provided OAuth credentials.
    ///
    /// # Arguments
    ///
    /// * `table_properties` - Properties of the target table including name and optional schema
    /// * `client_id` - OAuth 2.0 client ID
    /// * `client_secret` - OAuth 2.0 client secret
    /// * `options` - Optional stream configuration (uses defaults if not provided)
    ///
    /// # Returns
    ///
    /// A Promise that resolves to a ZerobusStream ready for data ingestion.
    ///
    /// # Errors
    ///
    /// - Authentication failures (invalid credentials)
    /// - Invalid table name or insufficient permissions
    /// - Network connectivity issues
    /// - Schema validation errors
    #[napi]
    pub async fn create_stream(
        &self,
        table_properties: TableProperties,
        client_id: String,
        client_secret: String,
        options: Option<StreamConfigurationOptions>,
    ) -> Result<ZerobusStream> {
        let rust_table_props = table_properties.to_rust()?;
        let rust_options = options.map(|o| o.into());

        let stream = self
            .inner
            .create_stream(rust_table_props, client_id, client_secret, rust_options)
            .await
            .map_err(|e| Error::from_reason(format!("Failed to create stream: {}", e)))?;

        Ok(ZerobusStream {
            inner: Arc::new(Mutex::new(Some(stream))),
        })
    }
}

/// Helper function to decode base64 strings.
fn base64_decode(input: &str) -> std::result::Result<Vec<u8>, String> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    STANDARD
        .decode(input)
        .map_err(|e| format!("Base64 decode error: {}", e))
}
