# Manual real-device verification

`cargo test` never touches hardware, network, or credentials. Real-device
behavior is verified manually with the steps below (do not automate them).

## BLE (Meter Pro CO2 or any SwitchBot broadcaster in range)

1. `cargo run -- scan --once --timeout 15 | jq .`
   - Expect one line per SwitchBot device in range.
   - Every line has `ts`, `addr`, `rssi`, `mfr_hex` (starting `6909`).
   - A Meter Pro CO2 line additionally has `model: "MeterProCO2"` and
     `values.temperature/humidity/co2/battery` that match the device display.
2. `cargo run -- scan --follow | jq .values.co2` for a minute — values
   should update every few seconds and stay plausible (400–2000 ppm indoors).
3. Unplug/disable the BLE adapter and rerun: expect exit code 4 and a
   `{"error":{"kind":"adapter",...}}` line on stderr.
4. With no device in range: `swb scan --once --timeout 3` → exit code 3.

Capturing new fixtures: take the `mfr_hex` (strip the leading `6909`) and
`svc_hex` fields from a scan line of the new model and add them to
`tests/fixtures/ble/<model>.txt` with the device's display readings noted
in a comment. Never commit lines from devices you do not own.

## Cloud API (needs SWITCHBOT_TOKEN / SWITCHBOT_SECRET)

1. `swb devices | jq .` — expect your device list; spot-check one ID.
2. `swb status <deviceId> | jq .` — readings should match the app.
3. `swb cmd <plugId> turnOn` then `turnOff` — the plug should switch and
   the response contain `"message": "success"`-equivalent body content.
4. Break the token (`SWITCHBOT_TOKEN=x swb devices`) → exit code 5.
5. `swb status NOSUCHDEVICE` → exit code 6.

Mind the account rate limit (10,000 calls/day); a manual pass uses ~10.
