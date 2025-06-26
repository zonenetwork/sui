### PHASE 3 – REVISED TEST STRATEGY & PLAN
**(Role: Gemini Pro 2 / Quality-Lead)**

This revised plan integrates the critical feedback from OpenAI O3 and Claude Sonnet 4, focusing on multi-validator consensus, mixed workloads, and explicit validation of transaction effects and simulation paths.

#### **1. Test Strategy Overview (Unchanged)**

My strategy remains centered on three pillars, but with a deeper scope:

1.  **Integration & E2E Testing:** Expanded to cover multi-validator clusters and critical system paths like `dev-inspect` and epoch changes.
2.  **Performance Smoke Testing:** Updated with a stricter acceptance threshold to catch any potential regressions.
3.  **Unit Test Coverage Review:** The initial assessment that unit test coverage is sufficient remains correct. My focus will be on the higher-level integration tests.

#### **2. Tier 1: Critical Integration & E2E Scenarios (Expanded)**

I will execute these tests in `sui-e2e-tests` using a `TestCluster` to properly simulate a decentralized network environment.

*   **Test Case 1: Multi-Validator Consensus & Mixed Workload (Addresses O3's #1, #2)**
    *   **Setup:** Initialize a **3-validator `TestCluster`** with the `gasless_transactions = true` protocol flag.
    *   **Action:**
        1.  Execute a gasless **owned-object transfer**.
        2.  Execute a gasless **shared-object mutation** (e.g., the standard shared counter).
    *   **Assert:**
        *   Both transactions achieve consensus successfully across all validators.
        *   The final state of the owned and shared objects is correct.
        *   The transaction effects are sane, as detailed below.

*   **Test Case 2: Transaction Effects Sanity Checks (Addresses O3's #4, #7)**
    *   **Setup:** Use the results from Test Case 1.
    *   **Action:** Inspect the `TransactionEffects` of the successful gasless transactions.
    *   **Assert:**
        *   `effects.gas_cost_summary` is `GasCostSummary { computation_cost: 0, storage_cost: 0, storage_rebate: 0, non_refundable_storage_fee: 0 }`.
        *   `effects.gas_object` is `None`. This is a critical check to ensure no gas coin is accidentally created or referenced.

*   **Test Case 3: Dev-Inspect & Simulation Path (Addresses O3's #3)**
    *   **Setup:** Use the live `TestCluster`.
    *   **Action:**
        1.  Use the `dev-inspect` RPC endpoint for a valid gasless transaction.
        2.  Use `dev-inspect` for a gasless transaction that incorrectly uses `Argument::GasCoin`.
    *   **Assert:**
        *   The valid inspection succeeds and returns the expected outcome without gas errors.
        *   The invalid inspection fails at the validation stage with `UserInputError::InvalidGasCoinUsageInGaslessTransaction`.

#### **3. Tier 2: System-Level & Regression Scenarios**

These tests ensure stability during normal network operations and confirm there are no regressions.

*   **Test Case 4: Epoch Boundary Transition (Addresses O3's #6)**
    *   **Setup:** Use the live `TestCluster`.
    *   **Action:** Force the cluster to advance to the next epoch.
    *   **Assert:**
        *   The `ChangeEpoch` transaction executes successfully.
        *   The cluster remains healthy and continues to process gasless transactions in the new epoch.

*   **Test Case 5: Full Regression on Standard Network (My Original Plan)**
    *   **Setup:** Initialize a `TestCluster` with `gasless_transactions = false`.
    *   **Action:** Execute a standard transaction that pays for gas.
    *   **Assert:**
        *   The transaction succeeds.
        *   Gas is correctly deducted from the gas coin, and the `gas_used` in the effects is greater than zero. This confirms we have not broken the default behavior of the network.

#### **4. Tier 3: Performance Smoke Testing (Revised)**

I will use the `sui-benchmark` crate with a stricter, more realistic performance target.

*   **Benchmark Workload:** High-throughput shared-counter workload.
*   **Methodology:**
    1.  Establish a baseline TPS by running the benchmark on a standard network (`gasless_transactions = false`).
    2.  Run the identical benchmark on a gasless network (`gasless_transactions = true`).
*   **Revised Acceptance Criteria (Addresses O3's #5):**
    *   The TPS of the gasless run must be **≥ 97%** of the standard network's baseline. Since the gasless path is computationally cheaper, any performance degradation greater than a minor 3% fluctuation is a critical regression that must be investigated. 