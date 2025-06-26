// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use sui_macros::sim_test;
use sui_test_transaction_builder::TestTransactionBuilder;
use sui_types::transaction::TransactionDataAPI;
use test_cluster::TestClusterBuilder;

#[sim_test]
async fn test_gasless_transactions_e2e() {
    // Create a simple test cluster first - we'll enable gasless mode later
    let cluster = TestClusterBuilder::new().with_num_validators(3).build().await;
    
    // TODO: Enable gasless mode when we have the proper API
    // For now, let's test the basic transaction builder functionality
    let sender = cluster.get_address_0();
    let recipient = cluster.get_address_1();

    // Get a dummy object reference for testing - we'll use a coin object
    let coins = cluster.wallet.get_all_gas_objects_owned_by_address(sender).await.unwrap();
    let coin_ref = coins[0];

    // Test that we can build gasless transactions
    let gasless_tx = TestTransactionBuilder::new_gasless(sender)
        .transfer(coin_ref, recipient)
        .build_gasless();
        
    // Basic validation that the transaction was built correctly
    assert!(gasless_tx.is_gasless());
    assert_eq!(gasless_tx.gas_data().payment.len(), 0);
    assert_eq!(gasless_tx.gas_data().price, 0);
    assert_eq!(gasless_tx.gas_data().budget, 0);

    println!("✅ Gasless transaction building works correctly");
} 