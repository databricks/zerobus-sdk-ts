/**
 * Zerobus SDK - Parallel Streams Example
 *
 * This example demonstrates parallel ingestion by partitioning data
 * across multiple parallel streams, each with its own gRPC connection.
 *
 * NOTE: This example uses JSON mode for simplicity. For production, consider
 * using Protocol Buffers (the default format) for better performance.
 *
 * Use case: Improved ingestion performance through parallelization
 *
 * Architecture:
 * - Creates N worker streams (default: 4)
 * - Each worker gets a partition of the data
 * - Workers run concurrently, each with independent gRPC connection
 * - Results are aggregated and stats calculated
 *
 * Benefits:
 * - Higher throughput than single stream
 * - Better CPU/network utilization
 * - Scales horizontally
 *
 * Trade-offs:
 * - Uses more resources (connections, memory)
 * - More complex error handling
 * - Records from different workers may not be strictly ordered
 */

import { ZerobusSdk, TableProperties, RecordType } from '../index';

const TOTAL_RECORDS = 10000;
const NUM_WORKERS = 4;
const RECORDS_PER_WORKER = TOTAL_RECORDS / NUM_WORKERS;

interface WorkerResult {
    workerId: number;
    recordsIngested: number;
    timeMs: number;
}

/**
 * Worker function that runs in parallel to ingest a partition of data.
 *
 * Each worker:
 * 1. Creates its own stream
 * 2. Ingests records in its assigned range
 * 3. Flushes periodically to control memory
 * 4. Measures throughput
 * 5. Closes stream when done
 */
async function worker(
    sdk: ZerobusSdk,
    workerId: number,
    tableProperties: TableProperties,
    clientId: string,
    clientSecret: string,
    startId: number,
    endId: number
): Promise<WorkerResult> {
    const startTime = Date.now();

    console.log(`Worker ${workerId}: Creating stream...`);
    // Each worker gets its own independent stream/connection
    const stream = await sdk.createStream(
        tableProperties,
        clientId,
        clientSecret,
        { recordType: RecordType.Json }  // JSON mode
    );

    try {
        console.log(`Worker ${workerId}: Ingesting records ${startId} to ${endId}...`);

        const promises: Promise<bigint>[] = [];

        // Ingest the worker's partition of data
        for (let i = startId; i < endId; i++) {
            const record = {
                device_name: `sensor-${workerId}-${i % 10}`,
                temp: 15 + (i % 20),
                humidity: 50 + (i % 40)
            };

            promises.push(stream.ingestRecord(JSON.stringify(record)));

            // Flush periodically to avoid unbounded memory growth
            if (promises.length >= 1000) {
                await stream.flush();
                promises.length = 0; // Clear the array
            }
        }

        // Wait for remaining acknowledgments
        if (promises.length > 0) {
            await Promise.all(promises);
        }

        // Final flush
        await stream.flush();

        const timeMs = Date.now() - startTime;
        const recordsIngested = endId - startId;

        console.log(
            `Worker ${workerId}: Completed ${recordsIngested} records in ${timeMs}ms ` +
            `(${(recordsIngested / (timeMs / 1000)).toFixed(0)} records/sec)`
        );

        return { workerId, recordsIngested, timeMs };
    } finally {
        await stream.close();
    }
}

async function main() {
    const zerobusEndpoint = process.env.ZEROBUS_SERVER_ENDPOINT || 'https://your-workspace-id.zerobus.region.cloud.databricks.com';
    const unityCatalogUrl = process.env.DATABRICKS_WORKSPACE_URL || 'https://your-workspace.cloud.databricks.com';
    const clientId = process.env.DATABRICKS_CLIENT_ID || 'your-client-id';
    const clientSecret = process.env.DATABRICKS_CLIENT_SECRET || 'your-client-secret';
    const tableName = process.env.ZEROBUS_TABLE_NAME || 'catalog.schema.table';

    console.log(`Starting parallel ingestion with ${NUM_WORKERS} workers...`);
    console.log(`Total records: ${TOTAL_RECORDS}`);

    const sdk = new ZerobusSdk(zerobusEndpoint, unityCatalogUrl);

    const tableProperties: TableProperties = {
        tableName: tableName,
    };

    const totalStartTime = Date.now();

    // Launch all workers in parallel
    const workerPromises: Promise<WorkerResult>[] = [];

    for (let i = 0; i < NUM_WORKERS; i++) {
        const startId = i * RECORDS_PER_WORKER;
        const endId = (i + 1) * RECORDS_PER_WORKER;

        workerPromises.push(
            worker(sdk, i, tableProperties, clientId, clientSecret, startId, endId)
        );
    }

    // Wait for all workers to complete
    const results = await Promise.all(workerPromises);

    const totalTimeMs = Date.now() - totalStartTime;
    const totalRecordsIngested = results.reduce((sum, r) => sum + r.recordsIngested, 0);

    console.log('\n=== Summary ===');
    console.log(`Total records ingested: ${totalRecordsIngested}`);
    console.log(`Total time: ${totalTimeMs}ms`);
    console.log(`Overall throughput: ${(totalRecordsIngested / (totalTimeMs / 1000)).toFixed(0)} records/sec`);

    console.log('\nPer-worker results:');
    results.forEach((result) => {
        console.log(
            `  Worker ${result.workerId}: ${result.recordsIngested} records in ${result.timeMs}ms`
        );
    });
}

main().catch((error) => {
    console.error('Fatal error:', error);
    process.exit(1);
});
