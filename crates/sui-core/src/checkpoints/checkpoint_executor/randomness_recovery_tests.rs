// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::checkpoints::randomness_recovery::{
    recover_randomness_keys,
    tests::{authority, checkpoint, checkpoint_with_effects, randomness},
};

#[tokio::test]
async fn randomness_recovery_repairs_persisted_effects_ahead_of_checkpoint_watermark() {
    let state = authority().await;
    let epoch_store = state.epoch_store_for_testing();
    let directory = tempfile::tempdir().unwrap();
    let store = CheckpointStore::new(directory.path());
    let previous = checkpoint(
        &state,
        &store,
        0,
        &VerifiedTransaction::new_genesis_transaction(vec![]),
        true,
    );
    store.update_highest_executed_checkpoint(&previous).unwrap();

    let transaction = randomness(&state, 0);
    let key = transaction.key();
    let digest = *transaction.digest();
    let certificate = VerifiedExecutableTransaction::new_from_checkpoint(transaction.clone(), 0, 1);
    let assigned = epoch_store
        .assign_shared_object_versions_for_tests(
            state.get_object_cache_reader().as_ref(),
            &[certificate.clone()],
        )
        .unwrap()
        .into_map()
        .remove(&key)
        .unwrap();
    let (effects, error) = state
        .try_execute_immediately(
            &certificate,
            ExecutionEnv::new().with_assigned_versions(assigned),
            &epoch_store,
        )
        .await
        .unwrap();
    assert!(error.is_none(), "{error:?}");
    assert!(effects.status().is_ok());
    let batch = state
        .get_cache_commit()
        .build_db_batch(epoch_store.epoch(), &[digest]);
    state
        .get_cache_commit()
        .commit_transaction_outputs(epoch_store.epoch(), batch, &[digest]);

    // Model the old binary: its outputs reached disk, but it never wrote the key
    // and crashed before advancing the checkpoint execution watermark.
    epoch_store.remove_tx_key_for_testing(&key).unwrap();
    let certified =
        checkpoint_with_effects(&state, &store, 1, &transaction, true, effects.digest());
    store.update_highest_synced_checkpoint(&certified).unwrap();
    recover_randomness_keys(
        &store,
        &epoch_store,
        state.get_transaction_cache_reader().as_ref(),
    )
    .await
    .unwrap();
    assert_eq!(epoch_store.tx_key_to_digest(&key).unwrap(), Some(digest));
    assert_eq!(
        store.get_highest_executed_checkpoint_seq_number().unwrap(),
        Some(0)
    );

    // Exercise the independent already-executed path, including checkpoints that
    // become available after startup captured its bound.
    epoch_store.remove_tx_key_for_testing(&key).unwrap();

    let data = CheckpointExecutionState::new(CheckpointExecutionData {
        checkpoint_contents: store
            .get_checkpoint_contents(&certified.content_digest)
            .unwrap()
            .unwrap(),
        checkpoint: certified,
        tx_digests: vec![digest],
        fx_digests: vec![effects.digest()],
    });
    let transactions = CheckpointTransactionData {
        transactions: vec![certificate],
        executed_fx_digests: state
            .get_transaction_cache_reader()
            .multi_get_executed_effects_digests(&[digest]),
        effects: vec![effects.clone()],
    };
    assert_eq!(
        transactions.executed_fx_digests,
        vec![Some(effects.digest())]
    );
    let hasher = Arc::new(GlobalStateHasher::new_for_tests(
        state.get_global_state_hash_store().clone(),
    ));
    let executor =
        CheckpointExecutor::new_for_tests(epoch_store.clone(), store.clone(), state, hasher);
    assert!(executor
        .schedule_transaction_execution(&data, &transactions)
        .is_empty());
    assert_eq!(epoch_store.tx_key_to_digest(&key).unwrap(), Some(digest));
    assert_eq!(
        store.get_highest_executed_checkpoint_seq_number().unwrap(),
        Some(0)
    );
    // Repeating the already-executed path neither schedules nor changes the key.
    assert!(executor
        .schedule_transaction_execution(&data, &transactions)
        .is_empty());
    assert_eq!(epoch_store.tx_key_to_digest(&key).unwrap(), Some(digest));
}
