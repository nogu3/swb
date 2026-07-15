# swb CLI — Initial Implementation Design

Date: 2026-07-15
Status: approved for implementation

CLAUDE.md is the authoritative spec (CLI surface, output schema, exit codes,
architecture, conventions). This document only pins down the decisions
CLAUDE.md leaves open, so the two documents together are the full design.

## Scope of the initial implementation

All six subcommands from CLAUDE.md:

- `swb scan --follow` / `swb scan --once [--timeout <secs>]` — BLE passive scan → JSONL
- `swb models` — static list of models with BLE decode support → JSON
- `swb devices` — cloud device list → JSON
- `swb status <device>` — cloud device status → JSON
- `swb cmd <device> <command> [--param <json>]` — cloud control command → JSON

Initial BLE decode coverage: **MeterProCO2 only** (the stated primary use
case). All other SwitchBot advertisements are still emitted losslessly
(`mfr_hex` / `svc_hex` without `model`/`values`).

## Decisions pinned here

### D1. BLE model detection and service data

SwitchBot advertisements carry two payloads (format reference: pySwitchbot
`adv_parser.py`):

- Manufacturer data, company ID `0x0969` (2409, Woan Technology). Payload
  excludes the 2-byte company ID.
- Service data under UUID `0000fd3d-0000-1000-8000-00805f9b34fb`.
  Byte 0 & `0x7f` is the model tag (`'5'` = Meter Pro CO2); byte 0 & `0x80`
  is the "encrypted" flag.

Model detection uses the service-data model tag only. If service data is
absent we do **not** guess from manufacturer data length; the line is emitted
raw-only. This follows "never guess undecoded values".

Consequence for the output schema: battery lives in service data, so the
lossless layer needs service data too. In addition to `mfr_hex` (always
emitted when manufacturer data is present), scan lines include `svc_hex`
(hex of the `fd3d` service data) whenever present. A line is emitted if at
least one of the two payloads is present. This is an additive extension of
the CLAUDE.md schema example.

### D2. MeterProCO2 decode (format reference: pySwitchbot)

From manufacturer payload `m` (`len(m) >= 11` for temperature/humidity,
`>= 15` for CO2) and service payload `s` (`len(s) >= 3` for battery):

- temperature (°C): sign = bit7 of `m[9]` (1 = positive), integer = `m[9] & 0x7f`,
  fraction = `(m[8] & 0x0f) / 10`
- humidity (%): `m[10] & 0x7f`
- co2 (ppm): big-endian u16 at `m[13..15]`, dropped if > 9999 (sensor spec
  range; larger values are transient artifacts)
- battery (%): `s[2] & 0x7f`

Fields that cannot be decoded (e.g. battery when service data is short) are
omitted — never null. `values` keys: `temperature`, `humidity`, `co2`,
`battery` (matching the CLAUDE.md example; the Fahrenheit display flag is
not emitted — YAGNI).

### D3. Fixture provenance

CLAUDE.md requires byte fixtures captured from real hardware. We do not have
a capture rig in this environment, so the initial fixtures are real-hardware
captures taken from the pySwitchbot test suite (MIT licensed), e.g.:

- mfr payload: `b0e9fe5432 15 b7e4 079ba400 37 02d5 00`
  → 27.7 °C, 36 %, 725 ppm
- svc payload: `350064` → model '5', battery 100

Each fixture file records its provenance (pySwitchbot commit URL). Replacing
or augmenting these with captures from the user's own devices is a follow-up.
The MAC embedded in fixture bytes is pySwitchbot's placeholder
(`b0:e9:fe:54:32:15`), not a private device ID.

### D4. Dependencies beyond the CLAUDE.md list

- `toml` — config file parsing (required by the config spec).
- `uuid` (v4 feature) — cloud nonce.
- `time` (formatting + well-known RFC 3339) — `ts` timestamps with
  millisecond precision.
- Cargo feature `vendored-dbus` (off by default): forwards to
  `dbus/vendored` so Linux builds work without system `libdbus-1-dev` /
  `pkg-config` (btleplug → bluez → libdbus). Development on this machine
  uses `--features vendored-dbus`; release builds on normal distros use the
  system library.

`ureq` (with `json` off — we serialize via serde_json ourselves), `hmac`,
`sha2`, `base64`, `clap` (derive), `serde`, `serde_json`, `tracing`,
`tracing-subscriber`, `btleplug`, `tokio` (rt only, BLE path only) as per
CLAUDE.md.

### D5. Async boundary

`main` is sync. The `scan` command builds a current-thread tokio runtime
inside `ble::mod` and blocks on the scan loop. Cloud commands are plain sync
`ureq` calls. Parsers are pure sync functions.

### D6. Error output and exit codes

Errors are emitted to stderr as one JSON line
`{"error":{"kind":"<kind>","detail":"<message>"}}` and mapped to the
CLAUDE.md exit-code table. `kind` values: `usage` (2), `timeout` (3),
`adapter` (4), `auth` (5), `device` (6), `other` (1). Credentials never
appear in `detail`.

- HTTP 401 → auth (5); missing credentials → auth (5)
- API `statusCode != 100` → device (6)
- `--once` with zero received advertisements within timeout → timeout (3)
- No BLE adapter / permission denied opening it → adapter (4)

### D7. `swb models` output

One JSON document: an array of
`{"model":"MeterProCO2","tag":"5","values":["temperature","humidity","co2","battery"]}`.
Static, no I/O.

### D8. Cloud command surface

- `devices` → GET `/v1.1/devices`, emit the `body` object as-is.
- `status <device>` → GET `/v1.1/devices/{id}/status`, emit `body` plus
  `device_id`.
- `cmd <device> <command> [--param <json>]` → POST
  `/v1.1/devices/{id}/commands` with
  `{"command": <command>, "parameter": <param or "default">, "commandType": "command"}`,
  emit `body` plus `device_id`.
- Unparseable response body → emit `{"raw": "<body text>", ...}` instead of
  failing.
- `<device>` resolves through config aliases first, then is used verbatim.

### D9. Config

`$XDG_CONFIG_HOME/swb/config.toml` (fallback `~/.config/swb/config.toml`):

```toml
default_timeout = 30        # seconds, for scan --once

[credentials]
token = "..."               # optional; env SWITCHBOT_TOKEN wins
secret = "..."              # optional; env SWITCHBOT_SECRET wins

[aliases]
living_co2 = "DEVICEID123"
```

Precedence: CLI flags > env > config. A missing config file is not an error.
Ship `config.example.toml` with placeholders.

## Architecture, testing, CI

As specified in CLAUDE.md (`src/main.rs`, `config.rs`, `emit.rs`, `ble/`,
`cloud/`; `ble`⊥`cloud`, both depend on `emit`). Tests: parser unit tests
against `tests/fixtures/ble/*.txt` hex fixtures; HMAC signing against a
known vector; cloud client parsing against canned JSON (no live HTTP).
CI: GitHub Actions on Linux running fmt / clippy / test (BLE code must also
compile on macOS — enforced by keeping platform-specific code inside
btleplug's abstractions, verified in review, not in CI initially).
README rewritten per the OSS hygiene checklist.

## Non-goals (this iteration)

- Decode support beyond MeterProCO2
- BLE connect/write, webhooks, polling loops, storage
- macOS CI runner
