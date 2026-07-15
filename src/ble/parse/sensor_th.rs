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
