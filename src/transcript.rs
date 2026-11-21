use serde::Serialize;
use serde_json::Value;
use std::io::{self, Write};

const MAX_EVENTS: usize = 1000;

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Observation {
    BuySubmitted,
    ResumeSelected,
    ResumedFromJournal,
    HandoverWaiting,
    HandoverReceived,
    HandoverTimeout,
    HandoverDecryptFailed,
    EndpointReady,
    DealKeptOpen,
    GatewayConnectionFailed,
    GatewayAuthenticationFailed,
    AmbiguousSubmit,
    SettlementSubmitted,
    SettledReported,
    JournalReportedClosed,
    JournalRetained,
}

impl Observation {
    fn label(&self) -> &'static str {
        match self {
            Self::BuySubmitted => "BUY submission reported by the client",
            Self::ResumeSelected => "Client selected an existing deal for resume",
            Self::ResumedFromJournal => "Client selected a deal from the durable journal",
            Self::HandoverWaiting => "Client waiting for seller handover",
            Self::HandoverReceived => "Client reported receiving seller handover",
            Self::HandoverTimeout => "Client reported that handover timed out",
            Self::HandoverDecryptFailed => "Client could not decrypt the seller handover",
            Self::EndpointReady => "Client reported that its local buyer endpoint is ready",
            Self::DealKeptOpen => "Client kept the deal open for retry on the same deal",
            Self::GatewayConnectionFailed => "Client reported a seller gateway connection failure",
            Self::GatewayAuthenticationFailed => {
                "Client reported a seller gateway authentication failure"
            }
            Self::AmbiguousSubmit => "Money submission outcome is uncertain",
            Self::SettlementSubmitted => "Client reported submitting settlement",
            Self::SettledReported => "Client reported settlement",
            Self::JournalReportedClosed => "Client reported closing the BUY journal",
            Self::JournalRetained => {
                "Client reported retaining the journal after an uncertain submission"
            }
        }
    }
}

#[derive(Serialize)]
struct Event {
    line: usize,
    seq: Option<u64>,
    session: Option<String>,
    observation: Observation,
    explanation: &'static str,
}

#[derive(Serialize)]
pub(crate) struct Report {
    evidence_scope: &'static str,
    events: Vec<Event>,
    ignored_nonempty_lines: usize,
    malformed_json_lines: usize,
    omitted_observations: usize,
}

pub(crate) fn inspect(input: &str) -> Report {
    let mut report = Report {
        evidence_scope: "local_transcript",
        events: Vec::new(),
        ignored_nonempty_lines: 0,
        malformed_json_lines: 0,
        omitted_observations: 0,
    };
    for (index, line) in input.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut frame = None;
        let observation = if line.starts_with('{') {
            match serde_json::from_str::<Value>(line) {
                Ok(value) => frame = Some(value),
                Err(_) => {
                    report.malformed_json_lines += 1;
                    continue;
                }
            }
            frame.as_ref().and_then(machine)
        } else {
            plain(line)
        };
        let Some(observation) = observation else {
            report.ignored_nonempty_lines += 1;
            continue;
        };
        if report.events.len() >= MAX_EVENTS {
            report.omitted_observations += 1;
            continue;
        }
        let explanation = observation.label();
        let seq = frame.as_ref().and_then(|v| v["seq"].as_u64());
        let session = frame
            .as_ref()
            .and_then(|v| v["session_id"].as_str())
            .filter(|s| {
                s.strip_prefix("buyer-")
                    .is_some_and(|id| id.len() == 6 && id.bytes().all(|b| b.is_ascii_hexdigit()))
            })
            .map(str::to_owned);
        report.events.push(Event {
            line: index + 1,
            seq,
            session,
            observation,
            explanation,
        });
    }
    report
}

