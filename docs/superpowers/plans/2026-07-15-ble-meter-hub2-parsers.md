# Meter / Hub 2 BLE Parsers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add BLE decode support for the SwitchBot Meter (tag `'T'`) and Hub 2 (tag `'v'`), sharing the 3-byte temperature/humidity codec all three supported models use.

**Architecture:** Pure parser functions (`fn parse(mfr: Option<&[u8]>, svc: Option<&[u8]>) -> Option<Values>`) in one-file-per-model under `src/ble/parse/`, dispatched by service-data model tag in `parse/mod.rs`. A new private `sensor_th` module holds the shared codec; the existing `meter_co2` parser is refactored onto it with byte-identical output.

**Tech Stack:** Rust stable (edition 2021), serde_json. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-07-15-ble-meter-hub2-parsers-design.md`

## Global Constraints

- Every cargo command on this machine needs `--features vendored-dbus` (no system libdbus).
- `cargo fmt` and `cargo clippy --all-targets --features vendored-dbus -- -D warnings` must pass.
- No panics in library code; `unwrap`/`expect` only in tests.
- Fixtures are real-hardware captures (2026-07-15 Raspberry Pi scan) with the 6 MAC bytes at the start of the manufacturer payload masked to `000000000000` (public repo). Never substitute invented payloads.
- Existing output must not change: schema additions only.
- Conventional Commits, English.

### Capture reference (already validated against pySwitchbot)

Byte offsets are into the manufacturer payload **without** the 2-byte 0x0969 company ID; the payload starts with the 6-byte device MAC.

- Meter: mfr `…5303039b3e` → temp/humidity bytes 8..11 = `03 9b 3e` → 27.3 °C, 62 %; svc `540064039b3e` → battery = svc[2] & 0x7f = 100. Fallback: svc bytes 3..6 hold the same 3-byte codec when mfr is absent/short.
- Hub 2: mfr `…00ff6a57094c4901983b00` → light_level = m[12] & 0x1f = 9; temp/humidity bytes 13..16 = `01 98 3b` → 24.1 °C, 59 %. svc is just `7600` (tag only). No battery (mains-powered).
- Shared 3-byte codec: byte 0 bits 3..0 = tenths; byte 1 bit 7 = sign (1 = positive), bits 6..0 = integer °C; byte 2 bit 7 = Fahrenheit flag (ignored), bits 6..0 = humidity %.

---

### Task 1: Shared temperature/humidity codec (`sensor_th`)

**Files:**
- Create: `src/ble/parse/sensor_th.rs`
- Modify: `src/ble/parse/mod.rs` (add `mod sensor_th;`)
- Modify: `src/ble/parse/meter_co2.rs` (refactor onto the codec)
- Test: unit tests inside `src/ble/parse/sensor_th.rs`; existing `tests/ble_parse.rs` is the regression net

**Interfaces:**
- Consumes: nothing new.
- Produces: `pub(crate) fn temp_humidity(b: &[u8]) -> Option<(f64, u8)>` in `crate::ble::parse::sensor_th` — returns `(temperature_celsius, humidity_percent)`, `None` if `b.len() < 3`. Tasks 2 and 3 call this exact signature.

- [x] **Step 1: Write `sensor_th.rs` with failing unit tests**

```rust
//! Shared 3-byte temperature/humidity codec used by SwitchBot sensor
//! advertisements (Meter family, Hub 2).
//!
//! Layout: byte 0 bits 3..0 = temperature tenths; byte 1 bit 7 = sign
//! (1 = positive), bits 6..0 = integer Celsius; byte 2 bit 7 =
//! Fahrenheit-preference flag (ignored), bits 6..0 = humidity %.
//! Reference: pySwitchbot `adv_parsers/meter.py`.

/// Decode 3 bytes into (temperature in Celsius, humidity in %).
/// Returns `None` when fewer than 3 bytes are given.
pub(crate) fn temp_humidity(b: &[u8]) -> Option<(f64, u8)> {
    if b.len() < 3 {
        return None;
    }
    let sign = if b[1] & 0x80 != 0 { 1.0 } else { -1.0 };
    let temperature = sign * (f64::from(b[1] & 0x7f) + f64::from(b[0] & 0x0f) / 10.0);
    Some((temperature, b[2] & 0x7f))
}

#[cfg(test)]
mod tests {
    use super::temp_humidity;

    #[test]
    fn positive_temperature() {
        // 27.3 C / 62 % — bytes from the 2026-07-15 Meter capture.
        assert_eq!(temp_humidity(&[0x03, 0x9b, 0x3e]), Some((27.3, 62)));
    }

