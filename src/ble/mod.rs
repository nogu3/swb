//! BLE backend: passive scan loop and advertisement parsers.
//!
//! Async stays at this I/O boundary: a current-thread tokio runtime lives
//! only for the duration of the scan; parsers are pure sync functions.

pub mod parse;

use std::collections::HashSet;
use std::time::Duration;

use btleplug::api::{Central, CentralEvent, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::{Adapter, Manager, PeripheralId};
use futures::StreamExt;
use uuid::Uuid;

use crate::emit::{self, ScanLine, SwbError};

/// 16-bit UUID 0xfd3d expanded to the Bluetooth base UUID: the SwitchBot
/// service-data key.
const FD3D: Uuid = Uuid::from_u128(0x0000fd3d_0000_1000_8000_00805f9b34fb);

/// Assemble one JSONL line. `mfr_payload` arrives without the company ID
/// (btleplug strips it); `mfr_hex` re-prepends the little-endian company
/// ID so the emitted hex matches the over-the-air bytes.
pub fn build_line(
    ts: String,
    addr: String,
    rssi: Option<i16>,
    mfr_payload: &[u8],
    svc_payload: Option<&[u8]>,
) -> ScanLine {
    let decoded = parse::decode(Some(mfr_payload), svc_payload);
    let mut mfr_hex = emit::hex(&parse::COMPANY_ID.to_le_bytes());
    mfr_hex.push_str(&emit::hex(mfr_payload));
    let (model, values) = match decoded {
        Some(d) => (Some(d.model), Some(d.values)),
        None => (None, None),
    };
    ScanLine {
        ts,
        addr,
        rssi,
        mfr_hex: Some(mfr_hex),
        svc_hex: svc_payload.map(emit::hex),
        model,
        values,
    }
}

/// Run the scan loop until interrupted (`follow`) or until `timeout_secs`
/// elapses (`--once`, deduplicated per device).
pub fn run_scan(follow: bool, timeout_secs: u64) -> Result<(), SwbError> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| SwbError::other(format!("tokio runtime: {e}")))?;
    rt.block_on(scan(follow, timeout_secs))
}

async fn scan(follow: bool, timeout_secs: u64) -> Result<(), SwbError> {
    let manager = Manager::new().await.map_err(adapter_err)?;
    let central = manager
        .adapters()
        .await
        .map_err(adapter_err)?
        .into_iter()
        .next()
        .ok_or_else(|| SwbError::adapter("no BLE adapter found"))?;
    let mut events = central.events().await.map_err(adapter_err)?;
    central
        .start_scan(ScanFilter::default())
        .await
        .map_err(adapter_err)?;
    tracing::info!(follow, timeout_secs, "BLE scan started");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);
    let mut seen: HashSet<String> = HashSet::new();
    let mut emitted = 0usize;

    loop {
        let event = if follow {
            events.next().await
        } else {
            match tokio::time::timeout_at(deadline, events.next()).await {
                Ok(event) => event,
                Err(_) => break, // collection window elapsed
            }
        };
        let Some(event) = event else { break };

        if let CentralEvent::ManufacturerDataAdvertisement {
            id,
            manufacturer_data,
        } = event
        {
            let Some(payload) = manufacturer_data.get(&parse::COMPANY_ID) else {
                continue;
            };
            let Some(line) = assemble(&central, &id, payload).await else {
                continue;
            };
            if !follow && !seen.insert(line.addr.clone()) {
                continue;
            }
            emit::print_json(&line);
            emitted += 1;
        }
    }

    let _ = central.stop_scan().await;
    if !follow && emitted == 0 {
        return Err(SwbError::timeout(format!(
            "no SwitchBot advertisement received within {timeout_secs}s"
        )));
    }
    Ok(())
}

/// Pull address/rssi/service-data from the peripheral cache and build the
/// line. Failures here just skip the event — never break the stream.
async fn assemble(central: &Adapter, id: &PeripheralId, mfr_payload: &[u8]) -> Option<ScanLine> {
    let peripheral = central.peripheral(id).await.ok()?;
    let props = peripheral.properties().await.ok().flatten()?;
    let svc = props.service_data.get(&FD3D).map(Vec::as_slice);
    Some(build_line(
        emit::now_ts(),
        props.address.to_string(),
        props.rssi,
        mfr_payload,
        svc,
    ))
}

fn adapter_err(e: btleplug::Error) -> SwbError {
    SwbError::adapter(format!("BLE adapter error: {e}"))
}
