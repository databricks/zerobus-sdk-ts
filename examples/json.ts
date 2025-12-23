/**
 * JSON Ingestion Example
 *
 * This example demonstrates:
 * - Creating a stream with JSON record type
 * - Ingesting individual records with ingestRecord()
 * - Ingesting batch records with ingestRecords()
 * - Inspecting unacked records with getUnackedRecords() and getUnackedBatches()
 * - Type widening: object vs string
 * - Stream recovery using recreateStream() (recommended approach for failures)
 */

import { ZerobusSdk, StreamConfigurationOptions, TableProperties, RecordType } from '../index';

// Configuration
const ZEROBUS_ENDPOINT = process.env.ZEROBUS_SERVER_ENDPOINT || 'your-workspace-id.zerobus.region.cloud.databricks.com';
const UNITY_CATALOG_URL = process.env.DATABRICKS_WORKSPACE_URL || 'https://your-workspace.cloud.databricks.com';
const TABLE_NAME = process.env.ZEROBUS_TABLE_NAME || 'catalog.schema.table';
const CLIENT_ID = process.env.DATABRICKS_CLIENT_ID || 'your-oauth-client-id';
const CLIENT_SECRET = process.env.DATABRICKS_CLIENT_SECRET || 'your-oauth-client-secret';

async function main() {
    console.log('JSON Ingestion Example');
    console.log('='.repeat(60));

    // Validate configuration
    if (CLIENT_ID === 'your-oauth-client-id' || CLIENT_SECRET === 'your-oauth-client-secret') {
        console.error('Error: Please set DATABRICKS_CLIENT_ID and DATABRICKS_CLIENT_SECRET environment variables');
        return;
    }

    if (ZEROBUS_ENDPOINT === 'your-workspace-id.zerobus.region.cloud.databricks.com') {
        console.error('Error: Please set ZEROBUS_SERVER_ENDPOINT environment variable');
        return;
    }

    if (TABLE_NAME === 'catalog.schema.table') {
        console.error('Error: Please set ZEROBUS_TABLE_NAME environment variable');
        return;
    }

    try {
        // Step 1: Initialize SDK
        const sdk = new ZerobusSdk(ZEROBUS_ENDPOINT, UNITY_CATALOG_URL);
        console.log('✓ SDK initialized');

        // Step 2: Configure table properties (no descriptor needed for JSON)
        const tableProperties: TableProperties = {
            tableName: TABLE_NAME
        };

        // Step 3: Configure stream with JSON record type
        const options: StreamConfigurationOptions = {
            recordType: RecordType.Json,  // JSON mode
            maxInflightRequests: 1000,
            recovery: true
        };
        console.log('✓ Stream configured (JSON mode)');

        // Step 4: Create stream
        const stream = await sdk.createStream(
            tableProperties,
            CLIENT_ID,
            CLIENT_SECRET,
            options
        );
        console.log('✓ Stream created');

        try {
            // ===================================================================
            // Part 1: Ingest 100 individual records
            // ===================================================================
            console.log('\n[Part 1] Ingesting 100 individual records...');
            let lastAckPromise: Promise<bigint> | null = null;

            for (let i = 0; i < 100; i++) {
                const record = {
                    device_name: `sensor-${i % 10}`,
                    temp: 20 + (i % 15),
                    humidity: 50 + (i % 40)
                };

                // JSON supports 2 types:
                // Type 1 (high-level): object - SDK auto-stringifies
                lastAckPromise = stream.ingestRecord(record);

                // Type 2 (low-level): string - pre-serialized JSON
                // lastAckPromise = stream.ingestRecord(JSON.stringify(record));

                if ((i + 1) % 25 === 0) {
                    console.log(`  Sent ${i + 1}/100 records`);
                }
            }

            // Wait for last acknowledgment
            console.log('  Waiting for last acknowledgment...');
            if (lastAckPromise) {
                const lastOffset = await lastAckPromise;
                console.log(`✓ Last record acknowledged at offset ${lastOffset}`);
            }

            // ===================================================================
            // Part 2: Ingest 10 records as a batch
            // ===================================================================
            console.log('\n[Part 2] Ingesting 10 records as a batch...');

            const batchRecords = Array.from({ length: 10 }, (_, i) => ({
                device_name: `batch-sensor-${i}`,
                temp: 25 + i,
                humidity: 60 + i
            }));

            // Batch with Type 1: objects (high-level) - SDK auto-stringifies
            const batchOffsetId = await stream.ingestRecords(batchRecords);

            // Batch with Type 2: strings (low-level) - pre-serialized
            // const batchRecords = Array.from({ length: 10 }, (_, i) =>
            //     JSON.stringify({ device_name: `batch-sensor-${i}`, temp: 25 + i, humidity: 60 + i })
            // );
            // const batchOffsetId = await stream.ingestRecords(batchRecords);

            if (batchOffsetId !== null) {
                console.log(`✓ Batch acknowledged at offset ${batchOffsetId}`);
            } else {
                console.log('  Note: Batch was empty');
            }

            // ===================================================================
            // Part 3: Flush and close
            // ===================================================================
            console.log('\n[Part 3] Flushing and closing stream...');
            await stream.flush();
            console.log('✓ Stream flushed');

            await stream.close();
            console.log('✓ Stream closed');

            // Note: getUnackedRecords() and getUnackedBatches() are available for
            // inspection after failures. See the error handler below and README.md
            // for recovery patterns using recreateStream().

            // Summary
            console.log('\n' + '='.repeat(60));
            console.log('Summary:');
            console.log('  Individual records: 100');
            console.log('  Batch records: 10');
            console.log('  Total: 110 records');
            console.log('  Mode: JSON');
            console.log('='.repeat(60));

        } catch (error) {
            console.error(`\n✗ Error during ingestion: ${error}`);

            // When stream fails, close it and use recreateStream() for recovery
            await stream.close();
            console.log('Stream closed after error');

            // Recommended recovery approach: Use recreateStream()
            // This automatically retrieves unacknowledged batches, creates a new stream,
            // and re-ingests them. See README.md for detailed recovery patterns.
            console.log('\nFor recovery, use: const newStream = await sdk.recreateStream(stream);');

            throw error;
        }

    } catch (error) {
        console.error(`\n✗ Failed: ${error}`);
        throw error;
    }
}

// Run the example
if (require.main === module) {
    main().catch((error) => {
        console.error('Fatal error:', error);
        process.exit(1);
    });
}
