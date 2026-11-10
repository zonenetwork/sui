// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use move_core_types::ident_str;
use serde::Deserialize;
use std::path::PathBuf;
use sui_macros::sim_test;
use sui_move_build::BuildConfig;
use sui_protocol_config::{Chain, ProtocolConfig, ProtocolVersion};
use sui_rpc_api::client::ExecutedTransaction;
use sui_swarm_config::genesis_config::AccountConfig;
use sui_types::SUI_FRAMEWORK_PACKAGE_ID;
use sui_types::base_types::{ObjectID, SuiAddress};
use sui_types::effects::TransactionEffectsAPI;
use sui_types::gas::GasCostSummary;
use sui_types::move_package::UpgradePolicy;
use sui_types::object::{Object, Owner};
use sui_types::programmable_transaction_builder::ProgrammableTransactionBuilder;
use sui_types::transaction::{
    Argument, Command, ObjectArg, SharedObjectMutability, TransactionData, TransactionDataAPI,
    TransactionExpiration, TransactionKind,
};
use test_cluster::TestCluster;
use test_cluster::addr_balance_test_env::TestEnvBuilder;

#[derive(Deserialize)]
struct CounterContents {
    _id: ObjectID,
    value: u64,
    references: Vec<u64>,
}

async fn tx(
    cluster: &TestCluster,
    sender: SuiAddress,
    pt: ProgrammableTransactionBuilder,
) -> TransactionData {
    let epoch = cluster
        .wallet
        .grpc_client()
        .unwrap()
        .get_latest_checkpoint()
        .await
        .unwrap()
        .epoch;
    TransactionData::new_gasless(
        TransactionKind::ProgrammableTransaction(pt.finish()),
        sender,
        TransactionExpiration::ValidDuring {
            min_epoch: Some(epoch),
            max_epoch: Some(epoch + 1),
            min_timestamp: None,
            max_timestamp: None,
            chain: cluster.get_chain_identifier(),
            nonce: rand::random(),
        },
    )
}

async fn execute(cluster: &TestCluster, data: &TransactionData) -> ExecutedTransaction {
    let result = cluster.sign_and_execute_transaction(data).await;
    assert_eq!(
        result.effects.gas_cost_summary(),
        &GasCostSummary::default()
    );
    if data.gas().is_empty() {
        assert!(result.effects.gas_object().is_none());
    }
    result
}

async fn created(cluster: &TestCluster, result: &ExecutedTransaction, suffix: &str) -> Object {
    for (reference, _) in result.effects.created() {
        let object = cluster
            .get_object_from_fullnode_store(&reference.0)
            .await
            .unwrap();
        if object
            .type_()
            .is_some_and(|ty| ty.to_string().contains(suffix))
        {
            return object;
        }
    }
    panic!("Missing created object {suffix}");
}

async fn owned(cluster: &TestCluster, id: ObjectID) -> ObjectArg {
    ObjectArg::ImmOrOwnedObject(
        cluster
            .get_object_from_fullnode_store(&id)
            .await
            .unwrap()
            .compute_object_reference(),
    )
}

fn shared(object: &Object) -> ObjectArg {
    let Owner::Shared {
        initial_shared_version,
    } = object.owner
    else {
        panic!("expected shared object")
    };
    ObjectArg::SharedObject {
        id: object.id(),
        initial_shared_version,
        mutability: SharedObjectMutability::Mutable,
    }
}

async fn counter(cluster: &TestCluster, id: ObjectID) -> CounterContents {
    let object = cluster.get_object_from_fullnode_store(&id).await.unwrap();
    bcs::from_bytes(object.data.try_as_move().unwrap().contents()).unwrap()
}

