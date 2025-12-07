#!/usr/bin/env python3
"""
Query recent analysis events from ClickHouse with pretty formatting.

Usage:
    python scripts/query_click_analysis.py [limit] [--type TYPE] [--limit N] [--show-metadata]

Arguments:
    limit           Number of records to fetch (positional, default: 10)
    --limit N       Number of records to fetch (flag, overrides positional)
    --type          Analysis type: video_context, webcam_sentiment, or all (default: video_context)
    --show-metadata Print raw metadata JSON for each record

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
import argparse
import requests
from typing import List, Dict, Any


def get_analyses(limit: int = 10, analysis_type: str = "video_context") -> List[Dict[str, Any]]:
    """Fetch recent analysis events from ClickHouse."""
    host = os.getenv("CLICKHOUSE_HOST", "localhost")
    port = os.getenv("CLICKHOUSE_PORT", "8123")
    user = os.getenv("CLICKHOUSE_USER", "default")
    password = os.getenv("CLICKHOUSE_PASSWORD", "default")
    database = os.getenv("CLICKHOUSE_DATABASE", "default")

    url = f"http://{host}:{port}"
    
    if analysis_type == "all":
        subtype_filter = ""
    else:
        subtype_filter = f"AND event_subtype = '{analysis_type}'"
    
    query = f"""
    SELECT
        toString(event_type) as event_type,
        toString(event_subtype) as event_subtype,
        timestamp,
        session_id,
        button,
        x,
        y,
        timecode,
        metadata
    FROM events
    WHERE event_type = 'analysis'
      {subtype_filter}
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


def _fmt_value(val: Any) -> str:
    if isinstance(val, float):
        return f"{val:.3f}"
    if isinstance(val, (dict, list)):
        return json.dumps(val)
    return str(val)


def print_analysis(record: Dict[str, Any], index: int):
    """Pretty-print a single analysis record."""
    metadata = json.loads(record["metadata"]) if isinstance(record["metadata"], str) else record["metadata"]
    
    print(f"\n{'='*80}")
    print(f"📌 Analysis #{index + 1} ({record.get('event_subtype', 'unknown')})")
    print(f"{'='*80}")
    print(f"Session:   {record['session_id']}")
    print(f"Timestamp: {record['timestamp']}")
    
    if record.get('timecode'):
        print(f"Timecode:  {record['timecode']:.2f}s")
    
    if record.get('button'):
        print(f"Button:    {record['button']}")
    if record.get('x') is not None and record.get('y') is not None:
        print(f"Position:  ({record['x']}, {record['y']})")
    print()
    
    # Print summary
    summary = metadata.get("summary", "No summary available")
    print(f"📝 Summary:")
    print(f"   {summary}")
    print()
    
    # Print frame analyses if present
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
    
    # Print sentiment data if present (webcam_sentiment)
    sentiment_keys = [
        "sentiment",
        "attention",
        "arousal",
        "valence",
        "engagement",
        "confidence",
        "probability",
        "emotion",
        "emotions",
        "focus",
        "face_detected",
    ]
    present_sentiment = [(k, metadata[k]) for k in sentiment_keys if k in metadata]
    if present_sentiment:
        print(f"😊 Webcam sentiment details:")
        for key, val in present_sentiment:
            print(f"   {key}: {_fmt_value(val)}")


def main():
    parser = argparse.ArgumentParser(description="Query analysis events from ClickHouse")
    parser.add_argument("limit", nargs="?", type=int, default=10, help="Number of records to fetch (default: 10)")
    parser.add_argument("--limit", dest="limit_flag", type=int, help="Number of records to fetch (overrides positional)")
    parser.add_argument("--type", dest="analysis_type", default="video_context",
                        choices=["video_context", "webcam_sentiment", "all"],
                        help="Analysis type to query (default: video_context)")
    parser.add_argument("--show-metadata", action="store_true", help="Print raw metadata JSON for each record")
    
    args = parser.parse_args()
    limit = args.limit_flag if args.limit_flag is not None else args.limit
    
    type_label = "all analysis" if args.analysis_type == "all" else args.analysis_type
    print(f"📊 Fetching last {limit} {type_label} events from ClickHouse...")
    
    try:
        results = get_analyses(limit, args.analysis_type)
        
        if not results:
            print(f"\n⚠️  No {type_label} events found in the database.")
            return
        
        print(f"\n✅ Found {len(results)} analysis records\n")
        
        for idx, record in enumerate(results):
            print_analysis(record, idx)
            if args.show_metadata:
                raw = record["metadata"] if isinstance(record["metadata"], str) else json.dumps(record["metadata"])
                print("   Raw metadata:")
                print(f"   {raw}")
                print()
        
        print(f"\n{'='*80}")
        print(f"Total: {len(results)} analysis events")
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
