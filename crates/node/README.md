# Local P2P test

Run three node processes on one machine, each with a separate UDP port and data directory. The v1 transport accepts loopback endpoints (`127.0.0.1` or `::1`) only.

1. Start the bootstrap with `crates/node/.env` containing (the current binary loads that source-directory file):

```dotenv
NETWORK_ID=1111111111111111111111111111111111111111111111111111111111
NODE_ADDR=127.0.0.1:5000
NODE_DATA_DIR=/tmp/cubecoin/bootstrap
RUST_LOG=info
```

2. Run `cargo run -p cubecoin-node`. It logs its stable `node_id`.

3. For another node, use a different `NODE_ADDR` and `NODE_DATA_DIR`, then add:

```dotenv
BOOTSTRAP_ADDR=127.0.0.1:5000
BOOTSTRAP_NODE_ID=<node_id printed by bootstrap>
```

Start a third node in the same way. It obtains the second node through peer exchange and connects to it directly.

`BOOTSTRAP_ADDR` and `BOOTSTRAP_NODE_ID` must either both be set or both be omitted. The node creates missing data-directory parents and requires the final `NODE_DATA_DIR` to be owned by the current user with mode `0700`; `identity.pk8` is mode `0600`. Node data is not wallet data; never copy or reuse a wallet key.

Use Ctrl-C or SIGTERM to stop a node gracefully. Protocol Buffers use a vendored `protoc`, so no system protobuf installation is required.

## Start a local cluster

From the repository root, start a bootstrap plus two peer nodes:

```bash
./scripts/run-local-nodes.sh
```

The count includes the bootstrap node:

```bash
./scripts/run-local-nodes.sh 5          # bootstrap plus four peers
./scripts/run-local-nodes.sh 5 6000     # use UDP ports 6000 through 6004
```

The script creates a fresh temporary run directory containing per-node keys, logs, and `nodes.tsv`, then remains in the foreground. Press Ctrl-C to stop every child node; the run directory is retained for inspection.

To use a release binary or retain identities across runs:

```bash
CUBECOIN_PROFILE=release ./scripts/run-local-nodes.sh 10
CUBECOIN_DATA_ROOT=/tmp/my-cubecoin-cluster ./scripts/run-local-nodes.sh 3
```

## Live multi-node demo

Use `--demo` to stream node-prefixed P2P discovery events while retaining the complete JSON log for each node. `jq` makes the streamed JSON readable; without it, the script streams the raw JSON lines.

```bash
./scripts/run-local-nodes.sh --demo 3
```

The expected sequence demonstrates the solution's key steps:

1. `identity.created` or `identity.loaded` and `node.listening`
2. `bootstrap.configured`
3. `tls.peer_identity`, `hello.validated`, and `peer.authenticated` with `bootstrap=true`
4. `pex.request_sent`, `pex.request_received`, and `pex.response_received`
5. `peer.book_inserted` and `discovery.dial_scheduled`
6. A direct `peer.authenticated` connection with `bootstrap=false`

To also show protobuf control-frame metadata (message IDs, types, and lengths), use `--trace-wire`. It never prints private keys, certificate bytes, or ping/pong nonces:

```bash
./scripts/run-local-nodes.sh --trace-wire 3
```

Set `CUBECOIN_RUST_LOG` to provide a custom tracing filter. For example:

```bash
CUBECOIN_RUST_LOG='info,cubecoin_p2p=debug,cubecoin_p2p::wire=trace' \
./scripts/run-local-nodes.sh --demo 3
```
