# Pending-checkpoint lookup during consensus replay

Status: focused local tests pass; candidate image and production improvement
remain unverified. This change supplements randomness mapping recovery.

## Evidence and causal limits

The operator's production profile of `sui-node 1.51.1-59e26ba5dc` attributed
20.44% of sampled self CPU to `AuthorityPerEpochStore::write_pending_checkpoint`.
Its duplicate check scans the queued consensus outputs for each new checkpoint.
If the queue cannot drain, repeated negative lookups perform quadratic total work
as replay appends outputs. Kernel page-fault/allocation samples are also present;
the profile does not establish that this lookup causes all of that overhead.

This scan and the checkpoint builder's wait for startup replay predate the
randomness fix. The diff from incident revision `50ba71a66377` to deployed source
`59e26ba5dc91` is empty for `consensus_quarantine.rs`, `consensus_handler.rs`,
`commit_observer.rs`, `commit_consumer.rs`, `commit_syncer.rs`, and consensus
`parameters.rs`. The supplied incident handoff already reports 534,412 commits
replayed in 3 hours 28 minutes on the earlier build, followed by a parked builder,
and quarantine sizes of approximately 535,000 to 560,000.

The new randomness recovery scan has a measured startup cost: the operator's
2026-11-10 logs report 210,315 ms, 304,679 checkpoints scanned, zero inserted
mappings, 121,895 existing mappings, and zero skips. It completed before the
subsequent hours of replay. This does not exclude cache or other indirect costs,
nor prove that the complete new binary has identical performance. There is no
matched-state, matched-configuration before/after production comparison.

The operator also confirmed two settings changed for this recovery:

- `propagation_delay_stop_proposal_threshold` was previously unset (default 5)
  and was set to 4,294,967,295. This effectively disables the proposal delay
  guard. It can allow proposals and subsequent consensus work during a checkpoint
  stall. At the sampled propagation delay of zero, either threshold permits
  proposals; earlier effects cannot be inferred from that sample.
- `commit_sync_batches_ahead` was increased to 20,000. With batch size 100,
  confirmed by the logged threshold of 2,000,000, this is 625 times the default
  fetch-ahead allowance of 100 times 32. The previous configured value is not
  recorded. This controls commit fetch scheduling and parallel fetching, and can
  increase buffering/resource pressure. It is not a cap on the quarantine or on
  startup replay of commits already stored locally. The samples with
  `highest_scheduled_index=0` do not demonstrate active commit-range fetching.

The supported diagnosis is an existing scaling defect under a large replay
backlog, with changed recovery settings as possible amplifiers. Attribution of
the additional slowdown to either setting or binary alone remains unproven.

## Change and invariants

When the requested checkpoint height exceeds the highest height in the queue's
last output, return false immediately. Consensus commit rounds increase; pending
checkpoint heights derive from the round, with a regular checkpoint followed by
an optional randomness checkpoint. The existing pending-checkpoint reader already
checks this ordering in debug builds.

The comparison is strictly greater: a duplicate at the highest height still uses
the original scan. Earlier heights, gaps, empty queues, and an empty final output
also retain the original behavior. No index, persistence format, consensus rule,
protocol setting, transaction identity, or checkpoint content is changed.

## Verification

Local WSLC run on 2026-11-10, Rust 1.85.1, Ubuntu 22.04, offline locked Cargo
dependencies, unoptimized test profile with debug symbols disabled:

```sh
cargo test --offline --locked -p sui-core --lib consensus_quarantine::tests -- --nocapture
cargo test --offline --locked -p sui-core --lib pending_checkpoint_lookup_benchmark -- --ignored --nocapture
cargo test --offline --locked -p sui-core --lib randomness_recovery -- --nocapture
```

- Three lookup regressions passed, covering duplicates, gaps, prefix removal,
  empty outputs, and a 10,000-output replay queue. The manual benchmark is ignored
  in the default suite and was explicitly run separately: one pass.
- All seven existing randomness recovery regressions passed. No failures.
- Source was parent `59e26ba5dc91773006a8ae41aa49af0e7dd77bed` with the changed
  `consensus_quarantine.rs` overlay, SHA-256
  `7b8307210f2794b7aab2309e923042f6f4f85919c4e356ae5ab57464d4426767`.

For 1,000 lookups above the queue's highest height, excluding queue setup:

| Queue outputs | Original linear scan | New upper-bound check |
| --- | --- | --- |
| 1,000 | 25.416 ms | 31.777 microseconds |
| 10,000 | 284.543 ms | 24.597 microseconds |
| 100,000 | 8.449 seconds | 26.944 microseconds |

These are synthetic lookup measurements, not an end-to-end replay benchmark or
a prediction of production speedup. The very short fast-path timings should not
be treated as precise ratios. Prefix removal tests model queue state changes;
they do not exercise database persistence, which this change does not modify.
Independent review found no actionable source/API defect and checked the test
evidence. CI image construction and runtime qualification remain separate.

## Operator considerations

Observe handled commits against the fixed highest observed commit at startup to
track the builder's replay gate. The live consensus tip is a different target.
Recovery is complete only when checkpoint construction, signatures accepted by
peers, certification, and execution progress, with no checkpoint fork errors.

Reducing fetch-ahead cannot erase existing stored commits or queued replay work.
Another restart can repeat replay because quarantined progress is not yet durable.
Do not recommend a restart solely to restore parameters without weighing current
replay progress. Keep the propagation override temporary and restore normal
settings in a planned, verified operation. Preserve the never-restarted healthy
signers through checkpoint recovery. This document does not authorize production
deployment, restarts, or database changes.
