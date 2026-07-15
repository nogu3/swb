//! Client tests against a local one-shot mock HTTP server (no real API).

use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

/// Serve one canned HTTP response; return (base_url, handle-that-yields-the-request).
fn one_shot_server(response_body: &'static str) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = Vec::with_capacity(8192);
        let mut chunk = [0u8; 8192];
        // ureq may write the request line/headers and a POST body as separate
        // TCP segments; a single read() can race the second write. Loop until
        // we've seen the header terminator and (if present) the full body per
        // Content-Length before treating the request as complete.
        loop {
            let n = stream.read(&mut chunk).unwrap();
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            let Some(header_end) = find_subslice(&buf, b"\r\n\r\n") else {
                continue;
            };
            let body_len = std::str::from_utf8(&buf[..header_end])
                .ok()
                .and_then(|headers| {
                    headers.lines().find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                })
                .unwrap_or(0);
            let received_body_len = buf.len() - (header_end + 4);
            if received_body_len >= body_len {
                break;
            }
        }
        let request = String::from_utf8_lossy(&buf).to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response_body.len(),
            response_body
        );
        stream.write_all(response.as_bytes()).unwrap();
        request
    });
    (format!("http://{addr}"), handle)
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[test]
fn get_sends_signed_headers_and_returns_body() {
    let (base, handle) =
        one_shot_server(r#"{"statusCode":100,"message":"success","body":{"deviceList":[]}}"#);
    let client = swb::cloud::Client::with_base("tok".to_string(), "sec".to_string(), base);
    let body = client.get("/v1.1/devices").unwrap();
    assert_eq!(body["deviceList"], serde_json::json!([]));

    let request = handle.join().unwrap();
    assert!(request.starts_with("GET /v1.1/devices"));
    assert!(request.contains("Authorization: tok"));
    assert!(request.contains("sign: "));
    assert!(request.contains("nonce: "));
    assert!(request.contains("\r\nt: "));
}

#[test]
fn post_sends_json_payload() {
    let (base, handle) =
        one_shot_server(r#"{"statusCode":100,"message":"success","body":{"commandId":"C1"}}"#);
    let client = swb::cloud::Client::with_base("tok".to_string(), "sec".to_string(), base);
    let payload = serde_json::json!({
        "command": "turnOn", "parameter": "default", "commandType": "command"
    });
    let body = client
        .post("/v1.1/devices/DEVICE_ID/commands", &payload)
        .unwrap();
    assert_eq!(body["commandId"], "C1");

    let request = handle.join().unwrap();
    assert!(request.starts_with("POST /v1.1/devices/DEVICE_ID/commands"));
    assert!(request.contains(r#""command":"turnOn""#));
}

#[test]
fn api_rejection_maps_to_device_error_exit_6() {
    let (base, _handle) = one_shot_server(r#"{"statusCode":151,"message":"device type error"}"#);
    let client = swb::cloud::Client::with_base("tok".to_string(), "sec".to_string(), base);
    let err = client.get("/v1.1/devices/BADID/status").unwrap_err();
    assert_eq!(err.kind.exit_code(), 6);
}
