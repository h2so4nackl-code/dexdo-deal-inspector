use serde_json::Value;
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};

fn run(command: &str, file: PathBuf) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dexdo-deal-inspector"))
        .arg(command)
        .arg(file)
        .arg("--json")
        .output()
        .unwrap()
}

fn example(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join(name)
}

fn report(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn distinguishes_buyer_order_from_quoted_seller_order_and_preserves_input() {
    let file = example("buyer-order-v3.json");
    let before = fs::read(&file).unwrap();
    let observation = report(run("journal", file.clone()));
    assert_eq!(observation["observation"], "buyer_order_recorded");
    assert_eq!(observation["buyer_order_id"], "1052");
    assert_eq!(observation["quoted_seller_order_id"], "731");
    assert_eq!(fs::read(file).unwrap(), before);
    assert!(!observation.to_string().contains("boc-sha256"));
}

#[test]
fn log_tracks_resume_gateway_failure_and_keep_open_without_raw_data() {
    let observation = report(run("log", example("keep-open.ndjson")));
    let events = observation["events"].as_array().unwrap();
    let labels: Vec<_> = events
        .iter()
        .map(|e| e["observation"].as_str().unwrap())
        .collect();
    assert_eq!(
        labels,
        [
            "resumed_from_journal",
            "handover_waiting",
            "handover_received",
            "gateway_connection_failed",
            "deal_kept_open"
        ]
    );
    assert_eq!(events[0]["session"], "buyer-aabbcc");
    assert!(!observation.to_string().contains("TEST-PRIVATE-MARKER"));
    assert!(!observation.to_string().contains("https://"));
}

#[test]
fn missing_file_does_not_claim_no_deal() {
    let output = run("journal", example("absent"));
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("absence alone"));
}
