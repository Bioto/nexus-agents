#!/bin/bash
# Query recent click-context analyses from ClickHouse
#
# Usage:
#   ./scripts/query_click_analysis.sh [limit]
#
# Environment variables:
#   CLICKHOUSE_HOST     - ClickHouse host (default: localhost)
#   CLICKHOUSE_PORT     - ClickHouse port (default: 9000)
#   CLICKHOUSE_USER     - ClickHouse user (default: default)
#   CLICKHOUSE_PASSWORD - ClickHouse password (default: default)
#   CLICKHOUSE_DATABASE - ClickHouse database (default: default)

set -euo pipefail

# Read environment or use defaults
CLICKHOUSE_HOST="${CLICKHOUSE_HOST:-localhost}"
CLICKHOUSE_PORT="${CLICKHOUSE_PORT:-9000}"
CLICKHOUSE_USER="${CLICKHOUSE_USER:-default}"
CLICKHOUSE_PASSWORD="${CLICKHOUSE_PASSWORD:-default}"
CLICKHOUSE_DATABASE="${CLICKHOUSE_DATABASE:-default}"

# Number of records to fetch
LIMIT="${1:-10}"

echo "📊 Fetching last ${LIMIT} click-context analyses from ClickHouse..."
echo ""

# Execute query using clickhouse-client
clickhouse-client \
    --host="${CLICKHOUSE_HOST}" \
    --port="${CLICKHOUSE_PORT}" \
    --user="${CLICKHOUSE_USER}" \
    --password="${CLICKHOUSE_PASSWORD}" \
    --database="${CLICKHOUSE_DATABASE}" \
    --format=PrettyCompact \
    --query="
SELECT
    timestamp,
    session_id,
    button,
    x,
    y,
    JSONExtractString(metadata, 'summary') AS summary,
    JSONExtractString(metadata, 'click', 'timestamp') AS click_timestamp,
    metadata
FROM events
WHERE event_type = 'analysis'
  AND event_subtype = 'click_context'
ORDER BY timestamp DESC
LIMIT ${LIMIT}
FORMAT Vertical
"

echo ""
echo "✅ Query complete"

