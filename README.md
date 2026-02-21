# moqgen

A load generator and relay probe for [MoQ (Media over QUIC)](https://datatracker.ietf.org/wg/moq/about/) — inspired by OpenTelemetry's [`telemetrygen`](https://github.com/open-telemetry/opentelemetry-collector-contrib/tree/main/cmd/telemetrygen).

Built on [`kixelated/moq-rs`](https://github.com/kixelated/moq-rs) (`moq-lite` + `moq-native`).

## Subcommands

| Command | What it does |
|---|---|
| `publish` | Connects as a publisher and generates synthetic tracks at a configurable rate |
| `subscribe` | Connects as a subscriber and measures received throughput; optionally validates sequence ordering |
| `probe` | Runs both roles simultaneously through the relay and measures round-trip latency |

## Installation

```bash
git clone https://github.com/ColinLeeCampbell/moqgen
cd moqgen
cargo build --release
# binary at: target/release/moqgen
```

## Usage

Start a local relay first (from [moq-rs](https://github.com/kixelated/moq-rs)):

```bash
moq-relay
```

### Publish

Generate 3 tracks at 30 groups/sec for 5 seconds:

```bash
moqgen publish \
  --relay https://localhost:4443 \
  --insecure \
  --broadcast moqgen/test \
  --tracks 3 \
  --rate 30 \
  --duration 5
```

### Subscribe

Subscribe to a broadcast and report throughput (in a separate terminal):

```bash
moqgen subscribe \
  --relay https://localhost:4443 \
  --insecure \
  --broadcast moqgen/test \
  --tracks 3 \
  --validate \
  --frame-size 1024
```

`--tracks N` subscribes to `track-0` through `track-N-1`, matching the publisher convention. `--validate` enables sequence gap detection and, when `--frame-size` is non-zero, verifies that every received frame is exactly that many bytes.

### Probe

Measure round-trip latency through the relay (publisher + subscriber in one command):

```bash
moqgen probe \
  --relay https://localhost:4443 \
  --insecure \
  --tracks 1 \
  --rate 10 \
  --duration 10
```

Each frame's first 8 bytes carry a microsecond timestamp; the subscriber computes one-way latency from that.

### JSON output

All subcommands support `--output json` for scripting:

```bash
moqgen probe --relay https://localhost:4443 --insecure --output json \
  | jq '.latency_p99_us'
```

## Options

### `publish`

| Flag | Default | Description |
|---|---|---|
| `--relay` | — | Relay URL (required) |
| `--broadcast` | `moqgen/test` | Broadcast path |
| `--tracks` | `1` | Concurrent tracks per connection |
| `--rate` | `30.0` | Groups per second per track |
| `--group-size` | `1` | Frames per group |
| `--frame-size` | `1024` | Frame size in bytes |
| `--duration` | `10` | Run duration in seconds |
| `--workers` | `1` | Concurrent QUIC connections |
| `--insecure` | false | Skip TLS verification |
| `--output` | `text` | `text` or `json` |
| `--metrics-interval` | `1` | Stats print interval in seconds |

### `subscribe`

| Flag | Default | Description |
|---|---|---|
| `--relay` | — | Relay URL (required) |
| `--broadcast` | `moqgen/test` | Broadcast path |
| `--tracks` | `1` | Number of tracks to subscribe to (track-0 .. track-N-1) |
| `--duration` | `10` | Run duration in seconds |
| `--validate` | false | Sequence + frame size validation |
| `--frame-size` | `0` | Expected frame size for validation (0 = skip) |
| `--insecure` | false | Skip TLS verification |
| `--output` | `text` | `text` or `json` |
| `--metrics-interval` | `1` | Stats print interval in seconds |

### `probe`

| Flag | Default | Description |
|---|---|---|
| `--relay` | — | Relay URL (required) |
| `--broadcast` | `moqgen/test` | Broadcast path |
| `--tracks` | `1` | Concurrent tracks |
| `--rate` | `10.0` | Groups per second per track |
| `--frame-size` | `64` | Frame size in bytes (min 8 for timestamp) |
| `--duration` | `10` | Run duration in seconds |
| `--insecure` | false | Skip TLS verification |
| `--output` | `text` | `text` or `json` |
| `--metrics-interval` | `1` | Stats print interval in seconds |

## Metrics

| Metric | Subcommands |
|---|---|
| `groups/s`, `frames/s`, `bytes/s` | publish, subscribe, probe |
| `groups_total`, `errors_total` | publish, subscribe, probe |
| `groups_dropped` (sequence gaps) | subscribe, probe |
| `latency_p50/p90/p99/min/max/mean` (µs) | probe |
