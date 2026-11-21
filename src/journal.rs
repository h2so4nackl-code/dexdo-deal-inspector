use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    io::{self, Write},
};

#[derive(Deserialize)]
struct Schema {
    schema: String,
}

#[derive(Deserialize, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum IntentKind {
    LegacyUnknown,
    Foreground,
    OnDemand,
    PolicyNextSeller,
    ContinuityNextSeller,
    ContinuityRenewal,
}

#[derive(Deserialize)]
struct Intent {
    kind: IntentKind,
}

#[derive(Deserialize)]
struct QuotedOrder {
    order_id: u128,
}

#[derive(Deserialize)]
struct Quote {
    filled_ticks: u128,
    complete: bool,
}

#[derive(Deserialize, Eq, PartialEq)]
struct ResolvedMatch {
    token_contract: String,
    order_id: u128,
    ticks: u128,
    clearing_price: u128,
}

#[derive(Deserialize)]
struct Journal {
    note_addr: String,
    order_book: String,
    submit_identity: String,
    created_at_unix: u64,
    intent: Option<Intent>,
    quoted_order: Option<QuotedOrder>,
    quote: Quote,
    ticks: u128,
    max_price_per_tick: u128,
    escrow: u128,
    order_id: Option<u128>,
    resolved_match: Option<ResolvedMatch>,
    #[serde(default)]
    resolved_matches: Vec<ResolvedMatch>,
}

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Observation {
    SubmitRecorded,
    BuyerOrderRecorded,
    MatchesRecorded,
    ConflictingMatchRecords,
}

#[derive(Serialize)]
pub(crate) struct Report {
    evidence_scope: &'static str,
    journal_version: u8,
    intent: IntentKind,
    observation: Observation,
    explanation: &'static str,
    created_at_unix: u64,
    buyer_order_id: Option<String>,
    quoted_seller_order_id: Option<String>,
    requested_ticks: String,
    max_price_raw: String,
    escrow_raw: String,
    recorded_matches: usize,
}

pub(crate) fn inspect(input: &str) -> Result<Report, String> {
    let schema: Schema = parse(input, "missing or malformed journal schema")?;
    let version = match schema.schema.as_str() {
        "dexdo.buyer.submit.v1" => 1,
        "dexdo.buyer.submit.v2" => 2,
        "dexdo.buyer.submit.v3" => 3,
        "dexdo.buyer.subscription.submit.v1" | "dexdo.buyer.subscription.submit.v2" => {
            return Err("this is a subscription submit journal; this version supports ordinary BUY journals".into());
        }
        _ => {
            return Err("unsupported journal schema; select an ordinary BUY submit journal".into())
        }
    };
    let journal: Journal = parse(input, "malformed ordinary BUY journal")?;
    let valid_identity = journal
        .submit_identity
        .strip_prefix("boc-sha256:")
        .is_some_and(|hash| hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()));
    if !valid_identity || journal.note_addr.is_empty() || journal.order_book.is_empty() {
        return Err("journal is missing a usable submit identity or address".into());
    }
    let valid_quote = match journal.quoted_order {
        Some(_) => journal.quote.complete && journal.quote.filled_ticks == journal.ticks,
        None => version != 1 && !journal.quote.complete && journal.quote.filled_ticks == 0,
    };
    if !valid_quote || journal.ticks == 0 || version < 3 && journal.order_id.is_some() {
        return Err("contradictory BUY journal fields".into());
    }
    let intent = match (version, journal.intent) {
        (1, None) => IntentKind::LegacyUnknown,
        (2 | 3, Some(intent)) => intent.kind,
        _ => return Err("BUY intent does not match the journal version".into()),
    };

    let mut contracts = HashSet::new();
    let duplicate_contract = journal.resolved_matches.iter().any(|fill| {
        fill.token_contract.is_empty()
            || !contracts.insert(fill.token_contract.to_ascii_lowercase())
    });
    let conflicting_scalar = journal
        .resolved_matches
        .first()
        .zip(journal.resolved_match.as_ref())
        .is_some_and(|(first, scalar)| first != scalar);
    let resolved_count = journal
        .resolved_matches
        .len()
        .max(usize::from(journal.resolved_match.is_some()));
    let (observation, explanation) = if duplicate_contract
        || conflicting_scalar
        || journal
            .resolved_match
            .as_ref()
            .is_some_and(|fill| fill.token_contract.is_empty())
    {
        (
            Observation::ConflictingMatchRecords,
            "The stored match records disagree. The journal cannot establish the outcome.",
        )
    } else if resolved_count > 0 {
        (Observation::MatchesRecorded, "The journal contains local match records. Gateway readiness, settlement and current on-chain state remain unverified.")
    } else if journal.order_id.is_some() {
        (Observation::BuyerOrderRecorded, "The buyer order ID was recorded locally. This snapshot has no recorded match and cannot establish the current order outcome.")
    } else {
        (Observation::SubmitRecorded, "A durable submit record exists. The journal alone cannot establish whether the money request reached the chain.")
    };

    Ok(Report {
        evidence_scope: "local_journal",
        journal_version: version,
        intent,
        observation,
        explanation,
        created_at_unix: journal.created_at_unix,
        buyer_order_id: journal.order_id.map(|id| id.to_string()),
        quoted_seller_order_id: journal.quoted_order.map(|ask| ask.order_id.to_string()),
        requested_ticks: journal.ticks.to_string(),
        max_price_raw: journal.max_price_per_tick.to_string(),
        escrow_raw: journal.escrow.to_string(),
        recorded_matches: resolved_count,
    })
}

