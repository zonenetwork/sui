# Zone unmetered gasless port

Status: unreleased; fresh-network functional qualification passed on 2026-11-10.
See the [qualification evidence](zone-gasless-evidence/README.md) for exact source,
artifact identities, independent execution, failed attempts and remaining boundaries.

The implementation owner is the Sui fork. The base is updated `main` at
`a4f70b9e6639587a2bac63a1684097b74bbdcefd`. The behavioral reference is the local
`C:\zone\zone.sui` checkout at `50ba71a663770e37da34234cbb36f52eb61f38e2`;
the GitHub gasless branch is not a source for this work.

## Acceptance

A newly generated private network must execute general programmable transactions
without a gas payment, gas budget, computation meter or user SUI balance. Publish,
upgrade, authorized Move calls, owned/shared object mutation, transfer, creation
and deletion must preserve permissions, events, atomic failure and asset value.
Empty-payment transactions must reject `Argument::GasCoin`. Explicit gas coins
must remain subject to ownership and coin checks even when no fee is charged.
Replay protection must remain effective; builders must produce suitable signed
expiration data. Existing upstream protocol versions must retain their behavior.

## Verification and documentation plan

The implementation owner will cover protocol snapshots, transaction validity,
input checks, execution gas/payment/storage accounting, CLI and gRPC simulation.
Checks include affected Rust tests, scoped formatting/linting, simulation tests,
and matching candidate binaries on a fresh four-validator network with a full
node. Runtime cases include zero-SUI accounts, publish/upgrade, shared and owned
objects, non-SUI coin operations, failures, invalid signatures/GasCoin, retries,
epoch changes, restart and mode-disabled regression. Actual Zone contract fixtures
will be exercised where available. Evidence will identify exact source, commands,
environment, outcomes and unproved boundaries. An independent assessor will review
the complete candidate and repeat critical checks in an isolated checkout.

The operational documentation will specify real genesis activation, matching
binaries, supported transport/transaction encodings, recovery and limitations.
Node qualification does not establish bank credit or compatibility of old
JSON-RPC consumers; those composed-system boundaries remain separately identified.
Only task-owned network processes and data may be stopped or reset.

## Behavior and activation

Build all validators, fullnodes, genesis tools and the CLI from the same revision:

```sh
cargo build --locked --release -p sui -p sui-node --features sui-protocol-config/zone-gasless
target/release/sui genesis --working-dir /path/to/NEW/network --committee-size 4
target/release/sui start --network.config /path/to/NEW/network
```

Use a new empty directory. The opt-in build supports protocol 144 and uses it for
new genesis. At 144, `Chain::Unknown` enables `gasless_transactions`; public
Mainnet/Testnet configurations do not enable it. A standard build still supports
versions 1–143. Versions 1–143 retain their exact upstream configuration snapshots.
Do not use runtime protocol overrides to activate this mode.

The optional Docker build argument is `SUI_FEATURES=sui-protocol-config/zone-gasless`
for both `docker/sui-node/Dockerfile` and `docker/sui-tools/Dockerfile`. The
`sui.features` image label records the argument. Without it the images retain
the upstream build. Container builds require separate image qualification; native
Cargo and network results do not establish a container build.

Zone mode makes every user transaction unmetered, including transactions with
explicit payment coins. Empty-payment transactions use `price=0`, `budget=0`,
`owner=sender`; they cannot reference `Argument::GasCoin`. Explicit SUI payment
coins remain subject to type, ownership, version and asset-conservation checks.
Their value can be used by PTBs, without fees. Zero-price explicit-coin submissions
use baseline fullnode submission priority, including pending-log recovery.

General PTBs, publish/upgrade and object creation are allowed. Computation has no
gas meter. The upstream public gasless transfer allowlist, 16 KiB special size
limit, minimum remaining token balance and special 300 TPS limiter do not govern
Zone mode. Normal structural limits, signature and ownership checks, bytecode
verification, atomic aborts, replay protection, overload and consensus protections
remain active. Execution limits selected by metering status, including effect-size
and written-object limits, use upstream's unmetered/system thresholds; they are not
the paid user-transaction thresholds. No sponsor account, gas faucet or user SUI
top-up is needed.

New user objects have zero storage rebate; successful and failed execution report
zero computation/storage fees. This implementation qualifies **new genesis** only.
It does not migrate an existing paid chain's nonzero storage rebates. Validator
stake, genesis token allocation and system epoch accounting are still required.
Operate the private network with authenticated ingress, authorized contract
capabilities and capacity monitoring: unmetered execution provides no economic
bound on an admitted program's computation.

Protocol 144 is a fork-specific extension. A future upstream merge that introduces
its own version 144 must reconcile the version lineage explicitly; do not silently
replace this network's existing protocol semantics.

