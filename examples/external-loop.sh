#!/bin/bash
# Example: External loop orchestration with bash
#
# This demonstrates bash-orchestratable single-iteration mode where
# bash controls the loop and Hox provides single iteration execution.
#
# Usage:
#   ./examples/external-loop.sh <change-id>

set -euo pipefail

CHANGE_ID="${1:-}"
MAX_ITERATIONS=20
STATE_FILE="/tmp/hox-external-state.json"

if [ -z "$CHANGE_ID" ]; then
    echo "Usage: $0 <change-id>"
    echo ""
    echo "Example: $0 abc123"
    exit 1
fi

echo "Starting external loop for change: $CHANGE_ID"
echo "Max iterations: $MAX_ITERATIONS"
echo ""

# Clean up state file from previous runs
rm -f "$STATE_FILE"

# Iteration loop (controlled by bash, not Hox)
for iteration in $(seq 1 $MAX_ITERATIONS); do
    echo "=== Iteration $iteration of $MAX_ITERATIONS ==="

    # Build command
    cmd="hox loop external --change-id $CHANGE_ID --output-state $STATE_FILE"

    # Add state file if this is not the first iteration
    if [ -f "$STATE_FILE" ]; then
        cmd="$cmd --state-file $STATE_FILE"
    fi

    # Run single iteration and capture JSON output
    result_json=$(eval $cmd)

    # Parse results using jq
    success=$(echo "$result_json" | jq -r '.success')
    stop_signal=$(echo "$result_json" | jq -r '.stop_signal // "none"')
    files_created=$(echo "$result_json" | jq -r '.files_created | length')
    files_modified=$(echo "$result_json" | jq -r '.files_modified | length')

    echo "  Success: $success"
    echo "  Files created: $files_created"
    echo "  Files modified: $files_modified"
    echo "  Stop signal: $stop_signal"
    echo ""

    # Check for stop conditions
    if [ "$stop_signal" != "none" ] && [ "$stop_signal" != "null" ]; then
        echo "Stop signal detected: $stop_signal"
        break
    fi

    if [ "$success" = "true" ]; then
        echo "All checks passed! Loop complete."
        break
    fi

    # Optional: Add custom logic here
    # - Check external metrics
    # - Send notifications
    # - Update dashboards
    # - etc.
done

echo ""
echo "External loop completed after $iteration iteration(s)"

# Show final state
if [ -f "$STATE_FILE" ]; then
    echo ""
    echo "Final state:"
    cat "$STATE_FILE" | jq '.'
fi