    #[test]
    fn negative_temperature_when_sign_bit_clear() {
        assert_eq!(temp_humidity(&[0x05, 0x02, 0x28]), Some((-2.5, 40)));
    }

    #[test]
    fn fahrenheit_flag_is_masked_from_humidity() {
        assert_eq!(temp_humidity(&[0x03, 0x9b, 0xbe]), Some((27.3, 62)));
    }

    #[test]
    fn short_input_is_none() {
        assert_eq!(temp_humidity(&[0x03, 0x9b]), None);
    }
}
```

Note: writing the module and its tests together is still test-first here — the tests are the spec for the codec, and Step 2 proves they run and pass before any caller is refactored onto it. (A "fails first" cycle is impossible for a brand-new module that must compile to run its tests.)

- [x] **Step 2: Register the module and run the new tests**

In `src/ble/parse/mod.rs`, after `pub mod meter_co2;` add:

```rust
mod sensor_th;
```

Run: `cargo test --features vendored-dbus sensor_th`
Expected: 4 passed.

- [x] **Step 3: Refactor `meter_co2.rs` onto the codec**

Replace the temperature/humidity block in `src/ble/parse/meter_co2.rs`. The `if m.len() >= 11` body currently computes sign/temperature/humidity inline; change the file to:

```rust
//! SwitchBot Meter Pro CO2 (model tag '5').
//!
//! Format reference: pySwitchbot `adv_parsers/meter.py` +
//! `adv_parsers/_sensor_th.py`. Fixtures: `tests/fixtures/ble/meter_pro_co2.txt`.

use serde_json::json;

use super::sensor_th;
use crate::emit::Values;

/// Sensor spec range is 400-9999 ppm; larger values are transient
/// artifacts and are dropped rather than emitted.
const CO2_MAX_PPM: u16 = 9999;

pub fn parse(mfr: Option<&[u8]>, svc: Option<&[u8]>) -> Option<Values> {
    let mut values = Values::new();

    if let Some(m) = mfr {
        if m.len() >= 11 {
            if let Some((temperature, humidity)) = sensor_th::temp_humidity(&m[8..11]) {
                values.insert("temperature".to_string(), json!(temperature));
                values.insert("humidity".to_string(), json!(humidity));
            }
        }
        if m.len() >= 15 {
            let co2 = u16::from_be_bytes([m[13], m[14]]);
            if co2 <= CO2_MAX_PPM {
                values.insert("co2".to_string(), json!(co2));
            }
        }
    }

    if let Some(s) = svc {
        if s.len() >= 3 {
            values.insert("battery".to_string(), json!(s[2] & 0x7f));
        }
    }

    if values.is_empty() {
        None
    } else {
        Some(values)
    }
}
```

- [x] **Step 4: Run the full suite to prove the refactor is behavior-preserving**

Run: `cargo test --features vendored-dbus`
Expected: all tests pass (40 total: previous 36 + 4 codec tests), especially `decodes_meter_pro_co2_capture` and `negative_temperature_decodes` unchanged.

- [x] **Step 5: fmt + clippy**

Run: `cargo fmt && cargo clippy --all-targets --features vendored-dbus -- -D warnings`
Expected: no warnings.

- [x] **Step 6: Commit**

```bash
git add src/ble/parse/sensor_th.rs src/ble/parse/mod.rs src/ble/parse/meter_co2.rs
git commit -m "refactor: extract shared 3-byte temp/humidity codec

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: Meter parser (tag 'T')

**Files:**
- Create: `src/ble/parse/meter.rs`
- Create: `tests/fixtures/ble/meter.txt`
- Modify: `src/ble/parse/mod.rs` (module, dispatch arm, MODELS entry)
- Test: `tests/ble_parse.rs`

**Interfaces:**
- Consumes: `sensor_th::temp_humidity(b: &[u8]) -> Option<(f64, u8)>` from Task 1; `crate::emit::Values` (a `BTreeMap<String, serde_json::Value>` alias already used by `meter_co2`).
- Produces: `pub fn parse(mfr: Option<&[u8]>, svc: Option<&[u8]>) -> Option<Values>` in `crate::ble::parse::meter`; `decode()` returns model name `"Meter"` for tag `'T'`; `MODELS[1]` = Meter entry.

- [x] **Step 1: Add the fixture file**

Create `tests/fixtures/ble/meter.txt`:

