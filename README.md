# Zerobus TypeScript SDK

TypeScript/Node.js SDK for streaming data ingestion into Databricks Delta tables using the Zerobus service. This SDK wraps the high-performance [Rust SDK](https://github.com/databricks/zerobus-sdk-rs) using native bindings for optimal performance.

## Private Preview

**This SDK is in Private Preview and is NOT intended for production workloads.** It is provided for evaluation and testing purposes only. The API is subject to change without notice, and minor version updates may include backwards-incompatible changes.

We are keen to hear feedback from you on this SDK. Please [file issues](https://github.com/databricks/zerobus-sdk-ts/issues), and we will address them.

## Features

- **High Performance** - Native Rust implementation with zero-copy data transfer
- **TypeScript Support** - Full type definitions for excellent IDE support
- **Async/Await** - Modern async API built on Promises
- **Automatic Recovery** - Built-in retry and reconnection for transient failures
- **JSON Encoding** - Simple JSON-based ingestion
- **Cross-Platform** - Pre-compiled binaries for Linux, macOS, and Windows

## Requirements

- **Node.js**: >= 16
- **Databricks workspace** with Zerobus access enabled
- **OAuth 2.0 credentials**: Service Principal with client ID and secret
- **Unity Catalog**: Target Delta table

## Prerequisites

Before using the SDK, you need to set up your Databricks environment:

### 1. Find Your Workspace Information

After logging into your Databricks workspace, check the browser URL:

```
https://<workspace-name>.cloud.databricks.com/?o=<workspace-id>
```

Extract:
- **Workspace URL**: `https://<workspace-name>.cloud.databricks.com`
- **Workspace ID**: `<workspace-id>`
- **Zerobus Endpoint**: `https://<workspace-id>.zerobus.<region>.cloud.databricks.com`

### 2. Create a Delta Table

Create a table in Databricks SQL Editor:

```sql
CREATE TABLE main.default.air_quality (
    device_name STRING,
    temp INT,
    humidity BIGINT
) USING DELTA;
```

### 3. Create Service Principal

1. Go to **Settings → Identity and Access → Service Principals**
2. Click **Add Service Principal**
3. Name it (e.g., `zerobus-ingestion-service`)
4. Click **Generate Secret** and save both the **Client ID** and **Secret** securely

### 4. Grant Permissions

```sql
-- Grant table permissions
GRANT MODIFY, SELECT ON TABLE main.default.air_quality
TO `<service-principal-name>`;

-- Grant catalog permissions
GRANT USE_CATALOG ON CATALOG main TO `<service-principal-name>`;
GRANT USE_SCHEMA ON SCHEMA main.default TO `<service-principal-name>`;
```

## Installation

```bash
npm install @databricks/zerobus-sdk
# or
yarn add @databricks/zerobus-sdk
# or
pnpm add @databricks/zerobus-sdk
```

## Quick Start

```typescript
import { ZerobusSdk } from '@databricks/zerobus-sdk';

// Create SDK instance
const sdk = new ZerobusSdk(
    'https://<workspace-id>.zerobus.<region>.cloud.databricks.com',
    'https://<workspace-name>.cloud.databricks.com'
);

// Create a stream with JSON encoding
const stream = await sdk.createStream(
    { tableName: 'main.default.air_quality' },
    process.env.CLIENT_ID!,
    process.env.CLIENT_SECRET!,
    { recordType: 0 }  // 0 = JSON (required!)
);

try {
    // Ingest records in a loop
    for (let i = 0; i < 100; i++) {
        const record = {
            device_name: `sensor-${i % 10}`,
            temp: 15 + (i % 20),
            humidity: 50 + (i % 40)
        };

        const offset = await stream.ingestRecord(JSON.stringify(record));
        console.log(`Record ${i} acknowledged at offset: ${offset}`);
    }

    console.log('All records ingested successfully');
} finally {
    // Always close the stream
    await stream.close();
}
```

## API Reference

### `ZerobusSdk`

Main entry point for the SDK.

#### Constructor

```typescript
new ZerobusSdk(zerobusEndpoint: string, unityCatalogUrl: string)
```

**Parameters:**
- `zerobusEndpoint` - Zerobus service URL (e.g., `https://<workspace-id>.zerobus.<region>.cloud.databricks.com`)
- `unityCatalogUrl` - Unity Catalog endpoint (e.g., `https://<workspace-name>.cloud.databricks.com`)

#### Methods

##### `createStream()`

```typescript
async createStream(
    tableProperties: TableProperties,
    clientId: string,
    clientSecret: string,
    options?: StreamConfigurationOptions
): Promise<ZerobusStream>
```

Creates a new ingestion stream to a Delta table.

**Parameters:**
- `tableProperties.tableName` - Full table name (e.g., `"main.default.air_quality"`)
- `clientId` - OAuth 2.0 client ID
- `clientSecret` - OAuth 2.0 client secret
- `options.recordType` - **Required!** Must be set to `0` for JSON encoding
- `options.maxInflightRecords` - Maximum unacknowledged records (default: 1,000,000)
- `options.recovery` - Enable automatic recovery (default: true)
- `options.recoveryTimeoutMs` - Recovery timeout in ms (default: 15,000)
- `options.recoveryBackoffMs` - Retry delay in ms (default: 2,000)
- `options.recoveryRetries` - Max retry attempts (default: 4)

### `ZerobusStream`

Represents an active ingestion stream.

#### Methods

##### `ingestRecord()`

```typescript
async ingestRecord(payload: Buffer | string): Promise<bigint>
```

Ingests a single record and waits for server acknowledgment.

**Parameters:**
- `payload` - JSON string containing the record data

**Returns:** Promise resolving to the offset ID

##### `flush()`

```typescript
async flush(): Promise<void>
```

Flushes all pending records and waits for acknowledgments.

##### `close()`

```typescript
async close(): Promise<void>
```

Closes the stream gracefully, flushing all pending data. **Always call this!**

##### `getUnackedRecords()`

```typescript
async getUnackedRecords(): Promise<Buffer[]>
```

Returns unacknowledged record payloads. Only call after stream failure.

## Configuration Options

```typescript
interface StreamConfigurationOptions {
    recordType: number;                // Must be 0 for JSON encoding (REQUIRED!)
    maxInflightRecords?: number;       // Default: 1,000,000
    recovery?: boolean;                // Default: true
    recoveryTimeoutMs?: number;        // Default: 15,000
    recoveryBackoffMs?: number;        // Default: 2,000
    recoveryRetries?: number;          // Default: 4
    flushTimeoutMs?: number;           // Default: 300,000
    serverLackOfAckTimeoutMs?: number; // Default: 60,000
}
```

## Examples

Complete examples are available in the [`examples/`](./examples) directory:

- [`basic_json.ts`](./examples/basic_json.ts) - Basic JSON ingestion with error handling
- [`parallel_streams.ts`](./examples/parallel_streams.ts) - Parallel ingestion using multiple streams

### Running Examples

```bash
# Install dependencies
npm install

# Set environment variables
export ZEROBUS_ENDPOINT="https://<workspace-id>.zerobus.<region>.cloud.databricks.com"
export UNITY_CATALOG_URL="https://<workspace-name>.cloud.databricks.com"
export CLIENT_ID="your-client-id"
export CLIENT_SECRET="your-client-secret"
export TABLE_NAME="main.default.air_quality"

# Run example (requires tsx)
npm install -g tsx
tsx examples/basic_json.ts
```

## Best Practices

1. **Reuse SDK Instances** - Create one `ZerobusSdk` instance and reuse it for multiple streams
2. **Always Close Streams** - Use try/finally blocks to ensure streams are closed
3. **Store Credentials Securely** - Use environment variables, never hardcode credentials
4. **Enable Recovery** - Use automatic recovery to handle transient failures
5. **Specify Record Type** - Always set `recordType: 0` for JSON ingestion

## Error Handling

```typescript
try {
    const offset = await stream.ingestRecord(JSON.stringify(record));
    console.log(`Success: offset ${offset}`);
} catch (error) {
    console.error('Ingestion failed:', error);

    // Retrieve unacknowledged records
    const unacked = await stream.getUnackedRecords();
    console.log(`${unacked.length} records were not acknowledged`);

    // Optionally re-ingest with a new stream
    const newStream = await sdk.createStream(tableProps, clientId, clientSecret, { recordType: 0 });
    for (const record of unacked) {
        await newStream.ingestRecord(record);
    }
    await newStream.close();
}
```

## Troubleshooting

### Common Errors

**"Record type is not specified"**
- Make sure to set `recordType: 0` in stream options

**"Record decoder/encoder error: unrecognized field name"**
- Your JSON fields don't match the table schema
- Check that field names and types match your Delta table exactly

**"Authentication failure"**
- Verify OAuth credentials are correct
- Check that Service Principal has permissions for the target table

**"Invalid table name"**
- Ensure table name follows pattern: `catalog.schema.table`
- Verify table exists in Unity Catalog

**"Installation failed"**
- If pre-built binaries aren't available for your platform:
  ```bash
  # Install Rust
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

  # Rebuild from source
  npm rebuild @databricks/zerobus-sdk --build-from-source
  ```

## Platform Support

Pre-compiled binaries are provided for:

- **Linux**: x64 (glibc, musl), ARM64 (glibc, musl)
- **macOS**: x64 (Intel), ARM64 (Apple Silicon)
- **Windows**: x64, ARM64

If your platform is not listed, the package will attempt to compile from source during installation.

## Architecture

This SDK wraps the high-performance [Rust Zerobus SDK](https://github.com/databricks/zerobus-sdk-rs) using [NAPI-RS](https://napi.rs):

```
┌─────────────────────────────┐
│   TypeScript Application    │
└─────────────┬───────────────┘
              │ (NAPI-RS bindings)
┌─────────────▼───────────────┐
│   Rust Zerobus SDK          │
│   - gRPC communication      │
│   - OAuth authentication    │
│   - Stream management       │
└─────────────┬───────────────┘
              │ (gRPC/TLS)
┌─────────────▼───────────────┐
│   Databricks Zerobus Service│
└─────────────────────────────┘
```

Benefits:
- **Zero-copy data transfer** between JavaScript and Rust
- **Native async/await support** - Rust futures become JavaScript Promises
- **Automatic memory management** - No manual cleanup required
- **Type safety** - Compile-time checks on both sides

## Building from Source

If you need to build the native addon from source:

### Prerequisites

- Node.js >= 16
- Rust toolchain (1.70+)
- Cargo

### Build Steps

```bash
# Clone the repository
git clone https://github.com/databricks/zerobus-sdk-ts.git
cd zerobus-sdk-ts

# Install dependencies
npm install

# Build the native addon
npm run build

# Run examples
tsx examples/basic_json.ts
```

## Contributing

We welcome contributions! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for details.

## Related Projects

- [Zerobus Rust SDK](https://github.com/databricks/zerobus-sdk-rs) - The underlying Rust implementation
- [Zerobus Python SDK](https://github.com/databricks/zerobus-sdk-py) - Python SDK for Zerobus
- [Zerobus Java SDK](https://github.com/databricks/zerobus-sdk-java) - Java SDK for Zerobus
- [NAPI-RS](https://napi.rs) - Rust/Node.js binding framework
