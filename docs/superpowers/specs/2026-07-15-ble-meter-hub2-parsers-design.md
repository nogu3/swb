# BLE parsers for Meter ('T') and Hub 2 ('v')

Date: 2026-07-15
Status: approved

## Goal

Extend the BLE decode dictionary with two models captured from real hardware
during the 2026-07-15 verification pass on a Raspberry Pi:

- **Meter** (original WoSensorTH, service-data tag `'T'` / 0x54) —
  temperature, humidity, battery.
- **Hub 2** (tag `'v'` / 0x76) — temperature, humidity, light_level.

Devices with tags 0x00 and 0x32 were also observed but are not identified
as owned hardware; they stay undecoded (raw `mfr_hex`/`svc_hex` lines),
per the "never break on unknown devices" principle.

## Background

All three supported models (existing MeterProCO2 plus the two new ones)
share the same 3-byte temperature/humidity codec, per pySwitchbot's adv
parsers (`adv_parsers/meter.py`, `adv_parsers/hub2.py`):

```
byte 0: bits 3..0 = temperature tenths
byte 1: bit 7 = sign (1 = positive), bits 6..0 = integer °C
byte 2: bit 7 = Fahrenheit-preference flag (ignored), bits 6..0 = humidity %
```

Both new formats were validated against today's real captures:

- Meter: mfr bytes 8..11 = `03 9b 3e` → 27.3 °C / 62 %; svc byte 2 =
  `0x64` → battery 100.
- Hub 2: mfr bytes 13..16 = `01 98 3b` → 24.1 °C / 59 %; mfr byte 12 &
  0x1f = 9 → light_level 9.

Byte offsets are into the manufacturer payload *without* the 2-byte
company ID (same convention as the existing `decode()` entry point; the
payload starts with the device MAC).

## Design

### Files

```
src/ble/parse/
  mod.rs        # dispatch 'T' → meter, 'v' → hub2; MODELS gains 2 entries
  sensor_th.rs  # NEW: shared codec fn temp_humidity(&[u8]) -> Option<(f64, u8)>
  meter.rs      # NEW: tag 'T'
  hub2.rs       # NEW: tag 'v'
  meter_co2.rs  # refactor: temperature/humidity via sensor_th (output unchanged)
```

Parsers stay pure functions `fn parse(mfr: Option<&[u8]>, svc: Option<&[u8]>)
-> Option<Values>` with no I/O, matching the existing architecture.

### Meter (tag 'T')

- temperature/humidity: mfr bytes 8..11 when `mfr.len() >= 11`; else fall
  back to svc bytes 3..6 when `svc.len() >= 6` (pySwitchbot precedence).
- battery: svc byte 2 & 0x7f when `svc.len() >= 3`.
- `models` entry: `{"model":"Meter","tag":"T","values":["temperature","humidity","battery"]}`.

### Hub 2 (tag 'v')

- temperature/humidity: mfr bytes 13..16 when `mfr.len() >= 16`.
- light_level: mfr byte 12 & 0x1f when `mfr.len() >= 13`, emitted as the
  raw 1–21 step value. pySwitchbot's lux lookup table is approximate, so
  it is deliberately not applied (never guess values).
- No battery (mains-powered).
- `models` entry: `{"model":"Hub2","tag":"v","values":["temperature","humidity","light_level"]}`.

### Shared rules (unchanged from meter_co2)

- Short payloads skip the affected fields; if nothing decodes, return
  `None` so the caller emits the raw line only.
- Additive schema change only: existing output is byte-identical.

## Testing

- Fixtures from today's real captures in `tests/fixtures/ble/meter.txt`
  and `tests/fixtures/ble/hub2.txt`, with device display readings noted in
  comments. **MAC bytes (payload bytes 0..6) are masked to
  `000000000000`** before committing — this is a public repo; the MAC is
  not a decoded field, so masking does not weaken the test.
- Unit tests: expected values above, negative temperature, humidity
  masking, short payloads, Meter svc fallback (no mfr data).
- Existing meter_co2 tests must pass unchanged after the refactor.
- Manual: rerun `swb scan --once` on the Pi and sanity-check decoded
  values (procedure in `docs/testing.md`).

## Non-goals

- Tags 0x00 / 0x32 (unidentified hardware).
- Meter Plus ('i') / outdoor meter ('w') — no owned hardware, no fixture.
- Lux conversion for Hub 2 light_level.