```
# SwitchBot Meter (original WoSensorTH, model tag 'T') — real-hardware
# capture, 2026-07-15 Raspberry Pi passive scan. The 6 MAC bytes at the
# start of the manufacturer payload are masked to zeros (public repo);
# the MAC is not a decoded field.
# Decoded reading at capture time: 27.3 C / 62 % / battery 100.
#
# Format: <name> <mfr_payload_hex (0x0969 company ID stripped)> <svc_payload_hex>
normal 0000000000005303039b3e 540064039b3e
```

- [x] **Step 2: Generalize the fixture loader in `tests/ble_parse.rs`**

Replace the existing `fixture` helper (keep `unhex` as is) with:

```rust
/// Returns (mfr_payload, svc_payload) for a named line of a fixture file.
fn fixture_in(file: &str, name: &str) -> (Vec<u8>, Vec<u8>) {
    let text = match file {
        "meter_pro_co2" => include_str!("fixtures/ble/meter_pro_co2.txt"),
        "meter" => include_str!("fixtures/ble/meter.txt"),
        _ => panic!("unknown fixture file {file}"),
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        if parts.next() == Some(name) {
            let mfr = unhex(parts.next().expect("mfr field"));
            let svc = unhex(parts.next().expect("svc field"));
            return (mfr, svc);
        }
    }
    panic!("fixture {file}/{name} not found");
}

/// Shorthand for the meter_pro_co2 fixture used by the original tests.
fn fixture(name: &str) -> (Vec<u8>, Vec<u8>) {
    fixture_in("meter_pro_co2", name)
}
```

Run: `cargo test --features vendored-dbus --test ble_parse`
Expected: all existing tests still pass.

- [x] **Step 3: Write the failing Meter tests**

Append to `tests/ble_parse.rs`:

```rust
#[test]
fn decodes_meter_capture() {
    let (mfr, svc) = fixture_in("meter", "normal");
    let decoded = decode(Some(&mfr), Some(&svc)).expect("should decode");
    assert_eq!(decoded.model, "Meter");
    assert_eq!(decoded.values["temperature"], json!(27.3));
    assert_eq!(decoded.values["humidity"], json!(62));
    assert_eq!(decoded.values["battery"], json!(100));
}

#[test]
fn meter_falls_back_to_service_data_when_mfr_short() {
    let (_, svc) = fixture_in("meter", "normal");
    // Payload too short for mfr bytes 8..11 -> svc bytes 3..6 fallback.
    let short = [0u8; 6];
    let decoded = decode(Some(&short), Some(&svc)).expect("should decode");
    assert_eq!(decoded.values["temperature"], json!(27.3));
    assert_eq!(decoded.values["humidity"], json!(62));
    assert_eq!(decoded.values["battery"], json!(100));
}

#[test]
fn meter_negative_temperature_decodes() {
    let (mut mfr, svc) = fixture_in("meter", "normal");
    mfr[8] = 0x05; // fraction .5
    mfr[9] = 0x02; // integer 2, sign bit clear -> negative
    mfr[10] = 0x28; // humidity 40
    let decoded = decode(Some(&mfr), Some(&svc)).expect("should decode");
    assert_eq!(decoded.values["temperature"], json!(-2.5));
    assert_eq!(decoded.values["humidity"], json!(40));
}

#[test]
fn models_table_lists_meter() {
    let m = &MODELS[1];
    assert_eq!(m.model, "Meter");
    assert_eq!(m.tag, 'T');
    assert_eq!(m.values, ["temperature", "humidity", "battery"]);
}
```

Run: `cargo test --features vendored-dbus --test ble_parse meter`
Expected: FAIL — `decode` returns `None` for tag 'T' (`should decode` panics) and `MODELS[1]` is out of bounds.

- [x] **Step 4: Implement the parser**

Create `src/ble/parse/meter.rs`:

```rust
//! SwitchBot Meter (original WoSensorTH, model tag 'T').
//!
//! Format reference: pySwitchbot `adv_parsers/meter.py`
//! (`process_wosensorth`). Fixtures: `tests/fixtures/ble/meter.txt`.

use serde_json::json;

use super::sensor_th;
use crate::emit::Values;

pub fn parse(mfr: Option<&[u8]>, svc: Option<&[u8]>) -> Option<Values> {
    let mut values = Values::new();

    // Manufacturer data is preferred; service data bytes 3..6 carry the
    // same codec and serve as the fallback (pySwitchbot precedence).
    let th = match (mfr, svc) {
        (Some(m), _) if m.len() >= 11 => sensor_th::temp_humidity(&m[8..11]),
        (_, Some(s)) if s.len() >= 6 => sensor_th::temp_humidity(&s[3..6]),
        _ => None,
    };
    if let Some((temperature, humidity)) = th {
        values.insert("temperature".to_string(), json!(temperature));
        values.insert("humidity".to_string(), json!(humidity));
    }

    if let Some(s) = svc {
        if s.len() >= 3 {
            values.insert("battery".to_string(), json!(s[2] & 0x7f));
        }
    }

    if values.is_empty() {
        None
    } else {
        Some(values)
    }
}
```

