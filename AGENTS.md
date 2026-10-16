# AGENTS.md

Guidance for AI agents (Claude Code, Codex, etc.) when working in this repository.

## Project Overview

`swb` is a UNIX-philosophy CLI for SwitchBot devices, written in Rust. Fully self-contained — it deliberately does not depend on the official `@switchbot/openapi-cli` (Node.js, OAuth/keychain-based) to avoid vendor lock-in and to stay headless-friendly (cron, Docker). Two backends:

1. **BLE (primary)** — passive scanning of SwitchBot advertisement broadcasts via `btleplug`. No hub, no cloud, no rate limits. Intended for continuous sensor collection (e.g. CO2 / temperature / humidity) piped into log collectors such as Vector.
2. **Cloud API v1.1 (secondary)** — SwitchBot's Web API (`api.switch-bot.com/v1.1`) for device control and devices out of BLE range. Simple token+secret auth, no OAuth flow, works headless.

Sibling CLIs: `enl` (ECHONET Lite) and `casa` (cross-protocol client). swb follows the same output conventions as enl.

This is a public OSS repository. Code and commit messages are in English. Never include private infrastructure details (hostnames, real device IDs, network layout) in code, docs, or fixtures — use placeholders (`XX:XX:...`, `192.0.2.x`, `DEVICE_ID`).

## Design Principles (shared with enl)

1. **Do one thing**: read/control SwitchBot devices. Aggregation, storage, and visualization are downstream jobs.
2. **stdout is pure structured JSON**: scan mode emits JSONL (one line = one advertisement event); one-shot commands emit one JSON document. No decoration, progress, or logs mixed in. `jq` and LLM function calling must consume it as-is.
3. **Diagnostics go to stderr as structured logs**: via `tracing`. Machine-readable errors as `{"error": {"kind": "...", "detail": "..."}}`.
4. **Never break on unknown devices or formats**: for an unrecognized advertisement or API payload, emit the raw data losslessly and keep going. Decode dictionaries are optional enrichment, never a requirement.
5. **BLE-first for sensing**: the cloud backend is for actuation and remote access. Do not add polling loops to the cloud backend; sensors are read via BLE.

## CLI Surface

```
swb scan --follow            # continuous BLE passive scan → JSONL stream
swb scan --once [--timeout]  # single collection pass, then exit
swb models                   # list models with BLE decode support (static info)
swb devices                  # cloud: list devices → JSON
swb status <device>          # cloud: fetch device status → JSON
swb cmd <device> <command>   # cloud: send control command → JSON
```

`<device>` accepts a raw device ID or an alias defined in config.

## Output Schema

Like enl, emit the result itself — no shared envelope wrapper. In scan streams the receive time is part of the data, so `ts` is always included there.

BLE scan line:

```json
{"ts":"2026-07-14T06:00:00.123Z","addr":"XX:XX:XX:XX:XX:XX","rssi":-61,"mfr_hex":"6909...","model":"MeterProCO2","values":{"temperature":24.5,"humidity":52,"co2":712,"battery":88}}
```

- `ts` is the receive time, RFC 3339 UTC with millisecond precision.
- `mfr_hex` (raw manufacturer data hex) is **always** emitted. This is the lossless layer.
- `model` / `values` are added only when the decode dictionary matches. Unknown models still produce a line (`mfr_hex` only). Never guess undecoded values; never emit nulls.

Cloud commands: emit the relevant `body` content of the API response as-is (plus `device_id`); do not re-wrap or rename fields the API already defines. If the response cannot be parsed as expected, emit the raw response body under `raw` instead of failing.

Document the schema and keep it stable (LLMs and downstream SQL depend on it). Breaking changes bump minor even pre-1.0.

## Exit Codes (so cron / periodic runners can branch)

| code | meaning |
|---|---|
| 0 | success |
| 2 | usage error (bad arguments) |
| 3 | timeout (`--once` received nothing / API request timed out) |
| 4 | BLE adapter missing / insufficient permissions |
| 5 | auth error (missing/invalid credentials, HTTP 401) |
| 6 | device rejected / API returned non-success statusCode |
| 1 | other |

