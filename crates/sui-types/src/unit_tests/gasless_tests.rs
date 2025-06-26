// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    base_types::{dbg_addr, ObjectDigest, ObjectID, SequenceNumber},
    transaction::{
        Argument, Command, GasData, ProgrammableTransaction, TransactionData,
        TransactionDataAPI, TransactionDataV1, TransactionExpiration, TransactionKind,
    },
};
use sui_protocol_config::ProtocolConfig;

/// Test that gasless transactions can be created and validated correctly
#[test]
fn gasless_transaction_creation_and_validation() {
    let sender = dbg_addr(1);
    
    // Create a gasless transaction with empty gas payment
    let gasless_tx = TransactionData::V1(TransactionDataV1 {
        kind: TransactionKind::ProgrammableTransaction(ProgrammableTransaction {
            inputs: vec![],
            commands: vec![Command::TransferObjects(
                vec![Argument::Input(0)],
                Argument::Input(1),
            )],
        }),
        sender,
        gas_data: GasData {
            payment: vec![], // Empty gas payment for gasless
            owner: sender,
            price: 0,
            budget: 0,
        },
        expiration: TransactionExpiration::None,
    });

    // Test with gasless config enabled - should pass validation
    let mut config = ProtocolConfig::get_for_max_version_UNSAFE();
    config.set_gasless_transactions_for_testing(true);
    assert!(gasless_tx.validity_check(&config).is_ok());

    // Test with gasless config disabled (default) - should fail validation
    let config_normal = ProtocolConfig::get_for_max_version_UNSAFE();
    assert!(gasless_tx.validity_check(&config_normal).is_err());
}

/// Test that regular transactions still work when gasless mode is enabled
#[test]
fn regular_transactions_work_with_gasless_enabled() {
    let sender = dbg_addr(1);
    
    // Create a regular transaction with gas payment
    let regular_tx = TransactionData::V1(TransactionDataV1 {
        kind: TransactionKind::ProgrammableTransaction(ProgrammableTransaction {
            inputs: vec![],
            commands: vec![Command::TransferObjects(
                vec![Argument::Input(0)],
                Argument::Input(1),
            )],
        }),
        sender,
        gas_data: GasData {
            payment: vec![(ObjectID::random(), SequenceNumber::from_u64(1), ObjectDigest::random())],
            owner: sender,
            price: 1000,
            budget: 1000000,
        },
        expiration: TransactionExpiration::None,
    });

    // Test with gasless config enabled - regular transactions should still work
    let mut config = ProtocolConfig::get_for_max_version_UNSAFE();
    config.set_gasless_transactions_for_testing(true);
    assert!(regular_tx.validity_check(&config).is_ok());
}

/// Test gasless flag defaults to false and can be toggled
#[test]
fn gasless_flag_defaults_to_false() {
    let config = ProtocolConfig::get_for_max_version_UNSAFE();
    assert!(!config.gasless_transactions(), "Gasless transactions should default to false");
}

/// Test setting gasless flag for testing
#[test]
fn gasless_flag_can_be_set_for_testing() {
    let mut config = ProtocolConfig::get_for_max_version_UNSAFE();
    
    // Initially false
    assert!(!config.gasless_transactions());
    
    // Can be enabled
    config.set_gasless_transactions_for_testing(true);
    assert!(config.gasless_transactions());
    
    // Can be disabled again
    config.set_gasless_transactions_for_testing(false);
    assert!(!config.gasless_transactions());
} 