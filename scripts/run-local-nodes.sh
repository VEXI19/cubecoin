#!/usr/bin/env bash
# Start a temporary, localhost-only CubeCoin P2P cluster.
set -euo pipefail

readonly DEFAULT_NETWORK_ID="1111111111111111111111111111111111111111111111111111111111111111"
readonly STARTUP_TIMEOUT_SECONDS=15
readonly SHUTDOWN_TIMEOUT_SECONDS=10

profile="${CUBECOIN_PROFILE:-debug}"
network_id="${CUBECOIN_NETWORK_ID:-$DEFAULT_NETWORK_ID}"
data_root="${CUBECOIN_DATA_ROOT:-}"
declare -a pids=()
declare -a node_ids=()
declare -a addresses=()
declare -a tail_pids=()
cleaned_up=false
demo=false
trace_wire=false

usage() {
    cat <<'EOF'
Usage: scripts/run-local-nodes.sh [--demo] [--trace-wire] [NODE_COUNT] [BASE_PORT]

Starts NODE_COUNT localhost nodes, including one bootstrap node. Defaults to
three nodes beginning at UDP port 5000.

Options:
  --demo        Stream readable, node-prefixed P2P discovery logs.
  --trace-wire  Include protobuf control-frame metadata (implies --demo).
  --help        Show this help text.

Optional environment variables:
  CUBECOIN_PROFILE=debug|release
  CUBECOIN_NETWORK_ID=<64 hexadecimal characters>
  CUBECOIN_DATA_ROOT=<absolute directory to reuse identities and logs>
  CUBECOIN_RUST_LOG=<tracing filter overriding the selected default>
EOF
}

fail() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

declare -a positional=()
for argument in "$@"; do
    case "$argument" in
        --demo) demo=true ;;
        --trace-wire) demo=true; trace_wire=true ;;
        --help)
            usage
            exit 0
            ;;
        --*) fail "unknown option: $argument" ;;
        *) positional+=("$argument") ;;
    esac
done
[[ "${#positional[@]}" -le 2 ]] || fail "expected at most NODE_COUNT and BASE_PORT"
count="${positional[0]:-3}"
base_port="${positional[1]:-5000}"

[[ "$count" =~ ^[0-9]+$ ]] || fail "NODE_COUNT must be a positive integer"
[[ "$count" -ge 1 && "$count" -le 24 ]] || fail "NODE_COUNT must be between 1 and 24"
[[ "$base_port" =~ ^[0-9]+$ ]] || fail "BASE_PORT must be an integer"
[[ "$base_port" -ge 1 && "$base_port" -le 65535 ]] || fail "BASE_PORT must be between 1 and 65535"
[[ $((base_port + count - 1)) -le 65535 ]] || fail "port range exceeds 65535"
[[ "$network_id" =~ ^[0-9a-fA-F]{64}$ ]] || fail "CUBECOIN_NETWORK_ID must contain 64 hexadecimal characters"
[[ "$profile" == "debug" || "$profile" == "release" ]] || fail "CUBECOIN_PROFILE must be debug or release"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"
umask 077

if [[ -z "$data_root" ]]; then
    data_root="$(mktemp -d /tmp/cubecoin-nodes.XXXXXX)"
