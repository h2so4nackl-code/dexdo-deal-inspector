# DEX.DO Deal Inspector

A read-only CLI for diagnosing DEX.DO buyer journals and saved buyer transcripts.
It runs offline and accepts an explicit file path. It does not use a wallet or
issue trading, settlement, recovery or cancellation commands.

## Quick start on Linux or WSL

```sh
cargo build --release --locked
./target/release/dexdo-deal-inspector journal examples/buyer-order-v3.json
./target/release/dexdo-deal-inspector log examples/keep-open.ndjson
```

Replace the sample paths with your inputs. Both examples are synthetic.

- **Journal.** Pass the existing `note-<hash>.json` file from
  `<your DEX.DO data directory>/buyer-submits/`. The CLI recognizes ordinary BUY
  journal schemas v1 through v3. It distinguishes the buyer's own order from the
  quoted seller order and reports whether the local file holds matching records.
  A missing file gives no evidence of the deal outcome.
- **Log.** Pass a saved UTF-8 buyer transcript. The CLI recognizes buyer
  machine-event v2 and machine-error v1 rows plus selected plain-text policy
  and journal diagnostics. It reports recognized observations in file order and
  includes machine session IDs where available. A file can contain multiple
  sessions, and gaps between recognized lines remain unknown.

Add `--json` after the filename for machine-readable output. Order IDs and
amounts are strings in JSON to preserve the exact 128-bit integers. Price and
escrow fields report the raw units stored in the journal. The CLI omits
addresses, raw messages, endpoints and signatures from its report. The original
input files may still contain private data.

The report describes local evidence. Gateway availability, settlement and the
current on-chain outcome require independent evidence. A local match is not
a settlement confirmation, and a `buy_submitted` line is not a count of network
POST requests. The successful exit status means the file was inspected.

Inputs are regular UTF-8 files of up to 16 MiB. Logs report the first 1,000
recognized observations and count additional ones. The first version focuses on
ordinary BUY journals and transcripts. Subscription journals are outside its
supported format.

Verified against the journal and machine event definitions at the
[DEX.DO CLI PR #186 commit](https://github.com/gosh-sh/dexdo-cli/tree/baa87edcb80a4be9afca9446004c92ed106bd0a2).

## Validation for v0.1.0

- All six inspector tests passed. The release build and tests were also run
  successfully in Ubuntu on Windows Subsystem for Linux (WSL).
- The inspector was exercised against a journal and transcripts generated
  from DEX.DO CLI PR #186 tests using a simulated backend. The reports
  recognized a resume from the durable journal, the keep-open policy, and an
  ambiguous submit.
- In a separate read-only check on an existing real BUY journal, the inspector
  reported `submit_recorded` with intent `on_demand`. The v3 journal had no buyer
  order ID, no quoted seller order ID, and no recorded matches. Its SHA-256
  was identical before and after inspection. There was no correlated transcript
  for this journal, so this sample does not establish an on-chain submission,
  a fill, settlement, or a resume event.
