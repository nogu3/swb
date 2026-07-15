//! End-to-end CLI tests that need no hardware, network, or credentials.

use std::process::Command;

fn swb() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_swb"));
    // Isolate from the developer's real config and credentials.
    cmd.env("XDG_CONFIG_HOME", env!("CARGO_TARGET_TMPDIR"))
        .env_remove("SWITCHBOT_TOKEN")
        .env_remove("SWITCHBOT_SECRET")
        .env_remove("RUST_LOG");
    cmd
}

#[test]
fn models_emits_one_json_document() {
    let out = swb().arg("models").output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v[0]["model"], "MeterProCO2");
    assert_eq!(v[0]["tag"], "5");
    assert_eq!(v[0]["values"][0], "temperature");
}

#[test]
fn scan_without_mode_is_usage_error_exit_2() {
    let out = swb().arg("scan").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    let err: serde_json::Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(err["error"]["kind"], "usage");
}

#[test]
fn scan_with_both_modes_is_usage_error_exit_2() {
    let out = swb().args(["scan", "--follow", "--once"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn devices_without_credentials_is_auth_error_exit_5() {
    let out = swb().arg("devices").output().unwrap();
    assert_eq!(out.status.code(), Some(5));
    assert!(out.stdout.is_empty());
    let err: serde_json::Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(err["error"]["kind"], "auth");
    let detail = err["error"]["detail"].as_str().unwrap();
    assert!(detail.contains("SWITCHBOT_TOKEN"));
}

#[test]
fn help_prints_and_exits_zero() {
    let out = swb().arg("--help").output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("scan"));
}
