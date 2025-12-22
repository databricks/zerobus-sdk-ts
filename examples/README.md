# Zerobus SDK Examples

This directory contains example applications demonstrating the Zerobus Ingest SDK for TypeScript.

## Examples Overview

1. **`json.ts`** - JSON ingestion with individual and batch records
2. **`proto.ts`** - Protocol Buffers ingestion with individual and batch records
3. **`parallel_streams.ts`** - Multiple parallel streams for high throughput

Each example demonstrates:
- Creating a stream
- Ingesting 100 individual records with `ingestRecord()`
- Ingesting 10 records as a batch with `ingestRecords()`
- Type widening (high-level vs low-level serialization)
- Flushing and closing the stream
- Error recovery with `recreateStream()` (see README.md for detailed patterns)

## Setup

### 1. Install Dependencies

```bash
npm install
npm run build
```

### 2. Generate Protocol Buffers (for proto.ts example)

```bash
# Generate TypeScript code and descriptor file
npm run build:proto
```

This generates:
- `examples/generated/air_quality.js` - TypeScript bindings
- `examples/generated/air_quality.d.ts` - Type definitions
- `schemas/air_quality_descriptor.pb` - Descriptor file for SDK

### 3. Configure Credentials

Set the following environment variables:

```bash
# Required for all examples
export CLIENT_ID="your-service-principal-application-id"
export CLIENT_SECRET="your-service-principal-secret"
export TABLE_NAME="catalog.schema.table"

# For AWS
export ZEROBUS_ENDPOINT="workspace-id.zerobus.region.cloud.databricks.com"
export UNITY_CATALOG_URL="https://your-workspace.cloud.databricks.com"

# For Azure
export ZEROBUS_ENDPOINT="workspace-id.zerobus.region.azuredatabricks.net"
export UNITY_CATALOG_URL="https://your-workspace.azuredatabricks.net"
```

## Running Examples

### JSON Ingestion

```bash
npm run example:json
# or
npx tsx examples/json.ts
```

**Features demonstrated:**
- JSON record type configuration
- Type 1: Passing objects (high-level) - SDK auto-stringifies
- Type 2: Passing strings (low-level) - pre-serialized JSON
- Individual record ingestion (100 records)
- Batch ingestion (10 records)
- Recovery methods: `getUnackedRecords()` and `getUnackedBatches()`

### Protocol Buffers Ingestion

```bash
npm run example:proto
# or
npx tsx examples/proto.ts
```

**Features demonstrated:**
- Protocol Buffers record type configuration (default)
- Descriptor loading with `loadDescriptorProto()`
- Type 3: Passing Message objects (high-level) - SDK auto-serializes
- Type 4: Passing Buffers (low-level) - pre-serialized bytes
- Individual record ingestion (100 records)
- Batch ingestion (10 records)
- Recovery methods: `getUnackedRecords()` and `getUnackedBatches()`

### Parallel Streams

```bash
npm run example:parallel
# or
npx tsx examples/parallel_streams.ts
```

**Features demonstrated:**
- Multiple concurrent streams
- High-throughput parallel ingestion
- Independent error handling per stream

## Type Widening

The SDK supports 4 input types across JSON and Protocol Buffers modes:

### JSON Mode (RecordType.Json)

```typescript
// Type 1: object (high-level) - SDK auto-stringifies
await stream.ingestRecord({ device: 'sensor-1', temp: 25 });

// Type 2: string (low-level) - pre-serialized
await stream.ingestRecord(JSON.stringify({ device: 'sensor-1', temp: 25 }));
```

### Protocol Buffers Mode (RecordType.Proto)

```typescript
// Type 3: Message object (high-level) - SDK auto-serializes
const message = AirQuality.create({ device: 'sensor-1', temp: 25 });
await stream.ingestRecord(message);

// Type 4: Buffer (low-level) - pre-serialized bytes
const buffer = Buffer.from(AirQuality.encode(message).finish());
await stream.ingestRecord(buffer);
```

## Recovery and Error Handling

### Stream Recovery with recreateStream() (Recommended)

The SDK provides `recreateStream()` as the **recommended approach** for recovering from stream failures. This method automatically handles the entire recovery process:

```typescript
try {
  await stream.ingestRecords(batch);
} catch (error) {
  // Close the stream first
  await stream.close();

  // Optional: Inspect what needs recovery (must be called on closed stream)
  const unackedBatches = await stream.getUnackedBatches();
  console.log(`Batches to recover: ${unackedBatches.length}`);

  // Recreate stream with all unacked batches automatically re-ingested
  const newStream = await sdk.recreateStream(stream);
  console.log(`Stream recreated with ${unackedBatches.length} batches re-ingested`);

  // Continue using newStream
}
```

**What `recreateStream()` does:**
1. Retrieves all unacknowledged batches from the failed stream
2. Creates a new stream with identical configuration (same table, auth, options)
3. Re-ingests all unacknowledged batches in their original order
4. Returns the new stream ready for continued ingestion

**Benefits:**
- **Automatic**: No manual batch tracking or re-ingestion logic needed
- **Preserves structure**: Batches are re-ingested atomically as they were originally
- **Configuration preserved**: New stream has identical settings to the original
- **Single method call**: Simplifies error handling code

### Automatic Recovery for Transient Failures

The SDK also includes automatic recovery for transient failures (enabled by default with `recovery: true`). This handles temporary network issues without requiring manual intervention.

### Inspection Methods (For Debugging)

These methods are primarily for debugging and understanding which records were not acknowledged.

**Important:** These methods can **only be called on closed streams**. You must call `stream.close()` first.

#### getUnackedRecords()

Returns unacknowledged records as a flat array for inspection:

```typescript
await stream.close();  // Must close first
const unackedRecords = await stream.getUnackedRecords();
console.log(`Unacknowledged records: ${unackedRecords.length}`);
```

#### getUnackedBatches()

Returns unacknowledged records grouped by their original batches for inspection:

```typescript
await stream.close();  // Must close first
const unackedBatches = await stream.getUnackedBatches();
console.log(`Unacknowledged batches: ${unackedBatches.length}`);
```

**Note:** When using `recreateStream()`, these inspection methods are optional - the recreation process automatically handles all unacknowledged batches.

## Troubleshooting

### "protoc: command not found"
Install Protocol Buffer compiler:
```bash
# macOS
brew install protobuf

# Ubuntu/Debian
sudo apt-get install protobuf-compiler

# Windows
choco install protoc
```

### "Cannot find module './generated/air_quality'" or "Descriptor file not found"
Run the proto build script (generates both TypeScript files and descriptor):
```bash
npm run build:proto
```

### Authentication errors
Verify your service principal has the required permissions:
- `USE_CATALOG` on the catalog
- `USE_SCHEMA` on the schema
- `SELECT` and `MODIFY` on the table

## Next Steps

- See the [main README](../README.md) for complete SDK documentation
- Review the [API Reference](../README.md#api-reference) for all available methods
- Check the [Configuration](../README.md#configuration) section for stream tuning options
