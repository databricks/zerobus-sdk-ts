# Version changelog

## Release v0.1.0

Initial release of the Databricks Zerobus Ingest SDK for TypeScript.

### New Features and Improvements

- High-throughput data ingestion into Databricks Delta tables using native Rust implementation
- Support for JSON and Protocol Buffers serialization formats
- OAuth 2.0 client credentials authentication
- Batch ingestion API with `ingestRecords()` for higher throughput
- Type widening support for flexible record input:
  - JSON mode: Accept objects (auto-stringify) or strings (pre-serialized)
  - Protocol Buffers mode: Accept Message objects (auto-serialize) or Buffers (pre-serialized)
- Stream recovery mechanisms with `getUnackedRecords()` and `getUnackedBatches()`
- Automatic retry and recovery for transient failures
- Protocol Buffer descriptor utilities with `loadDescriptorProto()`
- Cross-platform support (Linux, macOS, Windows)

### API Changes

- Added `ZerobusSdk` class for creating ingestion streams
- Added `ZerobusStream` class for managing stateful gRPC streams
- Added `createStream()` method with optional `headers_provider` parameter
- Added `ingestRecord()` method accepting Buffer, string, or object types
- Added `ingestRecords()` method for batch ingestion
- Added `getUnackedRecords()` and `getUnackedBatches()` for recovery
- Added `TableProperties` interface for table configuration
- Added `StreamConfigurationOptions` interface with `recordType` parameter
- Added `RecordType` enum with `Json` and `Proto` values
- Added `HeadersProvider` interface for custom authentication
- Support for Node.js >= 16

### Documentation

- Comprehensive README with quick start guide
- Protocol Buffer setup instructions
- Type mapping guide (Delta ↔ Proto)
- API reference documentation
- Examples: `json.ts`, `proto.ts`, `parallel_streams.ts`
