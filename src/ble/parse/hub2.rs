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
