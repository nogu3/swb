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