## Client and ZonePay integration

The updated CLI queries the network feature flag and builds gasless envelopes for
publish, upgrade, calls and PTBs. Dry-run/dev-inspect use the same private execution
path. gRPC v2 unresolved-transaction simulation with `do_gas_selection=true` can
resolve general eligible PTBs without payment coins. Supplied BCS and signatures
are never rewritten by the server.

For shared-only and input-free transactions, sign `ValidDuring` with the actual
chain identifier, current epoch through current epoch + 1, and a fresh nonce.
The CLI does this automatically. Rust consumers can call
`TransactionData::new_gasless(kind, sender, expiration)`. Transaction BCS layouts
and error enums are unchanged. Owned-object references remain replay protection,
but builders should use the bounded expiration consistently.

Persist a payment's signed bytes and digest before submission. Retry those same
bytes while the result is uncertain, query the digest, and enforce a unique business
reference in the contract before rebuilding an expired transaction. A new nonce is
a new transaction; it is not business-level duplicate protection. Refresh consumed
owned-object versions. Shard shared payment state according to the application’s
concurrency requirements rather than using a single global mutable payment object.

This upstream no longer serves the old JSON-RPC transaction APIs. The local Zone
.NET SDK's JSON-RPC transport and limited expiration encoder therefore need a
separate gRPC/BCS update before the existing services can use this network unchanged.
The on-chain tests establish contract execution and balances, not bank credit,
provider idempotency, notification delivery or end-to-end ZonePay service readiness.

The current CLI uses the modern Move manifest and environment/publication records.
On a new chain, publish fresh package IDs and propagate the new capability/shared
object IDs to consumers. The smoke test copies local Zone contract **sources**
unchanged, adapts only test manifests, and records hashes; it never overwrites the
original contract checkout or reuses old `published-at` addresses.

## Reproduce qualification

Use the repository Rust toolchain and required native build dependencies. The
checked-in tests deliberately include negative cases and the paid protocol 143
regression. Simulator tests use the repository's normal cryptography.

```sh
cargo test --locked -p sui-protocol-config --features zone-gasless
cargo test --locked -p sui-types --test zone_gasless --features sui-protocol-config/zone-gasless
bash scripts/simtest/cargo-simtest simtest -p sui-e2e-tests --test zone_gasless_tests \
  --features zone-gasless --test-threads 1
python3 scripts/zone-gasless-smoke.py --sui target/release/sui \
  --work-dir /tmp/zone-gasless-UNIQUE --rpc-port 18945 \
  --zone-contracts /path/to/local/zone.smartcontracts.sui
```

The optional last argument adds real local Zone package publication, funding,
duplicate-funding abort, liquidation and balance transfer. The standalone harness
creates four validators and a fullnode, new zero-SUI users, crosses two epochs,
stops and restarts its own network with persisted data, checks the state and writes
`report.json`. It stops only processes it started and retains its test directory.
Keep generated keystores private; share the sanitized report rather than the
network directory. This is functional qualification, not a production throughput
or availability benchmark.

The e2e crate's `zone-gasless` feature propagates the protocol feature and enables
the private-network test. Without it the same test target runs only the paid
protocol 143 regression, so the normal upstream test command remains valid.

## Evidence record

The authoring checks run in Ubuntu 24.04 under WSL on 2026-11-10. Rust 1.98.1
matches the updated repository. Debug binaries are used for functional checks.

- Protocol tests: 19 passed in each of the Zone and standard builds; 2 documentation
  examples ignored upstream in each run. Only the three new protocol 144 snapshots
  were added; earlier snapshots did not change.
- Focused gasless transaction tests: 5 passed.
- Four-validator simulator suite: 2 passed, including paid protocol 143 behavior,
  explicit zero-price coins and gRPC resolution/BCS regression cases. Independent
  execution with another simulator seed passed. The standard build ran its single
  paid protocol 143 regression successfully with the private test feature disabled.
- Standalone fresh network: zero-SUI publish, compute-heavy payment, dry-run,
  duplicate-reference abort, signed replay, create/mutate/transfer/delete, upgrade,
  two epoch transitions and persisted restart passed.
- Local Zone Move sources: all four packages published; funding, duplicate-reference
  abort, liquidation and transfer of the exact remaining balance passed with zero fees.
- Scoped implementation/test Clippy and formatting passed. Independent review found
  no unresolved product findings after the fixes and repeated runtime checks.
- The untouched local reference passed core gasless cases; its post-restart
  transaction timed out. This baseline limitation is retained in the evidence;
  the candidate's restart checks passed in author and independent runs.
- There is no CI, deployment or image-build claim in this record.
