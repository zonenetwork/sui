# Randomness mapping recovery after validator restart

Status: candidate in preparation; not qualified for deployment.

## Objective and ownership

Restore checkpoint building after a mid-epoch restart when randomness transactions
were executed from certified checkpoints without recording their round-to-digest
mapping. Base: Bitbucket `zonenetwork/zone.sui` master
`ec2e383d6427c85ec5e2ec32e580fe5bd95a1030`. The affected core source matches the
historical incident revision `50ba71a663770e37da34234cbb36f52eb61f38e2`.

Implementation and outcome evidence: primary agent. Independent diagnosis and
candidate assessment: separate assessor, who does not author the implementation.

## Acceptance and verification plan

- Reproduce a missing randomness key after checkpoint-supplied execution.
- Record the mapping when execution commits (the narrow fix from upstream
  `aaa6edef4f1eb714af5d6301cdc267de1f55a533`).
- Recover missing current-epoch mappings from already executed certified
  checkpoints at startup, without synthesizing transactions or changing contents.
- Test waiter wakeup, persistence, repeat recovery, existing/conflicting entries,
  absent/pruned data, epoch boundaries, and the captured execution watermark.
- Verify reconstructed checkpoint contents and summary digests remain identical.
- Qualify restart/signing recovery, healthy-validator behavior, and epoch change
  on an authorized development network; local tests alone do not establish this.
- Measure recovery duration before estimating production scan time.
- Obtain independent assessment of the complete candidate in a fresh checkout.

The existing schema, protocol version, consensus output, transaction construction,
checkpoint contents and beacon completion rules remain the contracts to preserve.
The broader scheduler rewrite in upstream `f0699cb902` is outside this patch.

## Evidence boundaries

The incident handoffs are supplied observations, not independently inspected
production evidence or authorization for node operations. Production image/source
parity, the exact missing production key, development-network reproduction,
production recovery and timing are unverified. Production operations belong to
the human operators. No action on production validators is part of this work.

## Source assumption check

References below use the unmodified master base, so line numbers remain stable
even where this patch inserts code. "Confirmed" concerns source behavior, not
the supplied live incident observations.

| ID | Finding | Base source evidence |
| --- | --- | --- |
| A1 | Confirmed: only Digest and RandomnessRound keys exist. | `sui-types/src/transaction.rs:3685` |
| A2 | Confirmed: only the beacon receiver inserts mappings; no table-specific delete/clear found. Epoch stores are separate databases. | `authority.rs:5638`; `authority/authority_per_epoch_store.rs:1623,1650,2456,844,1267` |
| A3 | Confirmed: consensus reserves a round and constructs randomness checkpoint roots. | `authority/authority_per_epoch_store.rs:3110,3310,3336,4326` |
| A4 | Confirmed: persisted consensus progress can trail execution; quarantine flush needs locally built summaries below the executed watermark. Actual incident indices remain unverified. | `consensus_handler.rs:507`; `authority/consensus_quarantine.rs:567-650` |
| A5 | Confirmed: builder resumes from its own persisted/quarantined height and summary. | `checkpoints/mod.rs:1090,1632`; `authority/authority_per_epoch_store.rs:4421` |
| A6 | Confirmed in source: old checkpoints are reconstructed using builder inclusion state; local and certified summary comparisons exist. Actual rebuilt digest equality remains untested. | `checkpoints/mod.rs:1151,1359,1406,1842`; `checkpoints/checkpoint_output.rs:92` |
| A7 | Confirmed: checkpoint executor has transaction digests but does not insert keys before notifying randomness completion. | `checkpoints/checkpoint_executor/mod.rs:373,670,963` |
| A8 | Confirmed: one waiting task per enqueue with a nonempty non-transaction group, not necessarily one per missing round. The incident task-count arithmetic is an inference. | `execution_scheduler/execution_scheduler_impl.rs:214-251` |
| A9 | Confirmed default: max-partial-sigs-concurrent-sends is 20; pending rounds counts requested minus completed. Live configuration is unknown. | `sui-config/src/p2p.rs:350,400`; `sui-network/src/randomness/mod.rs:846,989` |
| A10 | Confirmed mechanism: startup reloads completed/next rounds and requests only incomplete ones. Next round and consensus progress share a persistence batch. Affected production DB is uninspected. | `epoch/randomness.rs:281,362,371`; `authority/consensus_quarantine.rs:276-292` |
| A11 | Confirmed: backpressure waits occur in consensus handling, and are suppressed when certified progress does not exceed executed progress. No direct builder/signature wait. | `consensus_handler.rs:569,1280`; `authority/backpressure.rs:23,130` |
| A12 | Confirmed defaults: RocksDB default write options enable WAL but do not fsync each insertion. Live overrides/build are unknown; this does not prove a lost write. | `typed-store/src/rocks/mod.rs:289,1398`; RocksDB 0.22 default write options |
| A13 | Confirmed conditional identity: the actual certified transaction supplies its key and digest. Round alone is insufficient; randomness bytes and initial shared version also affect identity. Live digest pairs remain unverified. | `sui-types/src/transaction.rs:284,2723,2922` |
| A14 | Confirmed: the events migration scans all executed effects on each startup and skips already migrated entries; no global completion marker. Timing is unknown and migration is outside this fix. | `authority/authority_store_migrations.rs:11`; `authority.rs:3171` |
| A15 | Confirmed: upstream #22564 supplies the execution-commit insertion. #27840 requires newer scheduling architecture. Compilation remains pending. | Upstream `aaa6edef4f1eb714af5d6301cdc267de1f55a533` and `f0699cb902205bb12e28ed3e1f43d86616723c27` |
| A16 | Unknown: development/production image equality and exact binary/source provenance were not inspected. | Operator/build evidence required |
| A17 | Confirmed: the two gasless commits leave the relevant consensus, checkpoint and randomness source unchanged; subsequent master changes are packaging in this comparison. | Diff `devnet-v1.51.1..50ba71a`; diff `50ba71a..ec2e383d` |