## Architecture

```
src/
  main.rs        # clap parsing and dispatch only
  config.rs      # config file + env resolution (aliases, credentials)
  emit.rs        # output line types + serialization
  ble/
    mod.rs       # scan loop via btleplug
    parse/
      mod.rs       # model detection / dispatch on manufacturer & service data
      meter_co2.rs # one model = one file
  cloud/
    mod.rs       # HTTP client (one-shot request/response)
    auth.rs      # v1.1 HMAC-SHA256 signing (token + t + nonce)
    types.rs     # API request/response types
```

- `ble` and `cloud` must not depend on each other; both depend on `emit`.
- BLE parsers are pure functions `fn parse(data: &[u8]) -> Option<Values>` — no I/O, unit-testable with byte fixtures.
- Filter on manufacturer company ID `0x0969` (Woan Technology) before dispatching.
- **Adding a BLE parser requires committing a byte fixture captured from real hardware.** Never write a parser from documentation alone.
- The de facto BLE format reference is `pySwitchbot`'s adv parsers. The official `OpenWonderLabs/SwitchBotAPI-BLE` repo tends to lag for newer models.

## Cloud API Notes

- v1.1 auth headers: `Authorization` (token), `sign` (Base64 HMAC-SHA256 of `token + t + nonce`, keyed by secret), `t` (epoch millis), `nonce` (UUID v4).
- Rate limit: 10,000 calls/day per account. swb itself never polls; scheduling is the caller's responsibility.
- Credentials come from env (`SWITCHBOT_TOKEN`, `SWITCHBOT_SECRET`) or the config file. Never log them, never include them in error messages, never write them to fixtures.
- The API does not cover every device capability (some fields are known to be missing or wrong, e.g. battery on some models). Pass through what the API returns; do not compensate or fabricate.

## Configuration

- Config file: `$XDG_CONFIG_HOME/swb/config.toml` (fallback `~/.config/swb/config.toml`).
- Contents: device aliases, default scan timeout, optional credentials.
- Precedence: CLI flags > env vars > config file.
- Ship a documented `config.example.toml`; real config is never committed.

## Code Conventions

- Rust stable, edition 2021. `cargo fmt` and `cargo clippy -- -D warnings` must pass.
- Minimal dependencies: `clap`, `serde`, `serde_json`, `tracing`, `tracing-subscriber`, `btleplug`, a minimal HTTP client (prefer `ureq` for one-shot cloud calls over pulling full `reqwest`+`tokio` into the cloud path; `tokio` only as required by btleplug), `hmac`/`sha2`/`base64` for signing.
- Keep async at the I/O boundary (BLE scan loop); parsers and cloud calls stay sync.
- No panics in library code. `unwrap`/`expect` only in tests.
- Release profile matches enl (`lto`, `strip`, `opt-level = "z"`).

## Testing

- `cargo test` must pass without hardware, network, or credentials.
- BLE parsers: unit tests against real-hardware captures in `tests/fixtures/ble/`.
- Cloud: test the signing logic against known vectors; mock HTTP for client tests — never hit the real API in tests.
- Real-device verification is manual; document the steps in `docs/testing.md` (do not automate).

## OSS Hygiene

- License: MIT (same as enl).
- README covers: what it does, install, quick start for both backends, output schema, a pipe-to-Vector example.
- Conventional Commits. CI (GitHub Actions): fmt / clippy / test on Linux. BLE code must compile on macOS.
- No telemetry. No network traffic other than BLE reception and the SwitchBot API endpoint explicitly invoked by the user.

## Non-Goals

- Storage, dashboards, aggregation (downstream tools)
- BLE connect/write control (read-only scanning; revisit later if needed)
- Wrapping every cloud API endpoint — add commands only when actually needed
- Webhooks / long-running server processes (a stateless CLI; anything resident belongs elsewhere)
