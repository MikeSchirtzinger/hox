# External Loop Mode

External loop mode enables bash-orchestratable single-iteration execution where each iteration is controlled by an external script rather than Hox's internal loop engine.

## Overview

**Standard Loop Mode (`hox loop start`):**
- Hox controls the iteration loop
- Runs until max iterations or completion
- All state managed internally

**External Loop Mode (`hox loop external`):**
- Bash/external script controls the loop
- Single iteration per invocation
- JSON state interchange for observability
- Enables custom orchestration logic

## Use Cases

1. **Custom Monitoring** - Integrate with external dashboards/metrics
2. **Distributed Execution** - Run iterations across different machines
3. **Advanced Control Flow** - Implement complex stopping conditions
4. **Testing/Debugging** - Step through iterations manually
5. **CI/CD Integration** - Orchestrate from build pipelines

## Command Line Interface

```bash
hox loop external [OPTIONS] --change-id <CHANGE_ID>

Options:
  --change-id <ID>           JJ change ID to work on (required)
  --state-file <PATH>        Load state from JSON (omit for first iteration)
  --output-state <PATH>      Write updated state to JSON
  --no-backpressure          Disable backpressure checks
  -m, --model <MODEL>        Model: opus, sonnet, haiku [default: sonnet]
  --max-tokens <N>           Max tokens [default: 16000]
  -n, --max-iterations <N>   Max iterations for display [default: 20]
```

## JSON Interchange Format

### Input: ExternalLoopState

```json
{
  "change_id": "abc123",
  "iteration": 1,
  "context": {
    "current_focus": "Implementing feature X",
    "progress": ["Created module", "Added tests"],
    "next_steps": ["Add error handling"],
    "blockers": [],
    "files_touched": ["src/lib.rs"],
    "decisions": [],
    "loop_iteration": 1,
    "backpressure_status": {
      "tests_passed": false,
      "lints_passed": true,
      "builds_passed": true,
      "last_errors": ["Test failure: missing test case"]
    }
  },
  "backpressure": {
    "tests_passed": false,
    "lints_passed": true,
    "builds_passed": true,
    "errors": ["Test failure: missing test case"]
  },
  "files_touched": ["src/lib.rs", "tests/test.rs"]
}
```

### Output: ExternalLoopResult

```json
{
  "iteration": 2,
  "success": true,
  "output": "Agent's raw output text...",
  "context": {
    "current_focus": "Adding error handling",
    "progress": ["Created module", "Added tests", "Fixed test failure"],
    "next_steps": ["Add documentation"],
    "blockers": [],
    "files_touched": ["src/lib.rs", "tests/test.rs"],
    "decisions": [],
    "loop_iteration": 2,
    "backpressure_status": {
      "tests_passed": true,
      "lints_passed": true,
      "builds_passed": true,
      "last_errors": []
    }
  },
  "files_created": [],
  "files_modified": ["tests/test.rs"],
  "usage": {
    "input_tokens": 5000,
    "output_tokens": 2000
  },
  "stop_signal": null
}
```

### Stop Signals

The `stop_signal` field indicates completion:

- `null` - Continue iterating
- `"legacy_stop"` - Agent used `[STOP]` or `[DONE]`
- `"promise_complete"` - Agent used `<promise>COMPLETE</promise>`

## Example: Basic Bash Loop

```bash
#!/bin/bash
CHANGE_ID="abc123"
STATE_FILE="/tmp/hox-state.json"
MAX_ITERATIONS=20

# Clean up previous state
rm -f "$STATE_FILE"

for i in $(seq 1 $MAX_ITERATIONS); do
    echo "=== Iteration $i ==="

    # Build command
    cmd="hox loop external --change-id $CHANGE_ID --output-state $STATE_FILE"
    [ -f "$STATE_FILE" ] && cmd="$cmd --state-file $STATE_FILE"

    # Run iteration
    result=$(eval $cmd)

    # Parse results
    success=$(echo "$result" | jq -r '.success')
    stop_signal=$(echo "$result" | jq -r '.stop_signal // "none"')

    # Check stop conditions
    [ "$stop_signal" != "none" ] && echo "Stop signal: $stop_signal" && break
    [ "$success" = "true" ] && echo "All checks passed!" && break
done
```