Source paths in this table are below `crates/`; the unprefixed core paths are
below `crates/sui-core/src/`.

## Root cause supported by source

Certified checkpoints can deliver and execute a randomness transaction before
the local beacon delivers its round. The old execution path does not record the
round-to-digest mapping, while checkpoint completion independently persists the
completed-round watermark. Restart replay can therefore recreate a randomness
root that waits for a missing mapping, and the beacon no longer generates that
completed round. This explains a missing mapping without requiring disk loss.
The exact missing key on the affected production validator is still unobserved.

## Candidate changes

- `authority.rs`: backport the four execution-commit insertion lines from #22564.
- `authority_per_epoch_store.rs`: propagate lookup database errors to recovery
  instead of panicking inside the existing lookup helper.
- `checkpoints/mod.rs`: run recovery at the start of the existing builder task,
  before waiting for consensus replay. Its existing task owner handles shutdown.
- `checkpoints/randomness_recovery.rs`: capture the current epoch and synced/
  executed upper bounds, scan certified summaries, skip summaries with no randomness,
  validate content/transaction identity, preserve existing mappings, and insert
  missing mappings using the existing persistent writer and waiter notification.
  Missing/pruned/mismatched entries are counted and warned about. Every checkpoint
  iteration yields; the scan uses synchronous RocksDB calls and is not a claim of
  nonblocking I/O.
- Persisted outputs can precede the executed checkpoint watermark. For the synced
  range above that marker, recover only transactions with local executed effects
  matching the certified checkpoint. Run this before builder replay, because node
  startup waits for the builder before starting the checkpoint executor.
- `checkpoints/checkpoint_executor/mod.rs`: also repair keys in the already-executed
  transaction path, covering execution that advances beyond the startup snapshot.

## Verification status

- Independent source diagnosis: supports the narrow prevention/recovery design;
  does not establish deployment readiness.
- Rust parsing/formatting and patch whitespace: being checked locally.
- Rust compilation/test execution: blocked locally before compiling sui-core.
  Rust 1.85 and cached dependencies are present, but Windows lacks `link.exe` and
  the C++ build tools. Local Ubuntu lacks Rust/Clang/CMake; dependency downloads
  failed. No test is reported passed on that basis.
- Regression tests authored: real authority execution inserts the mapping;
  absent mapping parks a waiter and recovery wakes it; repeated recovery inserts
  nothing; conflicting mappings and the execution watermark are preserved;
  missing transactions, pruned contents, foreign-epoch transactions and wrong
  transaction digests are handled without replacing the mapping; committed effects
  ahead of the checkpoint watermark are recovered before replay and on executor retry.
- Live unpatched restart reproduction, patched restart and halted-network
  recovery, rebuilt checkpoint digest equality, healthy-node epoch transition,
  recovery timing and independent candidate assessment: pending.

## Qualification and operator considerations

The recovery scan starts before the builder's consensus-replay wait and snapshots
the highest synced/executed checkpoints, capped at the current epoch boundary.
Unexecuted synced transactions are not recovered. New executions are covered by
the execution-commit insertion and the already-executed path. Scan completion logs `scanned`, `inserted`, `existing`,
`skipped` and `elapsed_ms`; a nonzero skipped count requires inspection and must
not be described as successful recovery. A conflicting existing mapping is
preserved and reported, never silently replaced.

The existing checkpoint-service startup wait is 120 seconds. Scan time adds to
replay/rebuild time within that existing wait, so qualification must measure both
and observe actual builder progress. Do not extrapolate a production duration
until a representative scan has run. Missing/pruned required transactions can
leave roots unresolved and require a separate recovery plan.

Before any deployment, qualify the exact candidate/image on an authorized dev
network and compare rebuilt digests against certified checkpoints. Use a new
image tag with immutable source/image provenance. The human production operators
must preserve the never-restarted validators through certification recovery and
the epoch change. No production restart, database edit or deployment is authorized
by this document. Reverting the binary does not undo mappings already written;
rollback must preserve the database and use observed recovery state, rather than
promise restoration of the exact pre-patch state.
