# swb

A UNIX-philosophy CLI for SwitchBot devices. Reads sensors over **BLE
passive scanning** (no hub, no cloud, no rate limits) and controls devices
through the **SwitchBot Cloud API v1.1** — nothing else. Aggregation,
storage, and dashboards are downstream jobs for tools like `jq`, Vector,
or SQL.

- stdout is pure structured JSON (JSONL in scan mode) — pipe it anywhere
- diagnostics and errors go to stderr as structured logs
- unknown devices are emitted losslessly instead of breaking the stream
- headless-friendly: plain token+secret auth, works in cron and Docker

## Install

```sh
cargo install --path .
```

Linux needs BlueZ and `libdbus-1-dev`/`pkg-config` at build time (or build
with `--features vendored-dbus`). BLE scanning may require
`setcap 'cap_net_raw,cap_net_admin+eip'` on the binary or running as a
user with Bluetooth privileges.

## Quick start — BLE (primary)

```sh
# one collection pass, one line per device
swb scan --once --timeout 15

# continuous stream
swb scan --follow
```

Example line (Meter Pro CO2):

```json
{"ts":"2026-07-14T06:00:00.123Z","addr":"XX:XX:XX:XX:XX:XX","rssi":-61,"mfr_hex":"6909...","svc_hex":"350064","model":"MeterProCO2","values":{"battery":88,"co2":712,"humidity":52,"temperature":24.5}}
```

Schema:

| field | presence | meaning |
|---|---|---|
| `ts` | always | receive time, RFC 3339 UTC, millisecond precision |
| `addr` | always | device MAC (platform-specific ID on macOS) |
| `rssi` | when known | signal strength, dBm |
| `mfr_hex` | always | raw manufacturer data, hex, incl. little-endian company ID (`6909`) — the lossless layer |
| `svc_hex` | when present | raw `0xfd3d` service data, hex |
| `model` / `values` | when decoded | added only when the decode dictionary matches; never guessed, never null |

`swb models` lists the models with decode support. Unknown models still
produce raw lines.

Pipe to [Vector](https://vector.dev):

```toml
[sources.co2]
type = "exec"
mode = "streaming"
command = ["swb", "scan", "--follow"]

[transforms.parse]
type = "remap"
inputs = ["co2"]
source = ". = parse_json!(.message)"
```

## Quick start — Cloud API (secondary)

Get a token and secret from the SwitchBot app (Profile → Preferences →
Developer Options), then:

```sh
export SWITCHBOT_TOKEN=... SWITCHBOT_SECRET=...

swb devices                     # list devices
swb status DEVICE_ID            # fetch status
swb cmd DEVICE_ID turnOn        # send a command
swb cmd DEVICE_ID setMode --param '{"mode":"auto"}'
```

`DEVICE_ID` can be an alias from the config file — see
[`config.example.toml`](config.example.toml).

The cloud backend is for actuation and out-of-range devices; sensor
polling belongs on the BLE side. Rate limit: 10,000 calls/day per account;
swb never polls on its own.

## Exit codes

| code | meaning |
|---|---|
| 0 | success |
| 2 | usage error |
| 3 | timeout (nothing received / request timed out) |
| 4 | BLE adapter missing or insufficient permissions |
| 5 | auth error (missing/invalid credentials) |
| 6 | device rejected / API returned non-success statusCode |
| 1 | other |

Errors are one JSON line on stderr: `{"error":{"kind":"...","detail":"..."}}`.

## Stability

The output schema is documented and kept stable — LLM function calling and
downstream SQL depend on it. Breaking changes bump the minor version even
pre-1.0.

## License

MIT
