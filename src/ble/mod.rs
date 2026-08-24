//! BLE backend: passive scan loop and advertisement parsers.
//!
//! Async stays at this I/O boundary: a current-thread tokio runtime lives
//! only for the duration of the scan; parsers are pure sync functions.

pub mod parse;

use std::collections::{HashMap, HashSet};
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

/// Per-device dedup for `--once`. A device is settled by its first *decoded*
/// line, not its first advertisement: manufacturer and service data arrive in
/// separate packets, so the first line for a device often lacks the service
/// data that model detection needs (issue #1). Undecoded lines are held back
/// (latest wins) and flushed at the end of the window, so a device that never
/// decodes still produces its raw line exactly once.
struct OnceWindow {
    settled: HashSet<String>,
    pending: HashMap<String, ScanLine>,
}

impl OnceWindow {
    fn new() -> Self {
        Self {
            settled: HashSet::new(),
            pending: HashMap::new(),
        }
    }

    /// Returns the line to emit now, or `None` if it was held or skipped.
    fn offer(&mut self, line: ScanLine) -> Option<ScanLine> {
        if self.settled.contains(&line.addr) {
            return None;
        }
        if line.model.is_some() {
            self.settled.insert(line.addr.clone());
            self.pending.remove(&line.addr);
            Some(line)
        } else {
            self.pending.insert(line.addr.clone(), line);
            None
        }
    }

    /// Latest raw lines of the devices that never decoded, sorted by address
    /// for deterministic output.
    fn flush(self) -> Vec<ScanLine> {
        let mut lines: Vec<ScanLine> = self.pending.into_values().collect();
        lines.sort_by(|a, b| a.addr.cmp(&b.addr));
        lines
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
    let mut window = OnceWindow::new();
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
            if follow {
                emit::print_json(&line);
                emitted += 1;
            } else if let Some(line) = window.offer(line) {
                emit::print_json(&line);
                emitted += 1;
            }
        }
    }

    let _ = central.stop_scan().await;
    if !follow {
        for line in window.flush() {
            emit::print_json(&line);
            emitted += 1;
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emit::Values;

    fn line(addr: &str, decoded: bool) -> ScanLine {
        ScanLine {
            ts: "2026-08-25T00:00:00.000Z".to_string(),
            addr: addr.to_string(),
            rssi: Some(-70),
            mfr_hex: Some("6909".to_string()),
            svc_hex: decoded.then(|| "350064".to_string()),
            model: decoded.then_some("MeterProCO2"),
            values: decoded.then(Values::new),
        }
    }

    #[test]
    fn decoded_line_emits_immediately_and_settles_device() {
        let mut w = OnceWindow::new();
        assert!(w.offer(line("AA", true)).is_some());
        // Later advertisements for a settled device are skipped, decoded or not.
        assert!(w.offer(line("AA", true)).is_none());
        assert!(w.offer(line("AA", false)).is_none());
        assert!(w.flush().is_empty());
    }

    #[test]
    fn raw_line_is_held_until_a_decoded_one_arrives() {
        let mut w = OnceWindow::new();
        assert!(w.offer(line("AA", false)).is_none());
        let emitted = w.offer(line("AA", true)).expect("decoded line must emit");
        assert!(emitted.model.is_some());
        // The pending raw line was superseded by the decoded one.
        assert!(w.flush().is_empty());
    }

    #[test]
    fn never_decoded_device_flushes_its_latest_raw_line_at_window_end() {
        let mut w = OnceWindow::new();
        let mut first = line("AA", false);
        first.rssi = Some(-80);
        let mut second = line("AA", false);
        second.rssi = Some(-60);
        assert!(w.offer(first).is_none());
        assert!(w.offer(second).is_none());
        let flushed = w.flush();
        assert_eq!(flushed.len(), 1);
        assert_eq!(flushed[0].rssi, Some(-60)); // latest raw wins
        assert!(flushed[0].model.is_none());
    }

    #[test]
    fn flush_is_sorted_by_addr_for_deterministic_output() {
        let mut w = OnceWindow::new();
        assert!(w.offer(line("BB", false)).is_none());
        assert!(w.offer(line("AA", false)).is_none());
        let addrs: Vec<String> = w.flush().into_iter().map(|l| l.addr).collect();
        assert_eq!(addrs, ["AA", "BB"]);
    }

    #[test]
    fn devices_are_independent() {
        let mut w = OnceWindow::new();
        assert!(w.offer(line("AA", true)).is_some());
        assert!(w.offer(line("BB", false)).is_none());
        assert!(w.offer(line("CC", true)).is_some());
        let flushed = w.flush();
        assert_eq!(flushed.len(), 1);
        assert_eq!(flushed[0].addr, "BB");
    }
}