In `src/ble/parse/mod.rs`:

1. Module list becomes:

```rust
pub mod meter;
pub mod meter_co2;
mod sensor_th;
```

2. `MODELS` becomes:

```rust
pub const MODELS: &[ModelInfo] = &[
    ModelInfo {
        model: "MeterProCO2",
        tag: '5',
        values: &["temperature", "humidity", "co2", "battery"],
    },
    ModelInfo {
        model: "Meter",
        tag: 'T',
        values: &["temperature", "humidity", "battery"],
    },
];
```

3. Add a dispatch arm in `decode()` before the `_ => None` arm:

```rust
b'T' => meter::parse(mfr, svc).map(|values| Decoded {
    model: "Meter",
    values,
}),
```

- [x] **Step 5: Run tests**

Run: `cargo test --features vendored-dbus`
Expected: all pass, including the 4 new Meter tests.

- [x] **Step 6: fmt + clippy**

Run: `cargo fmt && cargo clippy --all-targets --features vendored-dbus -- -D warnings`
Expected: no warnings.

- [x] **Step 7: Commit**

```bash
git add src/ble/parse/meter.rs src/ble/parse/mod.rs tests/ble_parse.rs tests/fixtures/ble/meter.txt
git commit -m "feat: add Meter (tag 'T') BLE parser with real capture

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: Hub 2 parser (tag 'v')

**Files:**
- Create: `src/ble/parse/hub2.rs`
- Create: `tests/fixtures/ble/hub2.txt`
- Modify: `src/ble/parse/mod.rs` (module, dispatch arm, MODELS entry)
- Test: `tests/ble_parse.rs`

**Interfaces:**
- Consumes: `sensor_th::temp_humidity(b: &[u8]) -> Option<(f64, u8)>` from Task 1; `fixture_in(file, name)` loader from Task 2.
- Produces: `pub fn parse(mfr: Option<&[u8]>, svc: Option<&[u8]>) -> Option<Values>` in `crate::ble::parse::hub2`; `decode()` returns model name `"Hub2"` for tag `'v'`; `MODELS[2]` = Hub2 entry.

- [x] **Step 1: Add the fixture file**

Create `tests/fixtures/ble/hub2.txt`:

```
# SwitchBot Hub 2 (model tag 'v') — real-hardware capture, 2026-07-15
# Raspberry Pi passive scan. The 6 MAC bytes at the start of the
# manufacturer payload are masked to zeros (public repo); the MAC is not
# a decoded field.
# Decoded reading at capture time: 24.1 C / 59 % / light_level 9.
#
# Format: <name> <mfr_payload_hex (0x0969 company ID stripped)> <svc_payload_hex>
normal 00000000000000ff6a57094c4901983b00 7600
```

Register it in the `fixture_in` match in `tests/ble_parse.rs` (add before the panic arm):

```rust
        "hub2" => include_str!("fixtures/ble/hub2.txt"),
```

- [x] **Step 2: Write the failing Hub 2 tests**

Append to `tests/ble_parse.rs`:

```rust
#[test]
fn decodes_hub2_capture() {
    let (mfr, svc) = fixture_in("hub2", "normal");
    let decoded = decode(Some(&mfr), Some(&svc)).expect("should decode");
    assert_eq!(decoded.model, "Hub2");
    assert_eq!(decoded.values["temperature"], json!(24.1));
    assert_eq!(decoded.values["humidity"], json!(59));
    assert_eq!(decoded.values["light_level"], json!(9));
    assert!(!decoded.values.contains_key("battery"));
}

#[test]
fn hub2_short_payload_yields_light_level_only() {
    let (mfr, svc) = fixture_in("hub2", "normal");
    // 13 bytes reach the light byte (index 12) but not temp bytes 13..16.
    let decoded = decode(Some(&mfr[..13]), Some(&svc)).expect("should decode");
    assert_eq!(decoded.values["light_level"], json!(9));
    assert!(!decoded.values.contains_key("temperature"));
    assert!(!decoded.values.contains_key("humidity"));
}

#[test]
fn hub2_without_manufacturer_data_returns_none() {
    let (_, svc) = fixture_in("hub2", "normal");
    assert!(decode(None, Some(&svc)).is_none());
}

