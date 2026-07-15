//! Model detection and decode dispatch for SwitchBot advertisements.
//!
//! Decode dictionaries are optional enrichment: an unknown model tag or
//! undecodable payload returns `None` and the caller emits the raw line.

pub mod hub2;
pub mod meter;
pub mod meter_co2;
mod sensor_th;

use crate::emit::Values;

/// Bluetooth SIG company identifier for Woan Technology (SwitchBot).
pub const COMPANY_ID: u16 = 0x0969;

/// A model with BLE decode support (static info for `swb models`).
pub struct ModelInfo {
    pub model: &'static str,
    pub tag: char,
    pub values: &'static [&'static str],
}

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
    ModelInfo {
        model: "Hub2",
        tag: 'v',
        values: &["temperature", "humidity", "light_level"],
    },
];

pub struct Decoded {
    pub model: &'static str,
    pub values: Values,
}

/// Decode one advertisement.
///
/// `mfr` is the 0x0969 manufacturer payload *without* the 2-byte company
/// ID; `svc` is the fd3d service-data payload. The model tag lives in
/// `svc[0]` (bit 7 = encrypted flag, masked off). Without service data we
/// do not guess a model.
pub fn decode(mfr: Option<&[u8]>, svc: Option<&[u8]>) -> Option<Decoded> {
    let tag = svc?.first()? & 0x7f;
    match tag {
        b'5' => meter_co2::parse(mfr, svc).map(|values| Decoded {
            model: "MeterProCO2",
            values,
        }),
        b'T' => meter::parse(mfr, svc).map(|values| Decoded {
            model: "Meter",
            values,
        }),
        b'v' => hub2::parse(mfr, svc).map(|values| Decoded {
            model: "Hub2",
            values,
        }),
        _ => None,
    }
}