fn parse<'a, T: Deserialize<'a>>(input: &'a str, message: &str) -> Result<T, String> {
    serde_json::from_str(input).map_err(|error| {
        format!(
            "{message} at line {}, column {}",
            error.line(),
            error.column()
        )
    })
}

impl Report {
    pub(crate) fn write_text(&self, out: &mut dyn Write) -> io::Result<()> {
        writeln!(out, "Local BUY journal v{}", self.journal_version)?;
        writeln!(out, "{}", self.explanation)?;
        writeln!(
            out,
            "Buyer order: {}",
            self.buyer_order_id.as_deref().unwrap_or("none recorded")
        )?;
        writeln!(
            out,
            "Quoted seller order: {}",
            self.quoted_seller_order_id
                .as_deref()
                .unwrap_or("none recorded")
        )?;
        writeln!(
            out,
            "Requested ticks: {} | recorded matches: {}",
            self.requested_ticks, self.recorded_matches
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> serde_json::Value {
        serde_json::from_str(include_str!("../examples/buyer-order-v3.json")).unwrap()
    }

    #[test]
    fn accepts_legacy_and_resting_journals() {
        let mut v1 = fixture();
        v1["schema"] = "dexdo.buyer.submit.v1".into();
        v1.as_object_mut().unwrap().remove("order_id");
        v1.as_object_mut().unwrap().remove("intent");
        assert_eq!(
            inspect(&v1.to_string()).unwrap().observation,
            Observation::SubmitRecorded
        );

        let mut v2 = fixture();
        v2["schema"] = "dexdo.buyer.submit.v2".into();
        v2.as_object_mut().unwrap().remove("order_id");
        v2["quoted_order"] = serde_json::Value::Null;
        v2["expected_token_contract"] = serde_json::Value::Null;
        v2["quote"] =
            serde_json::json!({"complete":false,"filled_ticks":0,"total_with_fee":0,"fills":[]});
        assert_eq!(
            inspect(&v2.to_string()).unwrap().observation,
            Observation::SubmitRecorded
        );
    }

    #[test]
    fn preserves_wide_ids_and_detects_conflicting_match_records() {
        let exact = include_str!("../examples/buyer-order-v3.json").replace(
            "\"order_id\": 1052",
            "\"order_id\": 340282366920938463463374607431768211455",
        );
        assert_eq!(
            inspect(&exact).unwrap().buyer_order_id.unwrap(),
            u128::MAX.to_string()
        );

        let mut journal = fixture();
        journal["resolved_match"] =
            serde_json::json!({"token_contract":"hex","ticks":2,"order_id":1,"clearing_price":100});
        assert_eq!(
            inspect(&journal.to_string()).unwrap().observation,
            Observation::MatchesRecorded
        );
        journal["resolved_matches"] = serde_json::json!([{"token_contract":"hex","ticks":2,"order_id":2,"clearing_price":100}]);
        assert_eq!(
            inspect(&journal.to_string()).unwrap().observation,
            Observation::ConflictingMatchRecords
        );
    }

    #[test]
    fn refuses_future_schemas_and_sanitizes_parse_errors() {
        let future = r#"{"schema":"private-value"}"#;
        assert!(!inspect(future).err().unwrap().contains("private-value"));
        let invalid =
            r#"{"schema":"dexdo.buyer.submit.v3","note_addr":{"seed":"sensitive-marker"}}"#;
        assert!(!inspect(invalid).err().unwrap().contains("sensitive-marker"));
    }
}