#[test]
fn models_table_lists_hub2() {
    let m = &MODELS[2];
    assert_eq!(m.model, "Hub2");
    assert_eq!(m.tag, 'v');
    assert_eq!(m.values, ["temperature", "humidity", "light_level"]);
}
```

Run: `cargo test --features vendored-dbus --test ble_parse hub2`
Expected: FAIL — `decode` returns `None` for tag 'v' and `MODELS[2]` is out of bounds.

- [x] **Step 3: Implement the parser**

Create `src/ble/parse/hub2.rs`:

```rust
//! SwitchBot Hub 2 (model tag 'v').
//!
//! Format reference: pySwitchbot `adv_parsers/hub2.py`. Sensor readings
//! ride in manufacturer data; service data carries only the model tag.
//! `light_level` is the raw 1-21 step value — pySwitchbot's lux lookup
//! table is approximate, so it is deliberately not applied.
//! Fixtures: `tests/fixtures/ble/hub2.txt`.

use serde_json::json;

use super::sensor_th;
use crate::emit::Values;

pub fn parse(mfr: Option<&[u8]>, _svc: Option<&[u8]>) -> Option<Values> {
    let m = mfr?;
    let mut values = Values::new();

    if m.len() >= 13 {
        values.insert("light_level".to_string(), json!(m[12] & 0x1f));
    }
    if m.len() >= 16 {
        if let Some((temperature, humidity)) = sensor_th::temp_humidity(&m[13..16]) {
            values.insert("temperature".to_string(), json!(temperature));
            values.insert("humidity".to_string(), json!(humidity));
        }
    }

    if values.is_empty() {
        None
    } else {
        Some(values)
    }
}
```

In `src/ble/parse/mod.rs`:

1. Module list becomes:

```rust
pub mod hub2;
pub mod meter;
pub mod meter_co2;
mod sensor_th;
```

2. Append to `MODELS` (after the Meter entry):

```rust
    ModelInfo {
        model: "Hub2",
        tag: 'v',
        values: &["temperature", "humidity", "light_level"],
    },
```

3. Add a dispatch arm in `decode()` before the `_ => None` arm:

```rust
b'v' => hub2::parse(mfr, svc).map(|values| Decoded {
    model: "Hub2",
    values,
}),
```

- [x] **Step 4: Run tests**

Run: `cargo test --features vendored-dbus`
Expected: all pass, including the 4 new Hub 2 tests.

- [x] **Step 5: fmt + clippy**

Run: `cargo fmt && cargo clippy --all-targets --features vendored-dbus -- -D warnings`
Expected: no warnings.

- [x] **Step 6: Commit**

```bash
git add src/ble/parse/hub2.rs src/ble/parse/mod.rs tests/ble_parse.rs tests/fixtures/ble/hub2.txt
git commit -m "feat: add Hub 2 (tag 'v') BLE parser with real capture

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: Real-device verification on the Pi

**Files:** none (verification only; fix-forward if a defect is found)

**Interfaces:**
- Consumes: the swb binary with Tasks 1-3 merged.
- Produces: confirmation that Meter and Hub 2 lines decode on real hardware.

- [x] **Step 1: Quality gate on the full tree**

Run: `cargo fmt --check && cargo clippy --all-targets --features vendored-dbus -- -D warnings && cargo test --features vendored-dbus`
Expected: clean.

- [x] **Step 2: Cross-build and deploy to the Pi**

`PI_HOST` below is the ssh alias of the Raspberry Pi used for manual
verification (never commit the real hostname).

```bash
cross build --release --target aarch64-unknown-linux-gnu --features vendored-dbus
scp target/aarch64-unknown-linux-gnu/release/swb PI_HOST:/tmp/swb
```

- [x] **Step 3: Verify `models` lists three models**

Run: `ssh PI_HOST '/tmp/swb models'`
Expected: JSON array with MeterProCO2, Meter (tag "T"), Hub2 (tag "v").

- [x] **Step 4: Verify live decode**

Run: `ssh PI_HOST '/tmp/swb scan --once --timeout 15'`
Expected: the tag-'T' device line now carries `"model":"Meter"` with plausible `temperature`/`humidity`/`battery`; the tag-'v' device line carries `"model":"Hub2"` with plausible `temperature`/`humidity` and `light_level` in 1-21. MeterProCO2 lines unchanged. Tag 0x00/0x32 devices still raw-only.

- [x] **Step 5: Report results**

No commit — report the verified output lines (with real MACs redacted) back to the user.
