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
use napi::threadsafe_function::{ThreadsafeFunction, ErrorStrategy};
use napi::{Env, JsObject, JsFunction, JsUnknown, JsString, JsGlobal, ValueType};
use napi_derive::napi;

use databricks_zerobus_ingest_sdk::{
    EncodedRecord as RustRecordPayload,
    StreamConfigurationOptions as RustStreamOptions,
    TableProperties as RustTableProperties, ZerobusSdk as RustZerobusSdk,
    ZerobusStream as RustZerobusStream,
    HeadersProvider as RustHeadersProvider,
    ZerobusError as RustZerobusError,
    ZerobusResult as RustZerobusResult,
};
use databricks_zerobus_ingest_sdk::databricks::zerobus::RecordType as RustRecordType;
use async_trait::async_trait;
use prost_types;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Record serialization format.
///
/// Specifies how records should be encoded when ingested into the stream.
#[napi]
pub enum RecordType {
    /// JSON encoding - records are JSON-encoded strings
    Json = 0,
    /// Protocol Buffers encoding - records are binary protobuf messages
    Proto = 1,
}

/// Configuration options for the Zerobus stream.
///
/// These options control stream behavior including recovery, timeouts, and inflight limits.
#[napi(object)]
#[derive(Debug, Clone)]
pub struct StreamConfigurationOptions {
    /// Maximum number of unacknowledged requests that can be in flight.
    /// Default: 10,000
    pub max_inflight_requests: Option<u32>,

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

    /// Record serialization format.
    /// Use RecordType.Json for JSON encoding or RecordType.Proto for Protocol Buffers.
    /// Default: RecordType.Proto (Protocol Buffers)
    pub record_type: Option<i32>,
}

