# Zone gasless qualification evidence

Date: 2026-11-10. Base: `a4f70b9e6639587a2bac63a1684097b74bbdcefd`.
Implementation: `60dd91a83da976f8c86e527591503f8b3b897772`.
Final smoke harness and limit documentation: `4c81ede3d94794094174b5008d8759b3228c94df`.
Local behavioral reference: `50ba71a663770e37da34234cbb36f52eb61f38e2` from
`C:\zone\zone.sui`. The remote gasless branch was not used.

The Linux build mirror starts at the base revision with the candidate changes
overlaid. [Source hashes](source-hashes.json) verify the changed Rust, Move,
protocol snapshots and build configuration against the committed authoring tree,
normalizing only line endings. Its CLI version string therefore includes the base
revision and `dirty`; the tested binary is identified by SHA256 below. These are
debug functional-test artifacts, not published release images.

| Check | Result | Evidence |
| --- | --- | --- |
| Zone protocol configuration | 19 tests passed; 2 upstream doctests ignored | Historical snapshots unchanged; three new 144 snapshots |
| Gasless transaction unit tests | 5 passed | Canonical envelope, unmetered status, GasCoin rejection, replay shape, version isolation |
| Zone simulator, seed 1 | 2 passed, 0 skipped | Private network and paid protocol 143; includes explicit-coin gRPC selection on/off and exact BCS preservation |
| Independent private simulator, seed 2 | 1 passed, 0 failed | Same test binary hash; normal cryptographic verification |
| Author standalone fresh network | Passed | [Native report](native-network.json) |
| Independent standalone fresh network | 25 checks passed | [Independent report](independent-network.json) |
| Scoped Rust lint and formatting | Passed | `cargo xclippy`, affected-crate rerun, e2e-specific Clippy and scoped `rustfmt --check` |
| Standard feature-disabled simulator | 1 passed, 0 skipped | Only the paid protocol 143 test runs |
| Standard protocol configuration | 19 passed; 2 upstream doctests ignored | Zone feature disabled |

[Collected check summaries](checks.txt) retain the actual counts and local log names.
The [final independent assessment](independent-review.md) closes the review findings
and supports functional use on a newly generated private network.
The [independent source comparison](independent-source-hashes.json) separately
confirms that the reviewed product/test/Docker files match the tested Linux source
after line-ending normalization. All 18 local reference feature files were rehashed
after testing and remained unchanged.

Both standalone suites use four validators and a fullnode, entirely new genesis,
new zero-SUI users and no faucet or sponsor. They exercise package publication and
upgrade; compute-heavy Move calls; dry-run without committed changes; duplicate
reference aborts; signed retry; object creation, mutation, transfer and deletion;
epoch changes; persisted process restart; and explicit gas coins at zero price.
All reported gas fields are zero and the sender remains without SUI.

The simulator additionally exercises invalid ownership and signatures, absent
GasCoin rejection, stale-version rejection on submission, actual failed execution
with rollback, expired/shared-only replay protection, non-SUI split/merge,
24 concurrent transactions and validator restart. This is functional evidence,
not a throughput claim.

Actual local Zone contract Move sources were copied unchanged from
`zone.smartcontracts.sui` and hashed in the reports. Only the copied manifests
were adapted for the new package manager and genesis. Common, coins, registry,
and funding/liquidation packages published successfully. Funding 1,000 units,
rejecting the duplicate funding reference, liquidating 500 units, and transferring
the remaining 500 units preserved exact balances with zero gas fees.

## Artifact identities

- Node/CLI build used for final native runs: Rust 1.98.1, Ubuntu 24.04 under WSL,
  `cargo build --locked -p sui -p sui-node --features sui-protocol-config/zone-gasless`.
- CLI SHA256: `54f4d7dfa52580276bb1866f5826831915dab7d491dfbc2318b5e3434450cfe9`.
- Simulator SHA256: `d46e531e1c247a19636cb361c275906bf05ce77fedeac08c7f9c6f04440c2422`.
- The independent reviewer used a clean separate checkout and independently ran
  the hash-verified artifacts. An independent full Rust rebuild is not claimed.

## Reference comparison and failed attempts

The untouched local reference was built with its pinned Rust 1.85 toolchain. Its
fresh-network publish, compute-heavy payment, duplicate-reference abort, atomic
abort and epoch transaction all passed with zero fees. Its persisted counter state
was recovered after restart, but a subsequent transaction timed out before finality.
This happened with a distinct transaction as well as a retry. The cause was not
established; [the reference report](reference-network.json) correctly remains
`passed: false`. It is not presented as a full baseline pass or a candidate failure.
The candidate's restart transactions passed in both author and independent runs.

Earlier candidate genesis failed when upstream's mandatory `enable_gasless` flag
was disabled. The final implementation keeps that flag and gives the private mode
precedence in the relevant checks. A stale-object test was corrected to submit to
the network rather than use simulation, which supports historical object reads.
Independent review found and resolved the private gRPC estimation and default-test
feature-gating issues. A harness environment-registration race was reproduced and
fixed with explicit endpoint and readiness checks; its failing run is retained in
the local work log. No failed run is counted as a pass here.

## Boundaries

No deployment, CI result, container build, separate-host resilience, sustained-load
qualification or paid-chain storage-rebate migration is claimed. Existing .NET
JSON-RPC consumers still require the documented transport/expiration update.
Contract execution does not establish bank-credit delivery. Generated private keys
and network configuration are excluded from these committed reports.