## Example: Advanced Orchestration

```bash
#!/bin/bash
# Advanced external orchestration with monitoring

CHANGE_ID="$1"
STATE_FILE="/tmp/hox-state-${CHANGE_ID}.json"
METRICS_FILE="/tmp/metrics-${CHANGE_ID}.json"

run_iteration() {
    local iteration=$1

    # Run Hox iteration
    result=$(hox loop external \
        --change-id "$CHANGE_ID" \
        --state-file "$STATE_FILE" \
        --output-state "$STATE_FILE" 2>/dev/null)

    # Extract metrics
    local tokens_in=$(echo "$result" | jq -r '.usage.input_tokens')
    local tokens_out=$(echo "$result" | jq -r '.usage.output_tokens')
    local success=$(echo "$result" | jq -r '.success')

    # Update metrics dashboard
    jq -n \
        --arg iter "$iteration" \
        --arg tokens_in "$tokens_in" \
        --arg tokens_out "$tokens_out" \
        --arg success "$success" \
        '{iteration: $iter, tokens_in: $tokens_in, tokens_out: $tokens_out, success: $success}' \
        >> "$METRICS_FILE"

    # Send to monitoring system
    curl -X POST "http://monitoring-dashboard/metrics" \
        -H "Content-Type: application/json" \
        -d "$result" 2>/dev/null || true

    echo "$result"
}

# Main loop
for i in $(seq 1 50); do
    result=$(run_iteration $i)

    # Custom stopping logic
    stop_signal=$(echo "$result" | jq -r '.stop_signal // "none"')
    success=$(echo "$result" | jq -r '.success')

    # Check external health metrics
    if check_system_health; then
        [ "$success" = "true" ] && break
    else
        echo "System unhealthy, pausing..."
        sleep 60
    fi

    [ "$stop_signal" != "none" ] && break
done
```

## Example: Distributed Execution

```bash
#!/bin/bash
# Run iterations on different machines via SSH

CHANGE_ID="$1"
HOSTS=("worker1" "worker2" "worker3")

# Initialize on main machine
hox loop external --change-id "$CHANGE_ID" --output-state state.json

# Distribute iterations
for i in $(seq 1 20); do
    host="${HOSTS[$((i % ${#HOSTS[@]}))]}"

    echo "Running iteration $i on $host"

    # Copy state to worker
    scp state.json "$host:/tmp/hox-state.json"

    # Run iteration remotely
    ssh "$host" "cd /path/to/hox && \
        hox loop external \
            --change-id $CHANGE_ID \
            --state-file /tmp/hox-state.json \
            --output-state /tmp/hox-state-new.json"

    # Retrieve updated state
    scp "$host:/tmp/hox-state-new.json" state.json

    # Check completion
    success=$(jq -r '.success' < state.json)
    [ "$success" = "true" ] && break
done
```

## Comparison with Internal Loop

| Feature | Internal Loop | External Loop |
|---------|--------------|---------------|
| **Control** | Hox manages iterations | Bash/script manages |
| **State** | Internal to Hox | JSON files |
| **Monitoring** | Activity logs | Custom dashboards |
| **Distribution** | Single machine | Multi-machine capable |
| **Complexity** | Simple, built-in | Flexible, requires scripting |
| **Use Case** | Standard development | Advanced orchestration |

## Best Practices

1. **Always save state** - Use `--output-state` to preserve progress
2. **Check stop signals** - Respect agent completion signals
3. **Handle errors** - Wrap Hox calls in error handling
4. **Validate JSON** - Use `jq` to parse and validate output
5. **Clean up state** - Remove state files when done
6. **Monitor resources** - Track token usage and costs
7. **Set max iterations** - Prevent infinite loops

## Debugging

### View iteration output

```bash
result=$(hox loop external --change-id abc123)
echo "$result" | jq '.output'
```

### Inspect state file

```bash
cat state.json | jq '.context'
```

### Check backpressure errors

```bash
cat state.json | jq '.backpressure.errors'
```

### Validate JSON format

```bash
echo "$result" | jq . > /dev/null && echo "Valid JSON"
```

## See Also

- `examples/external-loop.sh` - Basic bash orchestration example
- `hox loop start` - Internal loop mode
- `hox loop status` - Check loop progress