impl From<StreamConfigurationOptions> for RustStreamOptions {
    fn from(opts: StreamConfigurationOptions) -> Self {
        let default = RustStreamOptions::default();

        let record_type = match opts.record_type {
            Some(0) => RustRecordType::Json,
            Some(1) => RustRecordType::Proto,
            _ => RustRecordType::Proto,
        };

        RustStreamOptions {
            max_inflight_requests: opts.max_inflight_requests.unwrap_or(default.max_inflight_requests as u32) as usize,
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

/// Helper function to convert a JavaScript value to a RustRecordPayload.
///
/// Supports:
/// - Buffer (low-level proto bytes)
/// - string (low-level JSON string)
/// - Protobuf message object with .encode() method (high-level, auto-serializes)
/// - Plain JavaScript object (high-level, auto-stringifies to JSON)
fn convert_js_to_record_payload(env: &Env, payload: Unknown) -> Result<RustRecordPayload> {
    let value_type = payload.get_type()?;

    match value_type {
        ValueType::Object => {
            let js_value: JsUnknown = payload.try_into()?;
            if js_value.is_buffer()? {
                let buffer: Buffer = Buffer::from_unknown(js_value)?;
                return Ok(RustRecordPayload::Proto(buffer.to_vec()));
            }

            let obj: JsObject = JsObject::from_unknown(js_value)?;

            let constructor: JsFunction = obj.get_named_property("constructor")?;
            let constructor_obj = JsObject::from_unknown(constructor.into_unknown())?;

            if constructor_obj.has_named_property("encode")? {
                let encode_fn: JsFunction = constructor_obj.get_named_property("encode")?;
                let obj_as_unknown = obj.into_unknown();
                let encode_result: JsUnknown = encode_fn.call::<JsUnknown>(Some(&constructor_obj), &[obj_as_unknown])?;
                let encode_obj = JsObject::from_unknown(encode_result)?;

                if encode_obj.has_named_property("finish")? {
                    let finish_fn: JsFunction = encode_obj.get_named_property("finish")?;
                    let buffer_result: JsUnknown = finish_fn.call::<JsUnknown>(Some(&encode_obj), &[])?;
                    let buffer: Buffer = Buffer::from_unknown(buffer_result)?;
                    Ok(RustRecordPayload::Proto(buffer.to_vec()))
                } else {
                    Err(Error::from_reason(
                        "Protobuf message .encode() must return an object with .finish() method"
                    ))
                }
            } else {
                let global: JsGlobal = env.get_global()?;
                let json_obj: JsObject = global.get_named_property("JSON")?;
                let stringify: JsFunction = json_obj.get_named_property("stringify")?;
                let obj_as_unknown = obj.into_unknown();
                let str_result: JsUnknown = stringify.call::<JsUnknown>(Some(&json_obj), &[obj_as_unknown])?;
                let js_string = JsString::from_unknown(str_result)?;
                let json_string = js_string.into_utf8()?.as_str()?.to_string();

                Ok(RustRecordPayload::Json(json_string))
            }
        }
        ValueType::String => {
            let js_value: JsUnknown = payload.try_into()?;
            let js_string = JsString::from_unknown(js_value)?;
            let json_string = js_string.into_utf8()?.as_str()?.to_string();
            Ok(RustRecordPayload::Json(json_string))
        }
        _ => {
            Err(Error::from_reason(
                "Payload must be a Buffer, string, protobuf message object, or plain JavaScript object"
            ))
        }
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
    /// Ingests a single record into the stream.
    ///
    /// This method accepts either:
    /// - A Protocol Buffer encoded record as a Buffer (Vec<u8>)
    /// - A JSON string
    ///
    /// This method BLOCKS until the record is sent to the SDK's internal landing zone,
    /// then returns a Promise for the server acknowledgment. This allows you to send
    /// many records immediately without waiting for acknowledgments:
    ///
    /// ```typescript
    /// let lastAckPromise;
    /// for (let i = 0; i < 1000; i++) {
    ///     // This call blocks until record is sent (in SDK)
    ///     lastAckPromise = stream.ingestRecord(record);
    /// }
    /// // All 1000 records are now in the SDK's internal queue
    /// // Wait for the last acknowledgment
    /// await lastAckPromise;
    /// // Flush to ensure all records are acknowledged
    /// await stream.flush();
    /// ```
    ///
    /// # Arguments
    ///
    /// * `payload` - The record data. Accepts:
    ///   - Buffer (low-level proto bytes)
    ///   - string (low-level JSON string)
    ///   - Protobuf message object with .encode() method (high-level, auto-serializes)
    ///   - Plain JavaScript object (high-level, auto-stringifies to JSON)
    ///
    /// # Returns
    ///
    /// A Promise that resolves to the offset ID when the server acknowledges the record.
    #[napi(ts_return_type = "Promise<bigint>")]
    pub fn ingest_record(&self, env: Env, payload: Unknown) -> Result<JsObject> {
        let record_payload = convert_js_to_record_payload(&env, payload)?;

        let ack_future = {
            let handle = tokio::runtime::Handle::current();
            let stream = self.inner.clone();

            handle.block_on(async move {
                let mut guard = stream.lock().await;
                let stream_ref = guard
                    .as_mut()
                    .ok_or_else(|| Error::from_reason("Stream has been closed"))?;

                stream_ref
                    .ingest_record(record_payload)
                    .await
                    .map_err(|e| Error::from_reason(format!("Failed to ingest record: {}", e)))
            })?
        };

        env.execute_tokio_future(
            async move {
                ack_future
                    .await
                    .map_err(|e| napi::Error::from_reason(format!("Acknowledgment failed: {}", e)))
            },
            |env, result| {
                let result_str = result.to_string();
                let global: JsGlobal = env.get_global()?;
                let bigint_ctor: JsFunction = global.get_named_property("BigInt")?;
                let js_str = env.create_string(&result_str)?;
                bigint_ctor.call(None, &[js_str.into_unknown()])
            },
        )
    }

    /// Ingests multiple records as a single atomic batch.
    ///
    /// This method accepts an array of records (Protocol Buffer buffers or JSON strings)
    /// and ingests them as a batch. The batch receives a single acknowledgment from
    /// the server with all-or-nothing semantics.
    ///
    /// Similar to ingestRecord(), this BLOCKS until the batch is sent to the SDK's
    /// internal landing zone, then returns a Promise for the server acknowledgment.
    ///
    /// # Arguments
    ///
    /// * `records` - Array of record data (Buffer for protobuf, string for JSON)
    ///
    /// # Returns
    ///
    /// Promise resolving to:
    /// - `bigint`: offset ID for non-empty batches
    /// - `null`: for empty batches
    ///
    /// # Example
    ///
    /// ```typescript
    /// const buffers = records.map(r => Buffer.from(encode(r)));
    /// const offsetId = await stream.ingestRecords(buffers);
    ///
    /// if (offsetId !== null) {
    ///   console.log('Batch acknowledged at offset:', offsetId);
    /// }
    /// ```
    #[napi(ts_return_type = "Promise<bigint | null>")]
    pub fn ingest_records(&self, env: Env, records: Vec<Unknown>) -> Result<JsObject> {
        let record_payloads: Result<Vec<RustRecordPayload>> = records
            .into_iter()
            .map(|payload| convert_js_to_record_payload(&env, payload))
            .collect();

        let record_payloads = record_payloads?;

        let ack_future_option = {
            let handle = tokio::runtime::Handle::current();
            let stream = self.inner.clone();

            handle.block_on(async move {
                let mut guard = stream.lock().await;
                let stream_ref = guard
                    .as_mut()
                    .ok_or_else(|| Error::from_reason("Stream has been closed"))?;

                // Send batch to SDK
                stream_ref
                    .ingest_records(record_payloads)
                    .await
                    .map_err(|e| Error::from_reason(format!("Failed to ingest batch: {}", e)))
            })?
        };

        env.execute_tokio_future(
            async move {
                match ack_future_option.await {
                    Ok(Some(offset_id)) => Ok(Some(offset_id)),
                    Ok(None) => Ok(None),
                    Err(e) => Err(napi::Error::from_reason(
                        format!("Batch acknowledgment failed: {}", e)
                    )),
                }
            },
            |env, result| match result {
                Some(offset_id) => {
                    let offset_str = offset_id.to_string();
                    let global: JsGlobal = env.get_global()?;
                    let bigint_ctor: JsFunction = global.get_named_property("BigInt")?;
                    let js_str = env.create_string(&offset_str)?;
                    let bigint = bigint_ctor.call(None, &[js_str.into_unknown()])?;
                    Ok(bigint.into_unknown())
                },
                None => env.get_null().map(|v| v.into_unknown()),
            },
        )
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

        Ok(unacked
            .into_iter()
            .map(|payload| match payload {
                RustRecordPayload::Proto(vec) => vec.into(),
                RustRecordPayload::Json(s) => s.into_bytes().into(),
            })
            .collect())
    }

    /// Gets unacknowledged records grouped by their original batches.
    ///
    /// This preserves the batch structure from ingestion:
    /// - Each ingestRecord() call → 1-element batch
    /// - Each ingestRecords() call → N-element batch
    ///
    /// Should only be called after stream failure. All records returned as Buffers
    /// (JSON strings are converted to UTF-8 bytes).
    ///
    /// # Returns
    ///
    /// Array of batches, where each batch is an array of Buffers
    ///
    /// # Example
    ///
    /// ```typescript
    /// try {
    ///   await stream.ingestRecords(batch1);
    ///   await stream.ingestRecords(batch2);
    /// } catch (error) {
    ///   const unackedBatches = await stream.getUnackedBatches();
    ///
    ///   // Re-ingest with new stream
    ///   for (const batch of unackedBatches) {
    ///     await newStream.ingestRecords(batch);
    ///   }
    /// }
    /// ```
    #[napi]
    pub async fn get_unacked_batches(&self) -> Result<Vec<Vec<Buffer>>> {
        let guard = self.inner.lock().await;
        let stream = guard
            .as_ref()
            .ok_or_else(|| Error::from_reason("Stream has been closed"))?;

        let unacked_batches = stream
            .get_unacked_batches()
            .await
            .map_err(|e| Error::from_reason(format!("Failed to get unacked batches: {}", e)))?;

        Ok(unacked_batches
            .into_iter()
            .map(|batch| {
                batch
                    .into_iter()
                    .map(|record| match record {
                        RustRecordPayload::Proto(vec) => vec.into(),
                        RustRecordPayload::Json(s) => s.into_bytes().into(),
                    })
                    .collect()
            })
            .collect())
    }
}

/// JavaScript headers provider callback wrapper.
///
/// Allows TypeScript code to provide custom authentication headers
/// by implementing a getHeaders() function.
#[napi(object)]
pub struct JsHeadersProvider {
    /// JavaScript function: () => Promise<Array<[string, string]>>
    pub get_headers_callback: JsFunction,
}

/// Internal adapter that wraps static headers as a HeadersProvider
/// This is used for custom authentication in the TypeScript SDK
struct StaticHeadersProvider {
    headers: HashMap<&'static str, String>,
}

impl StaticHeadersProvider {
    fn new(headers: Vec<(String, String)>) -> RustZerobusResult<Self> {
        // Convert Vec<(String, String)> to HashMap<&'static str, String>
        // We need to leak strings to get 'static lifetime for keys
        let mut map = HashMap::new();
        for (k, v) in headers {
            let static_key: &'static str = Box::leak(k.into_boxed_str());
            map.insert(static_key, v);
        }

        if !map.contains_key("authorization") {
            return Err(RustZerobusError::InvalidArgument(
                "HeadersProvider must include 'authorization' header with Bearer token".to_string()
            ));
        }
        if !map.contains_key("x-databricks-zerobus-table-name") {
            return Err(RustZerobusError::InvalidArgument(
                "HeadersProvider must include 'x-databricks-zerobus-table-name' header".to_string()
            ));
        }

        Ok(Self { headers: map })
    }
}

#[async_trait]
impl RustHeadersProvider for StaticHeadersProvider {
    async fn get_headers(&self) -> RustZerobusResult<HashMap<&'static str, String>> {
        Ok(self.headers.clone())
    }
}

/// Helper to create a threadsafe function from JavaScript callback
fn create_headers_tsfn(js_func: JsFunction) -> Result<ThreadsafeFunction<(), ErrorStrategy::Fatal>> {
    js_func.create_threadsafe_function(0, |ctx| Ok(vec![ctx.value]))
}

/// Helper to call headers callback and get result
async fn call_headers_tsfn(tsfn: ThreadsafeFunction<(), ErrorStrategy::Fatal>) -> Result<Vec<(String, String)>> {
    tsfn.call_async::<Vec<(String, String)>>(())
        .await
        .map_err(|e| Error::from_reason(format!("Failed to call headers callback: {}", e)))
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
    inner: Arc<RustZerobusSdk>,
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

        Ok(ZerobusSdk { inner: Arc::new(inner) })
    }

    /// Creates a new ingestion stream to a Delta table.
    ///
    /// This method establishes a bidirectional gRPC connection to the Zerobus service
    /// and prepares it for data ingestion. By default, it uses OAuth 2.0 Client Credentials
    /// authentication. For custom authentication (e.g., Personal Access Tokens), provide
    /// a custom headers_provider.
    ///
    /// # Arguments
    ///
    /// * `table_properties` - Properties of the target table including name and optional schema
    /// * `client_id` - OAuth 2.0 client ID (ignored if headers_provider is provided)
    /// * `client_secret` - OAuth 2.0 client secret (ignored if headers_provider is provided)
    /// * `options` - Optional stream configuration (uses defaults if not provided)
    /// * `headers_provider` - Optional custom headers provider for authentication.
    ///   If not provided, uses OAuth with client_id and client_secret.
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
    ///
    /// # Examples
    ///
    /// OAuth authentication (default):
    /// ```typescript
    /// const stream = await sdk.createStream(
    ///   { tableName: "catalog.schema.table" },
    ///   "client-id",
    ///   "client-secret"
    /// );
    /// ```
    ///
    /// Custom authentication with headers provider:
    /// ```typescript
    /// const headersProvider = {
    ///   getHeadersCallback: async () => [
    ///     ["authorization", `Bearer ${myToken}`],
    ///     ["x-databricks-zerobus-table-name", tableName]
    ///   ]
    /// };
    /// const stream = await sdk.createStream(
    ///   { tableName: "catalog.schema.table" },
    ///   "", // ignored
    ///   "", // ignored
    ///   undefined,
    ///   headersProvider
    /// );
    /// ```
    #[napi(ts_return_type = "Promise<ZerobusStream>")]
    pub fn create_stream(
        &self,
        env: Env,
        table_properties: TableProperties,
        client_id: String,
        client_secret: String,
        options: Option<StreamConfigurationOptions>,
        headers_provider: Option<JsHeadersProvider>,
    ) -> Result<JsObject> {
        let rust_table_props = table_properties.to_rust()?;
        let rust_options = options.map(|o| o.into());

        let headers_tsfn = match headers_provider {
            Some(JsHeadersProvider { get_headers_callback }) => {
                Some(create_headers_tsfn(get_headers_callback)?)
            }
            None => None,
        };

        let sdk = self.inner.clone();

        env.execute_tokio_future(
            async move {
                let headers_provider_arc = if let Some(tsfn) = headers_tsfn {
                    let headers = call_headers_tsfn(tsfn).await
                        .map_err(|e| napi::Error::from_reason(format!("Headers callback failed: {}", e)))?;

                    let static_provider = StaticHeadersProvider::new(headers)
                        .map_err(|e| napi::Error::from_reason(format!("Invalid headers: {}", e)))?;

                    Some(Arc::new(static_provider) as Arc<dyn RustHeadersProvider>)
                } else {
                    None
                };

                let stream = if let Some(provider) = headers_provider_arc {
                    sdk
                        .create_stream_with_headers_provider(
                            rust_table_props,
                            provider,
                            rust_options,
                        )
                        .await
                        .map_err(|e| napi::Error::from_reason(format!("Failed to create stream: {}", e)))?
                } else {
                    sdk
                        .create_stream(rust_table_props, client_id, client_secret, rust_options)
                        .await
                        .map_err(|e| napi::Error::from_reason(format!("Failed to create stream: {}", e)))?
                };

                Ok(ZerobusStream {
                    inner: Arc::new(Mutex::new(Some(stream))),
                })
            },
            |_env, stream| Ok(stream),
        )
    }

    /// Recreates a stream with the same configuration and re-ingests unacknowledged batches.
    ///
    /// This method is the recommended approach for recovering from stream failures. It:
    /// 1. Retrieves all unacknowledged batches from the failed stream
    /// 2. Creates a new stream with identical configuration
    /// 3. Re-ingests all unacknowledged batches in order
    /// 4. Returns the new stream ready for continued ingestion
    ///
    /// # Arguments
    ///
    /// * `stream` - The failed or closed stream to recreate
    ///
    /// # Returns
    ///
    /// A Promise that resolves to a new ZerobusStream with all unacknowledged batches re-ingested.
    ///
    /// # Errors
    ///
    /// - Failed to retrieve unacknowledged batches from the original stream
    /// - Authentication failures when creating the new stream
    /// - Network connectivity issues during re-ingestion
    ///
    /// # Examples
    ///
    /// ```typescript
    /// try {
    ///   await stream.ingestRecords(batch);
    /// } catch (error) {
    ///   await stream.close();
    ///   // Recreate stream with all unacked batches re-ingested
    ///   const newStream = await sdk.recreateStream(stream);
    ///   // Continue ingesting with newStream
    /// }
    /// ```
    #[napi]
    pub async fn recreate_stream(&self, stream: &ZerobusStream) -> Result<ZerobusStream> {
        let inner_guard = stream.inner.lock().await;
        let rust_stream = inner_guard
            .as_ref()
            .ok_or_else(|| Error::from_reason("Stream has been closed"))?;

        let new_rust_stream = self
            .inner
            .recreate_stream(rust_stream)
            .await
            .map_err(|e| Error::from_reason(format!("Failed to recreate stream: {}", e)))?;

        Ok(ZerobusStream {
            inner: Arc::new(Mutex::new(Some(new_rust_stream))),
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
