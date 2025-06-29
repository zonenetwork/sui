# Gasless Sui Network Configuration

This document outlines how to configure and operate a Sui network in "gasless" mode. This feature is intended exclusively for private, permissioned networks where traditional public blockchain economics (gas fees for spam prevention, validator incentives, etc.) do not apply.

## 1. Overview

When enabled, the gasless feature makes the following changes to the network's behavior:

- **No Gas Fees:** All user transactions are executed without requiring any SUI for gas payments. The execution is "unmetered," meaning computational and storage costs are not charged to the user.
- **No Gas Coin Required:** Users can submit transactions without owning or specifying a gas coin.
- **Simplified UX:** This removes significant friction for users in a private network, as they no longer need to acquire SUI tokens from a faucet before interacting with the blockchain.

**Warning:** This feature completely disables the primary economic and anti-spam protections of the Sui network. It should **NEVER** be enabled on a public or permissionless network, as it would expose the network to severe denial-of-service (DoS) attacks.

## 2. How to Enable Gasless Mode

Gasless mode is controlled by a single feature flag within the protocol configuration. To enable it, you must set the `gasless_transactions` flag to `true` in your network's `genesis.toml` file.

All validators in the network must launch with this identical configuration to prevent network forks.

### Example `genesis.toml` Configuration

Add the following section to your `genesis.toml` file:

```toml
# ... other genesis config ...

# Enable the gasless feature flag for the network
[protocol_config.feature_flags]
gasless_transactions = true

# ... other flags ...
```

When this flag is active, the network will no longer require gas payments for any user-submitted transactions.

## 3. Submitting a Gasless Transaction

When gasless mode is enabled, clients and SDKs should construct transactions with the following `GasData`:

- `payment`: An empty vector `[]`.
- `owner`: The sender's `SuiAddress`.
- `price`: `0`.
- `budget`: `0`.

The `sui-types` crate provides a helper function, `TransactionData::new_gasless(...)`, to correctly construct such a transaction.

### Using the Sui CLI in Gasless Mode

The Sui CLI automatically detects when a network is in gasless mode and allows commands to execute without gas coins:

```bash
# Publish a Move package without gas coins
sui client publish

# Call Move functions without gas coins  
sui client call --package 0x123... --module counter --function increment

# Transfer objects without gas coins
sui client transfer --object-id 0x456... --to 0x789...
```

When gasless mode is enabled, you can omit the `--gas` flag from all commands. The CLI will automatically create gasless transactions.

## 4. Operational Considerations

When running a gasless network, operators must be aware of the following implications:

### A. Spam Prevention

Since gas fees are disabled, you must rely on other mechanisms for spam and DoS protection. These typically include:

- **Permissioned Access:** Controlling which users or applications can connect to the network's Full nodes and submit transactions.
- **Rate Limiting:** Implementing API-level rate limiting on Full nodes.
- **Consensus Throughput Limits:** The network's intrinsic consensus and execution throughput will serve as the ultimate backstop against transaction spam.

### B. Storage Economics

The storage fund and rebate system is effectively disabled in gasless mode. There is no economic incentive for users to delete on-chain objects to reclaim SUI. Network operators are therefore responsible for:

- **Monitoring Storage Growth:** Keep a close watch on the total on-chain data size.
- **Data Pruning/Archiving Policies:** Implement off-chain policies or custom tooling for managing storage growth over time if it becomes a concern. 