fn machine(v: &Value) -> Option<Observation> {
    use Observation::*;
    match (v["schema"].as_str()?, v["event"].as_str()?) {
        ("dexdo.buyer.event.v2", "buy_submitted") => Some(BuySubmitted),
        ("dexdo.buyer.event.v2", "resume_selected") if v["source"] == "durable_journal" => {
            Some(ResumedFromJournal)
        }
        ("dexdo.buyer.event.v2", "resume_selected") => Some(ResumeSelected),
        ("dexdo.buyer.event.v2", "handover_waiting") => Some(HandoverWaiting),
        ("dexdo.buyer.event.v2", "handover_received") => Some(HandoverReceived),
        ("dexdo.buyer.event.v2", "endpoint_ready") => Some(EndpointReady),
        ("dexdo.buyer.event.v2", "settlement_submitted") => Some(SettlementSubmitted),
        ("dexdo.buyer.event.v2", "settled") => Some(SettledReported),
        ("dexdo.error.v1", "error")
            if matches!(
                v["operation"].as_str(),
                Some("buyer_start" | "buyer_runtime")
            ) =>
        {
            match v["code"].as_str()? {
                "HANDOVER_TIMEOUT" => Some(HandoverTimeout),
                "HANDOVER_DECRYPT_FAILED" => Some(HandoverDecryptFailed),
                "AMBIGUOUS_SUBMIT" => Some(AmbiguousSubmit),
                "GATEWAY_CONNECT_FAILED" | "E_GATEWAY_UNREACHABLE" => Some(GatewayConnectionFailed),
                "GATEWAY_AUTH_FAILED" => Some(GatewayAuthenticationFailed),
                _ => None,
            }
        }
        _ => None,
    }
}

fn plain(line: &str) -> Option<Observation> {
    use Observation::*;
    let line = tracing_message(line).unwrap_or(line);
    if let Some(fields) = line.strip_prefix("policy_action ") {
        let mut fields = fields.split_ascii_whitespace();
        if fields.clone().any(|field| field == "action=keep_open")
            && fields.any(|field| field == "result=deal_kept_open")
        {
            return Some(DealKeptOpen);
        }
    }
    if line.starts_with("buyer_submit_journal_closed ") {
        return Some(JournalReportedClosed);
    }
    if line.starts_with("buyer continuity: buy outcome unknown after money submit; durable journal retained; fresh buy suppressed") {
        return Some(JournalRetained);
    }
    if line.starts_with("E_GATEWAY_UNREACHABLE:")
        || line.starts_with("error[E_GATEWAY_UNREACHABLE] (network):")
    {
        return Some(GatewayConnectionFailed);
    }
    None
}

// DEX.DO writes tracing-subscriber’s plain timestamp/level/target format to stderr.
fn tracing_message(line: &str) -> Option<&str> {
    let (header, message) = line.split_once(": ")?;
    let mut fields = header.split_ascii_whitespace();
    let timestamp = fields.next()?;
    let level = fields.next()?;
    let target = fields.next()?;
    if !timestamp.contains('T')
        || !timestamp.ends_with('Z')
        || !matches!(level, "TRACE" | "DEBUG" | "INFO" | "WARN" | "ERROR")
        || !target.starts_with("dexdo::")
    {
        return None;
    }
    Some(message)
}

impl Report {
    pub(crate) fn write_text(&self, out: &mut dyn Write) -> io::Result<()> {
        writeln!(out, "Local buyer transcript (observations only)")?;
        for event in &self.events {
            write!(out, "line {}", event.line)?;
            if let Some(session) = &event.session {
                write!(out, " [{session}]")?;
            }
            writeln!(out, ": {}", event.explanation)?;
        }
        if self.events.is_empty() {
            writeln!(out, "No recognized BUY observations in this file.")?;
        }
        writeln!(
            out,
            "Ignored: {} | malformed JSON: {} | omitted observations: {}",
            self.ignored_nonempty_lines, self.malformed_json_lines, self.omitted_observations
        )
    }
}