else
    [[ "$data_root" = /* ]] || fail "CUBECOIN_DATA_ROOT must be an absolute path"
    mkdir -p "$data_root"
fi
data_root="$(cd "$data_root" && pwd)"

cleanup() {
    local pid deadline
    [[ "$cleaned_up" == true ]] && return
    cleaned_up=true

    for pid in "${tail_pids[@]:-}"; do
        kill "$pid" 2>/dev/null || true
    done
    for pid in "${pids[@]:-}"; do
        kill -TERM "$pid" 2>/dev/null || true
    done

    deadline=$((SECONDS + SHUTDOWN_TIMEOUT_SECONDS))
    while (( SECONDS < deadline )); do
        local running=false
        for pid in "${pids[@]:-}"; do
            if kill -0 "$pid" 2>/dev/null; then running=true; fi
        done
        [[ "$running" == false ]] && break
        sleep 0.1
    done

    for pid in "${pids[@]:-}"; do
        kill -KILL "$pid" 2>/dev/null || true
        wait "$pid" 2>/dev/null || true
    done
    for pid in "${tail_pids[@]:-}"; do
        wait "$pid" 2>/dev/null || true
    done

    if [[ -d "$data_root" ]]; then
        printf '\nSaved node logs:\n' >&2
        for log in "$data_root"/node-*.log; do
            [[ -e "$log" ]] && printf '  %s\n' "$log" >&2
        done
    fi
}

trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

printf 'Building cubecoin-node (%s)...\n' "$profile"
if [[ "$profile" == "release" ]]; then
    cargo build --release -p cubecoin-node
    binary="$repo_root/target/release/cubecoin-node"
else
    cargo build -p cubecoin-node
    binary="$repo_root/target/debug/cubecoin-node"
fi

wait_for_node_id() {
    local index="$1" log="$2" deadline line node_id
    deadline=$((SECONDS + STARTUP_TIMEOUT_SECONDS))
    while (( SECONDS < deadline )); do
        if ! kill -0 "${pids[index]}" 2>/dev/null; then
            tail -n 30 "$log" >&2 || true
            fail "node $index exited during startup"
        fi
        line="$(grep '"message":"node started"' "$log" 2>/dev/null | tail -n 1 || true)"
        node_id="$(printf '%s\n' "$line" | sed -nE 's/.*"node_id":"([0-9a-f]{64})".*/\1/p')"
        if [[ "$node_id" =~ ^[0-9a-f]{64}$ ]]; then
            printf '%s\n' "$node_id"
            return
        fi
        sleep 0.1
    done
    tail -n 30 "$log" >&2 || true
    fail "timed out waiting for node $index startup"
}

wait_for_bootstrap_authentication() {
    local index="$1" log="$2" bootstrap_id="$3" deadline
    deadline=$((SECONDS + STARTUP_TIMEOUT_SECONDS))
    while (( SECONDS < deadline )); do
        if ! kill -0 "${pids[index]}" 2>/dev/null; then
            tail -n 30 "$log" >&2 || true
            fail "node $index exited before bootstrap authentication"
        fi
        if grep -q 'P2P peer authenticated' "$log" 2>/dev/null && grep -q "\"peer\":\"$bootstrap_id\"" "$log" 2>/dev/null; then
            return
        fi
        sleep 0.1
    done
    tail -n 30 "$log" >&2 || true
    fail "node $index did not authenticate with bootstrap in time"
}

follow_log() {
    local index="$1" log="$2"
    if command -v jq >/dev/null 2>&1; then
        tail -n +1 -F "$log" | jq --unbuffered -r --arg label "node-$index" \
            '"\($label) \(.level) \(.fields.message // "") \((.fields | del(.message) | to_entries | map("\(.key)=\(.value | tostring)") | join(" ")))"' >&2 &
    else
        tail -n +1 -F "$log" | sed -u "s/^/node-$index /" >&2 &
    fi
    tail_pids+=("$!")
}

if [[ -n "${CUBECOIN_RUST_LOG:-}" ]]; then
    rust_log="$CUBECOIN_RUST_LOG"
elif [[ "$trace_wire" == true ]]; then
    rust_log="info,cubecoin_p2p=debug,cubecoin_p2p::wire=trace"
elif [[ "$demo" == true ]]; then
    rust_log="info,cubecoin_p2p=debug"
else
    rust_log="info"
fi

start_node() {
    local index="$1" bootstrap_id="${2:-}" port
    port=$((base_port + index))
    local address="127.0.0.1:$port" node_dir="$data_root/node-$index" log="$data_root/node-$index.log"
    local -a environment=(
        "NETWORK_ID=$network_id"
        "NODE_ADDR=$address"
        "NODE_DATA_DIR=$node_dir"
        "RUST_LOG=$rust_log"
        "LOG_FORMAT=json"
        "TOKIO_WORKER_THREADS=1"
    )
    if [[ -n "$bootstrap_id" ]]; then
        environment+=("BOOTSTRAP_ADDR=127.0.0.1:$base_port" "BOOTSTRAP_NODE_ID=$bootstrap_id")
    fi
    env -u BOOTSTRAP_ADDR -u BOOTSTRAP_NODE_ID "${environment[@]}" "$binary" >/dev/null 2>"$log" &
    pids[index]=$!
    addresses[index]="$address"
    node_ids[index]="$(wait_for_node_id "$index" "$log")"
    if [[ "$demo" == true ]]; then
        follow_log "$index" "$log"
    fi
}

start_node 0
bootstrap_id="${node_ids[0]}"
for ((index = 1; index < count; index++)); do
    start_node "$index" "$bootstrap_id"
    wait_for_bootstrap_authentication "$index" "$data_root/node-$index.log" "$bootstrap_id"
done

manifest="$data_root/nodes.tsv"
printf 'index\tpid\taddress\tnode_id\tdata_directory\tlog_file\n' >"$manifest"
printf '\nStarted %s local CubeCoin nodes.\n' "$count"
printf '%-6s %-8s %-22s %s\n' INDEX PID ADDRESS NODE_ID
for ((index = 0; index < count; index++)); do
    printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$index" "${pids[index]}" "${addresses[index]}" "${node_ids[index]}" "$data_root/node-$index" "$data_root/node-$index.log" >>"$manifest"
    printf '%-6s %-8s %-22s %s\n' "$index" "${pids[index]}" "${addresses[index]}" "${node_ids[index]}"
done
printf '\nRun data and logs: %s\nPress Ctrl-C to stop all nodes.\n' "$data_root"

while :; do
    for ((index = 0; index < count; index++)); do
        if ! kill -0 "${pids[index]}" 2>/dev/null; then
            tail -n 30 "$data_root/node-$index.log" >&2 || true
            fail "node $index exited unexpectedly"
        fi
    done
    sleep 1
done
