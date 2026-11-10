// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use sui_protocol_config::{Chain, ProtocolConfig, ProtocolVersion};
use sui_types::base_types::{ObjectID, SequenceNumber, SuiAddress};
use sui_types::digests::{ChainIdentifier, ObjectDigest};
use sui_types::gas::{GasCostSummary, SuiGasStatus, SuiGasStatusAPI};
use sui_types::programmable_transaction_builder::ProgrammableTransactionBuilder;
use sui_types::transaction::{
    Argument, Command, ObjectArg, ObjectReadResult, TransactionData, TransactionDataAPI,
    TransactionExpiration, TransactionKind, TxValidityCheckContext,
};

fn zone_config() -> ProtocolConfig {
    let mut config = ProtocolConfig::get_for_version(ProtocolVersion::new(143), Chain::Unknown);
    config.set_gasless_transactions_for_testing(true);
    config
}

fn expiration() -> TransactionExpiration {
    TransactionExpiration::ValidDuring {
        min_epoch: Some(0),
        max_epoch: Some(1),
        min_timestamp: None,
        max_timestamp: None,
        chain: ChainIdentifier::default(),
        nonce: 42,
    }
}

fn transfer() -> TransactionData {
    let sender = SuiAddress::random_for_testing_only();
    let mut pt = ProgrammableTransactionBuilder::new();
    let object = pt
        .obj(ObjectArg::ImmOrOwnedObject((
            ObjectID::random(),
            SequenceNumber::from_u64(1),
            ObjectDigest::MIN,
        )))
        .unwrap();
    pt.transfer_arg(SuiAddress::random_for_testing_only(), object);
    TransactionData::new_gasless(
        TransactionKind::ProgrammableTransaction(pt.finish()),
        sender,
        expiration(),
    )
}

#[test]
fn general_object_transfer_is_valid_without_payment() {
    let tx = transfer();
    assert!(tx.gas().is_empty());
    assert_eq!(tx.gas_price(), 0);
    assert_eq!(tx.gas_budget(), 0);
    assert_eq!(tx.gas_owner(), tx.sender());
    tx.validity_check(&TxValidityCheckContext::from_cfg_for_testing(&zone_config()))
        .unwrap();

    let upstream = ProtocolConfig::get_for_version(ProtocolVersion::new(143), Chain::Unknown);
    assert!(
        tx.validity_check(&TxValidityCheckContext::from_cfg_for_testing(&upstream))
            .is_err()
    );
}

#[test]
fn private_execution_has_no_computation_budget() {
    let config = zone_config();
    let mut tx = transfer();
    for (price, budget) in [(0, 0), (1000, 10_000_000)] {
        tx.gas_data_mut().price = price;
        tx.gas_data_mut().budget = budget;
        let mut status = SuiGasStatus::for_execution(tx.gas_data(), tx.kind(), 1000, &config);
        assert!(status.is_unmetered());
        assert_eq!(status.gas_budget(), 0);
        status.charge_publish_package(10_000_000).unwrap();
        assert_eq!(status.summary(), GasCostSummary::default());
    }
}

#[test]
fn absent_gas_coin_is_rejected_even_when_unmetered() {
    let sender = SuiAddress::random_for_testing_only();
    let mut pt = ProgrammableTransactionBuilder::new();
    let amount = pt.pure(1u64).unwrap();
    pt.command(Command::SplitCoins(Argument::GasCoin, vec![amount]));
    let tx = TransactionData::new_gasless(
        TransactionKind::ProgrammableTransaction(pt.finish()),
        sender,
        expiration(),
    );
    let error = tx
        .validity_check(&TxValidityCheckContext::from_cfg_for_testing(&zone_config()))
        .unwrap_err();
    assert!(error.to_string().contains("Argument::GasCoin"), "{error}");
}

#[test]
fn no_payment_shape_is_unambiguous_and_replay_protected() {
    let config = zone_config();
    let mut tx = transfer();
    assert!(tx.has_replay_protection(std::iter::empty::<ObjectReadResult>()));
    *tx.expiration_mut() = TransactionExpiration::None;
    assert!(!tx.has_replay_protection(std::iter::empty::<ObjectReadResult>()));
    tx.gas_data_mut().budget = 1;
    assert!(
        tx.validity_check(&TxValidityCheckContext::from_cfg_for_testing(&config))
            .is_err()
    );
    tx.gas_data_mut().budget = 0;
    tx.gas_data_mut().price = 1;
    assert!(
        tx.validity_check(&TxValidityCheckContext::from_cfg_for_testing(&config))
            .is_err()
    );
}

#[test]
fn existing_versions_and_public_chains_keep_upstream_policy() {
    for chain in [Chain::Unknown, Chain::Mainnet, Chain::Testnet] {
        for version in [87, 142, 143] {
            let config = ProtocolConfig::get_for_version(ProtocolVersion::new(version), chain);
            assert!(!config.gasless_transactions());
        }
    }
    if let Some(zone) =
        ProtocolConfig::get_for_version_if_supported(ProtocolVersion::new(144), Chain::Unknown)
    {
        assert!(zone.gasless_transactions());
        assert!(zone.enable_gasless());
        assert!(zone.gasless_verify_remaining_balance());
        assert_eq!(zone.gasless_max_tps_as_option(), None);
        for chain in [Chain::Mainnet, Chain::Testnet] {
            let public = ProtocolConfig::get_for_version(ProtocolVersion::new(144), chain);
            assert!(!public.gasless_transactions());
            assert!(public.enable_gasless());
        }
    }
}
