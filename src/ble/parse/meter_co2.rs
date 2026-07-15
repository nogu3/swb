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
