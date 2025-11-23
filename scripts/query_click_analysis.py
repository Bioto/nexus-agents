#!/usr/bin/env python3
"""
Query recent click-context analyses from ClickHouse with pretty formatting.

Usage:
    python scripts/query_click_analysis.py [limit]

Environment variables:
    CLICKHOUSE_HOST     - ClickHouse host (default: localhost)
    CLICKHOUSE_PORT     - ClickHouse HTTP port (default: 8123)
    CLICKHOUSE_USER     - ClickHouse user (default: default)
    CLICKHOUSE_PASSWORD - ClickHouse password (default: default)
    CLICKHOUSE_DATABASE - ClickHouse database (default: default)
"""

import os
import sys
import json
import requests
from datetime import datetime
from typing import List, Dict, Any


def get_click_analyses(limit: int = 10) -> List[Dict[str, Any]]:
    """Fetch recent click-context analyses from ClickHouse."""
    host = os.getenv("CLICKHOUSE_HOST", "localhost")
    port = os.getenv("CLICKHOUSE_PORT", "8123")
    user = os.getenv("CLICKHOUSE_USER", "default")
    password = os.getenv("CLICKHOUSE_PASSWORD", "default")
    database = os.getenv("CLICKHOUSE_DATABASE", "default")

    url = f"http://{host}:{port}"
    
    query = f"""
    SELECT
        timestamp,
        session_id,
        button,
        x,
        y,
        metadata
    FROM events
    WHERE event_type = 'analysis'
      AND event_subtype = 'click_context'
    ORDER BY timestamp DESC
    LIMIT {limit}
    FORMAT JSONEachRow
    """

    response = requests.post(
        url,
        params={"database": database},
        auth=(user, password),
        data=query,
    )
    response.raise_for_status()

    results = []
    for line in response.text.strip().split("\n"):
        if line:
            results.append(json.loads(line))
    
    return results


def print_analysis(record: Dict[str, Any], index: int):
    """Pretty-print a single click analysis record."""
    metadata = json.loads(record["metadata"]) if isinstance(record["metadata"], str) else record["metadata"]
    
    print(f"\n{'='*80}")
    print(f"📌 Click Analysis #{index + 1}")
    print(f"{'='*80}")
    print(f"Session:   {record['session_id']}")
    print(f"Timestamp: {record['timestamp']}")
    print(f"Button:    {record.get('button', 'unknown')}")
    print(f"Position:  ({record.get('x', '?')}, {record.get('y', '?')})")
    print()
    
    # Print summary
    summary = metadata.get("summary", "No summary available")
    print(f"📝 Summary:")
    print(f"   {summary}")
    print()
    
    # Print frame analyses
    frames = metadata.get("frames", [])
    if frames:
        print(f"🎞️  Frame Analyses ({len(frames)} frames):")
        for frame in frames:
            offset = frame.get("offset_secs", "?")
            desc = frame.get("description", "No description")
            file_path = frame.get("file_path")
            
            if file_path:
                print(f"   • +{offset}s: {desc}")
                print(f"     📁 {file_path}")
            else:
                print(f"   • +{offset}s: {desc}")
    else:
        print("   (No frame data)")


def main():
    limit = int(sys.argv[1]) if len(sys.argv) > 1 else 10
    
    print(f"📊 Fetching last {limit} click-context analyses from ClickHouse...")
    
    try:
        results = get_click_analyses(limit)
        
        if not results:
            print("\n❌ No click-context analyses found in the database.")
            print("   Make sure you've run the logger with click events captured.")
            return
        
        print(f"\n✅ Found {len(results)} click analysis records\n")
        
        for idx, record in enumerate(results):
            print_analysis(record, idx)
        
        print(f"\n{'='*80}")
        print(f"Total: {len(results)} click analyses")
        print(f"{'='*80}\n")
        
    except requests.exceptions.RequestException as e:
        print(f"\n❌ Failed to connect to ClickHouse: {e}", file=sys.stderr)
        print(f"   Make sure ClickHouse is running at {os.getenv('CLICKHOUSE_HOST', 'localhost')}:{os.getenv('CLICKHOUSE_PORT', '8123')}", file=sys.stderr)
        sys.exit(1)
    except Exception as e:
        print(f"\n❌ Error: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()







