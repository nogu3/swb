use serde_json::json;
use swb::ble::parse::{decode, COMPANY_ID, MODELS};

fn unhex(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2), "odd hex length: {s}");
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

/// Returns (mfr_payload, svc_payload) for a named fixture line.
fn fixture(name: &str) -> (Vec<u8>, Vec<u8>) {
    let text = include_str!("fixtures/ble/meter_pro_co2.txt");
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
    panic!("fixture {name} not found");
}

#[test]
fn company_id_is_woan_technology() {
    assert_eq!(COMPANY_ID, 0x0969);
}

#[test]
fn decodes_meter_pro_co2_capture() {
    let (mfr, svc) = fixture("normal");
    let decoded = decode(Some(&mfr), Some(&svc)).expect("should decode");
    assert_eq!(decoded.model, "MeterProCO2");
    assert_eq!(decoded.values["temperature"], json!(27.7));
    assert_eq!(decoded.values["humidity"], json!(36));
    assert_eq!(decoded.values["co2"], json!(725));
    assert_eq!(decoded.values["battery"], json!(100));
}

#[test]
fn co2_above_spec_range_is_dropped() {
    let (mfr, svc) = fixture("co2_out_of_range");
    let decoded = decode(Some(&mfr), Some(&svc)).expect("should decode");
    assert!(!decoded.values.contains_key("co2"));
    assert_eq!(decoded.values["temperature"], json!(27.7));
    assert_eq!(decoded.values["humidity"], json!(36));
}

#[test]
fn co2_at_9999_boundary_is_accepted() {
    let (mfr, svc) = fixture("co2_boundary");
    let decoded = decode(Some(&mfr), Some(&svc)).expect("should decode");
    assert_eq!(decoded.values["co2"], json!(9999));
}

#[test]
fn encrypted_flag_bit_is_masked_for_model_tag() {
    let (mfr, mut svc) = fixture("normal");
    svc[0] |= 0x80;
    let decoded = decode(Some(&mfr), Some(&svc)).expect("should decode");
    assert_eq!(decoded.model, "MeterProCO2");
}

#[test]
fn unknown_model_tag_returns_none() {
    let (mfr, mut svc) = fixture("normal");
    svc[0] = b'w'; // no decode support
    assert!(decode(Some(&mfr), Some(&svc)).is_none());
}

#[test]
fn missing_service_data_returns_none() {
    let (mfr, _) = fixture("normal");
    assert!(decode(Some(&mfr), None).is_none());
}

#[test]
fn short_manufacturer_payload_still_yields_battery() {
    let (_, svc) = fixture("normal");
    let short = [0u8; 6];
    let decoded = decode(Some(&short), Some(&svc)).expect("battery should decode");
    assert_eq!(decoded.values["battery"], json!(100));
    assert!(!decoded.values.contains_key("temperature"));
    assert!(!decoded.values.contains_key("co2"));
}

#[test]
fn negative_temperature_decodes() {
    // Sign bit (0x80) clear on byte 9 means negative: -2.5 C, 40 %.
    let mut mfr = fixture("normal").0;
    mfr[8] = 0x05; // fraction .5
    mfr[9] = 0x02; // integer 2, sign bit clear -> negative
    mfr[10] = 0x28; // humidity 40
    let (_, svc) = fixture("normal");
    let decoded = decode(Some(&mfr), Some(&svc)).expect("should decode");
    assert_eq!(decoded.values["temperature"], json!(-2.5));
    assert_eq!(decoded.values["humidity"], json!(40));
}

#[test]
fn models_table_lists_meter_pro_co2() {
    let m = &MODELS[0];
    assert_eq!(m.model, "MeterProCO2");
    assert_eq!(m.tag, '5');
    assert_eq!(m.values, ["temperature", "humidity", "co2", "battery"]);
}
