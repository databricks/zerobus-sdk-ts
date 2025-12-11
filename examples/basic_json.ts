/**
 * Zerobus SDK - Basic JSON Ingestion Example
 *
 * This example demonstrates how to use the Zerobus TypeScript SDK to ingest
 * JSON-encoded records into a Databricks Delta table.
 *
 * Prerequisites:
 * 1. Databricks workspace with Zerobus enabled
 * 2. Service Principal with OAuth credentials (client ID + secret)
 * 3. Delta table matching the schema below
 * 4. Appropriate permissions (MODIFY, SELECT) on the table
 *
 * Table Schema (air_quality):
 *   - device_name: STRING
 *   - temp: INT
 *   - humidity: BIGINT
 *
 * Steps shown:
 * 1. Create SDK instance with endpoints
 * 2. Create a stream with JSON record type
 * 3. Ingest records in a loop
 * 4. Handle errors and cleanup
 */

import { ZerobusSdk, StreamConfigurationOptions, TableProperties } from '../index';

async function main() {
    // Configuration - replace with your own values or set environment variables
    const zerobusEndpoint = process.env.ZEROBUS_ENDPOINT || 'https://your-workspace-id.zerobus.region.cloud.databricks.com';
    const unityCatalogUrl = process.env.UNITY_CATALOG_URL || 'https://your-workspace.cloud.databricks.com';
    const clientId = process.env.CLIENT_ID || 'your-client-id';
    const clientSecret = process.env.CLIENT_SECRET || 'your-client-secret';
    const tableName = process.env.TABLE_NAME || 'catalog.schema.table';

    console.log('Creating Zerobus SDK...');
    // Step 1: Initialize the SDK with Databricks endpoints
    const sdk = new ZerobusSdk(zerobusEndpoint, unityCatalogUrl);

    // Step 2: Configure the target table
    const tableProperties: TableProperties = {
        tableName: tableName,  // Full table name: catalog.schema.table
    };

    // Step 3: Configure stream options
    // IMPORTANT: recordType must be specified (0 = JSON, 1 = Proto)
    const options: StreamConfigurationOptions = {
        recordType: 0,  // REQUIRED: 0 for JSON encoding
        maxInflightRecords: 10000,  // Limit memory usage
        recovery: true,  // Enable automatic retry on failures
        recoveryTimeoutMs: 15000,  // Wait up to 15s for recovery
        recoveryBackoffMs: 2000,  // Wait 2s between retries
        recoveryRetries: 4,  // Retry up to 4 times
    };

    console.log('Creating stream...');
    // Step 4: Create the ingestion stream
    // This establishes a gRPC connection and authenticates with Databricks
    const stream = await sdk.createStream(
        tableProperties,
        clientId,
        clientSecret,
        options
    );

    try {
        console.log('Ingesting records...');

        // Step 5: Ingest a single record (useful for testing)
        // Records must match the table schema exactly
        const record1 = {
            device_name: 'sensor-1',
            temp: 20,
            humidity: 65
        };

        const offset1 = await stream.ingestRecord(JSON.stringify(record1));
        console.log(`Record 1 acknowledged at offset: ${offset1}`);

        // Step 6: Ingest multiple records
        // We collect promises without awaiting to send records in parallel
        const promises: Promise<bigint>[] = [];

        for (let i = 2; i <= 100; i++) {
            const record = {
                device_name: `sensor-${i % 10}`,
                temp: 15 + (i % 20),
                humidity: 50 + (i % 40)
            };

            // Send record immediately without waiting for acknowledgment
            const promise = stream.ingestRecord(JSON.stringify(record));
            promises.push(promise);

            // Flush periodically to:
            // 1. Free up memory from acknowledged records
            // 2. Ensure data is persisted regularly
            if (i % 50 === 0) {
                console.log(`Ingested ${i} records, flushing...`);
                await stream.flush();
            }
        }

        // Step 7: Wait for all remaining acknowledgments
        // This ensures all records have been durably written
        console.log('Waiting for all acknowledgments...');
        const offsets = await Promise.all(promises);
        console.log(`All ${offsets.length} records acknowledged`);
        console.log(`Last offset: ${offsets[offsets.length - 1]}`);

        // Step 8: Final flush to ensure everything is persisted
        console.log('Final flush...');
        await stream.flush();

        console.log('Success! All records ingested.');
    } catch (error) {
        console.error('Error during ingestion:', error);

        // On error, try to retrieve unacknowledged records
        // These can be re-ingested in a new stream
        try {
            const unacked = await stream.getUnackedRecords();
            console.error(`Failed to acknowledge ${unacked.length} records`);
            // In production, you would store these for retry
        } catch (unackedError) {
            console.error('Could not retrieve unacknowledged records:', unackedError);
        }

        throw error;
    } finally {
        // Step 9: Always close the stream to free resources
        console.log('Closing stream...');
        try {
            await stream.close();
            console.log('Stream closed successfully');
        } catch (closeError) {
            console.error('Error closing stream:', closeError);
        }
    }
}

// Run the example
main().catch((error) => {
    console.error('Fatal error:', error);
    process.exit(1);
});