#[sim_test]
async fn zone_unmetered_fresh_network() {
    let config = ProtocolConfig::get_for_version(ProtocolVersion::new(144), Chain::Unknown);
    assert!(
        config.gasless_transactions(),
        "build with sui-protocol-config/zone-gasless"
    );
    let env = TestEnvBuilder::new()
        .with_num_validators(4)
        .with_protocol_version(144.into())
        .with_test_cluster_builder_cb(Box::new(|builder| {
            builder.with_accounts(vec![
                AccountConfig {
                    address: None,
                    gas_amounts: vec![],
                },
                AccountConfig {
                    address: None,
                    gas_amounts: vec![],
                },
                AccountConfig {
                    address: None,
                    gas_amounts: vec![1000],
                },
            ])
        }))
        .build()
        .await;
    let cluster = &env.cluster;
    let empty: Vec<_> = env
        .gas_objects
        .iter()
        .filter(|(_, gas)| gas.is_empty())
        .map(|(address, _)| *address)
        .collect();
    let (sender, recipient) = (empty[0], empty[1]);
    assert_eq!(env.get_sui_balance_ab(sender), 0);
    assert_eq!(env.get_sui_balance_ab(recipient), 0);

    let package = BuildConfig::new_for_testing()
        .build_async(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/zone_gasless"))
        .await
        .unwrap();
    let mut pt = ProgrammableTransactionBuilder::new();
    let cap = pt.publish_upgradeable(
        package.get_package_bytes(false),
        package.get_dependency_storage_package_ids(),
    );
    pt.transfer_arg(sender, cap);
    let publish_tx = tx(cluster, sender, pt).await;
    let mut unresolved = sui_rpc::proto::sui::rpc::v2::Transaction::from(publish_tx.clone());
    unresolved.bcs = None;
    unresolved.digest = None;
    unresolved.expiration = None;
    unresolved.gas_payment_mut().price = None;
    unresolved.gas_payment_mut().budget = None;
    let mut rpc = sui_rpc::proto::sui::rpc::v2::transaction_execution_service_client::TransactionExecutionServiceClient::connect(cluster.rpc_url().to_owned()).await.unwrap();
    let resolved = rpc
        .simulate_transaction(
            sui_rpc::proto::sui::rpc::v2::SimulateTransactionRequest::default()
                .with_transaction(unresolved)
                .with_do_gas_selection(true),
        )
        .await
        .unwrap()
        .into_inner();
    let resolved_tx = resolved.transaction().transaction();
    assert_eq!(resolved_tx.gas_payment().price(), 0);
    assert_eq!(resolved_tx.gas_payment().budget(), 0);
    assert!(resolved_tx.gas_payment().objects.is_empty());
    let published = execute(cluster, &publish_tx).await;
    let package_id = published.get_new_package_obj().unwrap().0;
    let upgrade_cap = published.get_new_package_upgrade_cap().unwrap();
    let shared_counter = created(cluster, &published, "::workload::Counter").await;
    let admin = created(cluster, &published, "::workload::Admin").await;
    let treasury = created(cluster, &published, "::coin::TreasuryCap<").await;
    let mut invalid_payment_pt = ProgrammableTransactionBuilder::new();
    let value = invalid_payment_pt.pure(0u64).unwrap();
    invalid_payment_pt.programmable_move_call(
        package_id,
        ident_str!("workload").to_owned(),
        ident_str!("create").to_owned(),
        vec![],
        vec![value],
    );
    let invalid_payment = TransactionData::new_programmable(
        sender,
        vec![admin.compute_object_reference()],
        invalid_payment_pt.finish(),
        0,
        0,
    );
    assert!(
        cluster
            .wallet
            .grpc_client()
            .unwrap()
            .simulate_transaction(&invalid_payment, true, false)
            .await
            .is_err()
    );

    let mut pt = ProgrammableTransactionBuilder::new();
    for value in [1u8, 2, 3] {
        let bytes = pt.pure(vec![value; 8000]).unwrap();
        pt.programmable_move_call(
            package_id,
            ident_str!("workload").to_owned(),
            ident_str!("payload").to_owned(),
            vec![],
            vec![bytes],
        );
    }
    let large = tx(cluster, sender, pt).await;
    assert!(bcs::to_bytes(&large).unwrap().len() > 16 * 1024);
    execute(cluster, &large).await;

    // This loop exceeds upstream public gasless's 5,000 computation-unit allowance.
    let mut pt = ProgrammableTransactionBuilder::new();
    let c = pt.obj(shared(&shared_counter)).unwrap();
    let a = pt.obj(owned(cluster, admin.id()).await).unwrap();
    let reference = pt.pure(42u64).unwrap();
    let amount = pt.pure(500u64).unwrap();
    let iterations = pt.pure(1_000_000u64).unwrap();
    pt.programmable_move_call(
        package_id,
        ident_str!("workload").to_owned(),
        ident_str!("pay").to_owned(),
        vec![],
        vec![c, a, reference, amount, iterations],
    );
    let payment = tx(cluster, sender, pt).await;
    let mut unauthorized = payment.clone();
    *unauthorized.sender_mut_for_testing() = recipient;
    unauthorized.gas_data_mut().owner = recipient;
    assert!(
        cluster
            .wallet
            .grpc_client()
            .unwrap()
            .simulate_transaction(&unauthorized, true, false)
            .await
            .is_err()
    );
    let simulated = cluster
        .wallet
        .grpc_client()
        .unwrap()
        .simulate_transaction(&payment, true, false)
        .await
        .unwrap();
    assert!(simulated.transaction.effects.status().is_ok());
    assert_eq!(
        simulated.transaction.effects.gas_cost_summary(),
        &GasCostSummary::default()
    );
    assert_eq!(counter(cluster, shared_counter.id()).await.value, 0);
    let result = execute(cluster, &payment).await;
    assert_eq!(result.events.as_ref().unwrap().data.len(), 1);
    assert_eq!(counter(cluster, shared_counter.id()).await.value, 500);
    let repeated = execute(cluster, &payment).await;
    assert_eq!(result.effects, repeated.effects);
    assert_eq!(
        counter(cluster, shared_counter.id()).await.references,
        vec![42]
    );

    // A newly signed retry with the same business reference must abort atomically.
    let mut duplicate = payment.clone();
    if let TransactionKind::ProgrammableTransaction(pt) = duplicate.kind_mut() {
        for input in &mut pt.inputs {
            if let sui_types::transaction::CallArg::Object(ObjectArg::ImmOrOwnedObject(reference)) =
                input
                && reference.0 == admin.id()
            {
                *reference = cluster
                    .get_object_from_fullnode_store(&admin.id())
                    .await
                    .unwrap()
                    .compute_object_reference();
            }
        }
    }
    if let TransactionExpiration::ValidDuring { nonce, .. } = duplicate.expiration_mut() {
        *nonce += 1;
    }
    let (effects, _) = cluster
        .execute_transaction_return_raw_effects(cluster.sign_transaction(&duplicate).await)
        .await
        .unwrap();
    assert!(effects.status().is_err());
    assert_eq!(effects.gas_cost_summary(), &GasCostSummary::default());
    assert_eq!(counter(cluster, shared_counter.id()).await.value, 500);

    let mut pt = ProgrammableTransactionBuilder::new();
    let c = pt.obj(shared(&shared_counter)).unwrap();
    pt.programmable_move_call(
        package_id,
        ident_str!("workload").to_owned(),
        ident_str!("abort_after_write").to_owned(),
        vec![],
        vec![c],
    );
    let data = tx(cluster, sender, pt).await;
    let (effects, _) = cluster
        .execute_transaction_return_raw_effects(cluster.sign_transaction(&data).await)
        .await
        .unwrap();
    assert!(effects.status().is_err());
    assert_eq!(effects.gas_cost_summary(), &GasCostSummary::default());
    assert_eq!(counter(cluster, shared_counter.id()).await.value, 500);

    // Shared-only inputs retain replay protection without a gas coin.
    let mut pt = ProgrammableTransactionBuilder::new();
    let c = pt.obj(shared(&shared_counter)).unwrap();
    let amount = pt.pure(1u64).unwrap();
    pt.programmable_move_call(
        package_id,
        ident_str!("workload").to_owned(),
        ident_str!("shared_only").to_owned(),
        vec![],
        vec![c, amount],
    );
    let protected = tx(cluster, sender, pt).await;
    let mut unprotected = protected.clone();
    *unprotected.expiration_mut() = TransactionExpiration::None;
    assert!(
        cluster
            .wallet
            .grpc_client()
            .unwrap()
            .simulate_transaction(&unprotected, true, false)
            .await
            .is_err()
    );
    execute(cluster, &protected).await;

    let (_, wrong_key): (_, sui_types::crypto::AccountKeyPair) = sui_types::crypto::get_key_pair();
    let mut invalid_data = protected.clone();
    if let TransactionExpiration::ValidDuring { nonce, .. } = invalid_data.expiration_mut() {
        *nonce = nonce.wrapping_add(999);
    }
    let invalid_signature =
        sui_types::transaction::Transaction::from_data_and_signer(invalid_data, vec![&wrong_key]);
    assert!(
        cluster
            .wallet
            .execute_transaction_may_fail(invalid_signature)
            .await
            .is_err()
    );

    let mut pt = ProgrammableTransactionBuilder::new();
    let amount = pt.pure(1u64).unwrap();
    pt.command(Command::SplitCoins(Argument::GasCoin, vec![amount]));
    let missing_gas = tx(cluster, sender, pt).await;
    assert!(
        cluster
            .wallet
            .grpc_client()
            .unwrap()
            .simulate_transaction(&missing_gas, true, false)
            .await
            .is_err()
    );

    let mut pt = ProgrammableTransactionBuilder::new();
    let value = pt.pure(7u64).unwrap();
    pt.programmable_move_call(
        package_id,
        ident_str!("workload").to_owned(),
        ident_str!("create").to_owned(),
        vec![],
        vec![value],
    );
    let made = execute(cluster, &tx(cluster, sender, pt).await).await;
    let ticket = created(cluster, &made, "::workload::Ticket").await;
    let mut pt = ProgrammableTransactionBuilder::new();
    let t = pt.obj(owned(cluster, ticket.id()).await).unwrap();
    let delta = pt.pure(3u64).unwrap();
    pt.programmable_move_call(
        package_id,
        ident_str!("workload").to_owned(),
        ident_str!("mutate").to_owned(),
        vec![],
        vec![t, delta],
    );
    execute(cluster, &tx(cluster, sender, pt).await).await;
    let mut pt = ProgrammableTransactionBuilder::new();
    let t = pt.obj(owned(cluster, ticket.id()).await).unwrap();
    pt.transfer_arg(recipient, t);
    execute(cluster, &tx(cluster, sender, pt).await).await;
    assert_eq!(
        cluster
            .get_object_from_fullnode_store(&ticket.id())
            .await
            .unwrap()
            .owner,
        Owner::AddressOwner(recipient)
    );
    let mut pt = ProgrammableTransactionBuilder::new();
    let stale = pt
        .obj(ObjectArg::ImmOrOwnedObject(
            ticket.compute_object_reference(),
        ))
        .unwrap();
    pt.transfer_arg(recipient, stale);
    // Simulation can read historical object versions; submission must reject a spent reference.
    let stale_transaction = cluster
        .sign_transaction(&tx(cluster, sender, pt).await)
        .await;
    assert!(
        cluster
            .wallet
            .execute_transaction_may_fail(stale_transaction)
            .await
            .is_err()
    );
    let mut pt = ProgrammableTransactionBuilder::new();
    let t = pt.obj(owned(cluster, ticket.id()).await).unwrap();
    pt.programmable_move_call(
        package_id,
        ident_str!("workload").to_owned(),
        ident_str!("destroy").to_owned(),
        vec![],
        vec![t],
    );
    execute(cluster, &tx(cluster, recipient, pt).await).await;
    assert!(
        cluster
            .get_object_from_fullnode_store(&ticket.id())
            .await
            .is_none()
    );

    let mut pt = ProgrammableTransactionBuilder::new();
    let cap = pt.obj(owned(cluster, treasury.id()).await).unwrap();
    let amount = pt.pure(1000u64).unwrap();
    let target = pt.pure(sender).unwrap();
    pt.programmable_move_call(
        package_id,
        ident_str!("workload").to_owned(),
        ident_str!("mint").to_owned(),
        vec![],
        vec![cap, amount, target],
    );
    let minted = execute(cluster, &tx(cluster, sender, pt).await).await;
    let coin = created(cluster, &minted, "::coin::Coin<").await;
    let mut pt = ProgrammableTransactionBuilder::new();
    let coin_arg = pt.obj(owned(cluster, coin.id()).await).unwrap();
    let amount = pt.pure(400u64).unwrap();
    let split = pt.command(Command::SplitCoins(coin_arg, vec![amount]));
    pt.command(Command::MergeCoins(coin_arg, vec![split]));
    pt.transfer_arg(recipient, coin_arg);
    execute(cluster, &tx(cluster, sender, pt).await).await;
    assert_eq!(env.get_coin_balance(coin.id()).await, 1000);

    // Existing explicit-gas envelopes are also unmetered, while preserving the coin's value.
    let (rich, gas) = env
        .gas_objects
        .iter()
        .find(|(_, coins)| !coins.is_empty())
        .unwrap();
    let mut pt = ProgrammableTransactionBuilder::new();
    let amount = pt.pure(1u64).unwrap();
    let split = pt.command(Command::SplitCoins(Argument::GasCoin, vec![amount]));
    pt.transfer_arg(*rich, split);
    let explicit =
        TransactionData::new_programmable(*rich, vec![gas[0]], pt.finish(), 10_000_000, 1000);
    execute(cluster, &explicit).await;
    assert_eq!(env.get_coin_balance(gas[0].0).await, 999);
    let mut zero_price = explicit.clone();
    zero_price.gas_data_mut().payment = vec![
        cluster
            .get_object_from_fullnode_store(&gas[0].0)
            .await
            .unwrap()
            .compute_object_reference(),
    ];
    zero_price.gas_data_mut().price = 0;
    zero_price.gas_data_mut().budget = 0;
    execute(cluster, &zero_price).await;
    assert_eq!(env.get_coin_balance(gas[0].0).await, 998);

    let mut pt = ProgrammableTransactionBuilder::new();
    let cap = pt.obj(ObjectArg::ImmOrOwnedObject(upgrade_cap)).unwrap();
    let policy = pt.pure(UpgradePolicy::COMPATIBLE).unwrap();
    let digest = pt.pure(package.get_package_digest(false).to_vec()).unwrap();
    let ticket = pt.programmable_move_call(
        SUI_FRAMEWORK_PACKAGE_ID,
        ident_str!("package").to_owned(),
        ident_str!("authorize_upgrade").to_owned(),
        vec![],
        vec![cap, policy, digest],
    );
    let receipt = pt.upgrade(
        package_id,
        ticket,
        package.get_dependency_storage_package_ids(),
        package.get_package_bytes(false),
    );
    pt.programmable_move_call(
        SUI_FRAMEWORK_PACKAGE_ID,
        ident_str!("package").to_owned(),
        ident_str!("commit_upgrade").to_owned(),
        vec![],
        vec![cap, receipt],
    );
    execute(cluster, &tx(cluster, sender, pt).await).await;

    let mut concurrent = Vec::new();
    for value in 0..24u64 {
        let mut pt = ProgrammableTransactionBuilder::new();
        let value = pt.pure(value).unwrap();
        pt.programmable_move_call(
            package_id,
            ident_str!("workload").to_owned(),
            ident_str!("create").to_owned(),
            vec![],
            vec![value],
        );
        concurrent.push(tx(cluster, sender, pt).await);
    }
    let results =
        futures::future::join_all(concurrent.iter().map(|data| execute(cluster, data))).await;
    assert_eq!(results.len(), 24);

    cluster.trigger_reconfiguration().await;
    cluster.trigger_reconfiguration().await;
    let mut expired = protected.clone();
    if let TransactionExpiration::ValidDuring { nonce, .. } = expired.expiration_mut() {
        *nonce = nonce.wrapping_add(1234);
    }
    assert!(
        cluster
            .wallet
            .grpc_client()
            .unwrap()
            .simulate_transaction(&expired, true, false)
            .await
            .is_err()
    );
    cluster.stop_all_validators().await;
    cluster.start_all_validators().await;
    let mut pt = ProgrammableTransactionBuilder::new();
    let c = pt.obj(shared(&shared_counter)).unwrap();
    let amount = pt.pure(1u64).unwrap();
    pt.programmable_move_call(
        package_id,
        ident_str!("workload").to_owned(),
        ident_str!("shared_only").to_owned(),
        vec![],
        vec![c, amount],
    );
    execute(cluster, &tx(cluster, sender, pt).await).await;
    assert_eq!(counter(cluster, shared_counter.id()).await.value, 502);
    assert_eq!(env.get_sui_balance_ab(sender), 0);
    assert!(cluster.wallet.gas_objects(sender).await.unwrap().is_empty());
}

#[sim_test]
async fn zone_disabled_preserves_paid_execution() {
    let env = TestEnvBuilder::new()
        .with_num_validators(4)
        .with_protocol_version(143.into())
        .build()
        .await;
    let (sender, gas) = env.get_sender_and_gas(0);
    let recipient = env.get_sender(1);
    let mut pt = ProgrammableTransactionBuilder::new();
    let amount = pt.pure(1u64).unwrap();
    let split = pt.command(Command::SplitCoins(Argument::GasCoin, vec![amount]));
    pt.transfer_arg(recipient, split);
    let data =
        TransactionData::new_programmable(sender, vec![gas], pt.finish(), 10_000_000, env.rgp);
    let response = env.cluster.sign_and_execute_transaction(&data).await;
    assert!(response.effects.gas_cost_summary().computation_cost > 0);
    let object = created(&env.cluster, &response, "::coin::Coin<").await;
    let mut pt = ProgrammableTransactionBuilder::new();
    let coin = pt
        .obj(ObjectArg::ImmOrOwnedObject(
            object.compute_object_reference(),
        ))
        .unwrap();
    pt.transfer_arg(sender, coin);
    let gasless = tx(&env.cluster, recipient, pt).await;
    assert!(
        env.cluster
            .wallet
            .grpc_client()
            .unwrap()
            .simulate_transaction(&gasless, true, false)
            .await
            .is_err()
    );
}
