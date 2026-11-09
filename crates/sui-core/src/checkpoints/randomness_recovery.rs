// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Repair local indexes left incomplete by binaries predating upstream #22564.
//! Only certified, already executed, current-epoch transactions are used. Never
//! regenerate random bytes or change a checkpoint to resolve a missing key.

use super::CheckpointStore;
use crate::authority::authority_per_epoch_store::AuthorityPerEpochStore;
use crate::execution_cache::TransactionCacheRead;
use anyhow::{anyhow, ensure, Context};
use std::time::Instant;
use sui_types::digests::TransactionDigest;
use sui_types::messages_checkpoint::VerifiedCheckpoint;
use sui_types::transaction::TransactionKey;
use tracing::{info, warn};

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct RecoveryStats {
    scanned: u64,
    inserted: u64,
    existing: u64,
    skipped: u64,
}

pub(super) async fn recover_randomness_keys(
    store: &CheckpointStore,
    epoch_store: &AuthorityPerEpochStore,
    transactions: &dyn TransactionCacheRead,
) -> anyhow::Result<RecoveryStats> {
    let start = Instant::now();
    let epoch = epoch_store.epoch();
    let mut stats = RecoveryStats::default();
    // Outputs can reach disk before the executed checkpoint watermark advances.
    // Scan the captured synced range too, requiring matching local effects for
    // any transaction beyond the executed watermark. This must happen before
    // builder replay: node startup waits for the builder before starting execution.
    let executed = store.get_highest_executed_checkpoint()?;
    let synced = store.get_highest_synced_checkpoint()?;
    let Some(highest) = executed
        .iter()
        .chain(synced.iter())
        .max_by_key(|c| c.sequence_number())
    else {
        return Ok(stats);
    };
    if highest.epoch() < epoch {
        return Ok(stats);
    }
    let last = if highest.epoch() == epoch {
        *highest.sequence_number()
    } else {
        store
            .get_epoch_last_checkpoint_seq_number(epoch)?
            .context("current epoch checkpoint boundary is unavailable")?
    };
    let executed_through = executed.as_ref().map(|c| *c.sequence_number());
    let first = if epoch == 0 {
        0
    } else {
        store
            .get_epoch_last_checkpoint_seq_number(epoch - 1)?
            .context("previous epoch checkpoint boundary is unavailable")?
            .checked_add(1)
            .context("checkpoint sequence number overflow")?
    };
    ensure!(first <= last, "invalid current epoch checkpoint boundary");
    info!(epoch, first, last, "Starting randomness key recovery");

    for sequence in first..=last {
        // Yield even on skipped checkpoints, so shutdown/reconfiguration can
        // cancel the builder task and large scans cannot monopolize a worker.
        tokio::task::yield_now().await;
        stats.scanned += 1;
        let result = match store.get_checkpoint_by_sequence_number(sequence) {
            Ok(Some(checkpoint)) => recover_checkpoint(
                store,
                epoch_store,
                transactions,
                &checkpoint,
                executed_through,
                &mut stats,
            ),
            Ok(None) => Err(anyhow!("certified checkpoint is unavailable")),
            Err(error) => Err(error.into()),
        };
        if let Err(error) = result {
            stats.skipped += 1;
            warn!(
                epoch,
                sequence,
                ?error,
                "Skipping checkpoint during randomness key recovery"
            );
        }
    }
    info!(
        epoch,
        first,
        last,
        scanned = stats.scanned,
        inserted = stats.inserted,
        existing = stats.existing,
        skipped = stats.skipped,
        elapsed_ms = start.elapsed().as_millis() as u64,
        "Finished randomness key recovery"
    );
    Ok(stats)
}

fn recover_checkpoint(
    store: &CheckpointStore,
    epoch_store: &AuthorityPerEpochStore,
    transactions: &dyn TransactionCacheRead,
    checkpoint: &VerifiedCheckpoint,
    executed_through: Option<u64>,
    stats: &mut RecoveryStats,
) -> anyhow::Result<()> {
    let epoch = epoch_store.epoch();
    ensure!(
        checkpoint.epoch() == epoch,
        "checkpoint belongs to another epoch"
    );
    // Version-specific summaries let us avoid loading ordinary checkpoints.
    // Older summaries need their contents inspected instead.
    let rounds = checkpoint
        .version_specific_data(epoch_store.protocol_config())?
        .map(|data| data.into_v1().randomness_rounds);
    if rounds.as_ref().is_some_and(|rounds| rounds.is_empty()) {
        return Ok(());
    }
    let contents = store
        .get_checkpoint_contents(&checkpoint.content_digest)?
        .context("checkpoint contents are unavailable (possibly pruned)")?;
    ensure!(
        *contents.digest() == checkpoint.content_digest,
        "checkpoint contents digest mismatch"
    );
    for entry in contents.iter() {
        let Some(transaction) = transactions.get_transaction_block(&entry.transaction) else {
            stats.skipped += 1;
            warn!(epoch, sequence = checkpoint.sequence_number, digest = ?entry.transaction,
                "Skipping unavailable transaction during randomness key recovery");
            continue;
        };
        ensure!(
            *transaction.digest() == entry.transaction,
            "checkpoint transaction digest mismatch"
        );
        let key = transaction.key();
        let TransactionKey::RandomnessRound(tx_epoch, round) = key else {
            continue;
        };
        ensure!(
            tx_epoch == epoch,
            "randomness transaction belongs to another epoch"
        );
        if let Some(rounds) = &rounds {
            ensure!(
                rounds.contains(&round),
                "randomness round absent from checkpoint summary"
            );
        }
        if !executed_through.is_some_and(|sequence| checkpoint.sequence_number <= sequence) {
            let effects = transactions.multi_get_executed_effects_digests(&[entry.transaction]);
            let Some(executed_effects) = effects[0] else {
                // Synced data is not proof of execution. Leave future rounds alone.
                continue;
            };
            ensure!(
                executed_effects == entry.effects,
                "executed effects differ from certified checkpoint"
            );
        }
        match recover_transaction_key(epoch_store, key, transaction.digest(), &entry.transaction) {
            Ok(false) => stats.existing += 1,
            Ok(true) => stats.inserted += 1,
            Err(error) => {
                stats.skipped += 1;
                warn!(
                    epoch,
                    ?round,
                    ?error,
                    "Unable to recover randomness key; existing mapping preserved"
                );
            }
        }
    }
    Ok(())
}

/// Also used when old transaction effects were committed before a crash, but the
/// checkpoint execution watermark had not yet advanced. Such transactions bypass
/// commit_certificate; execution can also advance beyond the startup scan's bound.
pub(super) fn recover_transaction_key(
    epoch_store: &AuthorityPerEpochStore,
    key: TransactionKey,
    actual_digest: &TransactionDigest,
    certified_digest: &TransactionDigest,
) -> anyhow::Result<bool> {
    ensure!(
        actual_digest == certified_digest,
        "checkpoint transaction digest mismatch"
    );
    ensure!(
        matches!(key, TransactionKey::RandomnessRound(epoch, _) if epoch == epoch_store.epoch()),
        "not a current-epoch randomness transaction"
    );
    match epoch_store.tx_key_to_digest(&key)? {
        Some(existing) => {
            ensure!(
                existing == *certified_digest,
                "conflicting randomness key mapping"
            );
            Ok(false)
        }
        None => {
            epoch_store.insert_tx_key(key, *certified_digest)?;
            Ok(true)
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::authority::test_authority_builder::TestAuthorityBuilder;
    use crate::authority::{AuthorityState, ExecutionEnv};
    use futures::poll;
    use std::sync::Arc;
    use std::task::Poll;
    use std::time::Duration;
    use sui_protocol_config::{Chain, ProtocolConfig, ProtocolVersion};
    use sui_types::base_types::ExecutionDigests;
    use sui_types::crypto::{AuthorityStrongQuorumSignInfo, RandomnessRound};
    use sui_types::digests::{TransactionDigest, TransactionEffectsDigest};
    use sui_types::effects::TransactionEffectsAPI;
    use sui_types::executable_transaction::VerifiedExecutableTransaction;
    use sui_types::gas::GasCostSummary;
    use sui_types::messages_checkpoint::{
        CertifiedCheckpointSummary, CheckpointContents, CheckpointSummary, SignedCheckpointSummary,
    };
    use sui_types::transaction::VerifiedTransaction;
    use typed_store::Map;

    pub(in crate::checkpoints) async fn authority() -> Arc<AuthorityState> {
        TestAuthorityBuilder::new()
            .with_protocol_config(ProtocolConfig::get_for_version(
                ProtocolVersion::new(87),
                Chain::Unknown,
            ))
            .build()
            .await
    }

    pub(in crate::checkpoints) fn randomness(
        state: &AuthorityState,
        round: u64,
    ) -> VerifiedTransaction {
        let epoch_store = state.epoch_store_for_testing();
        VerifiedTransaction::new_randomness_state_update(
            epoch_store.epoch(),
            RandomnessRound(round),
            vec![42; 48],
            epoch_store
                .epoch_start_config()
                .randomness_obj_initial_shared_version()
                .unwrap(),
        )
    }

    // Recovery tests use signed checkpoint fixtures and real RocksDB/cache reads.
    // Execution and live restart behavior are distinct tests, not implied here.
    pub(in crate::checkpoints) fn checkpoint(
        state: &AuthorityState,
        store: &CheckpointStore,
        sequence: u64,
        transaction: &VerifiedTransaction,
        persist_transaction: bool,
    ) -> VerifiedCheckpoint {
        checkpoint_with_effects(
            state,
            store,
            sequence,
            transaction,
            persist_transaction,
            TransactionEffectsDigest::random(),
        )
    }

    pub(in crate::checkpoints) fn checkpoint_with_effects(
        state: &AuthorityState,
        store: &CheckpointStore,
        sequence: u64,
        transaction: &VerifiedTransaction,
        persist_transaction: bool,
        effects: TransactionEffectsDigest,
    ) -> VerifiedCheckpoint {
        let epoch_store = state.epoch_store_for_testing();
        let contents = CheckpointContents::new_with_digests_only_for_tests([ExecutionDigests {
            transaction: *transaction.digest(),
            effects,
        }]);
        let rounds = match transaction.key() {
            TransactionKey::RandomnessRound(_, round) => vec![round],
            _ => vec![],
        };
        let summary = CheckpointSummary::new(
            epoch_store.protocol_config(),
            epoch_store.epoch(),
            sequence,
            sequence + 1,
            &contents,
            None,
            GasCostSummary::default(),
            None,
            sequence * 1000,
            rounds,
        );
        let signed = SignedCheckpointSummary::new(
            epoch_store.epoch(),
            summary.clone(),
            state.secret.as_ref().get_ref(),
            state.name,
        );
        let signature = AuthorityStrongQuorumSignInfo::new_from_auth_sign_infos(
            vec![signed.auth_sig().clone()],
            epoch_store.committee(),
        )
        .unwrap();
        let certified = CertifiedCheckpointSummary::new_from_data_and_sig(summary, signature)
            .try_into_verified(epoch_store.committee())
            .unwrap();
        store.insert_checkpoint_contents(contents).unwrap();
        store.insert_verified_checkpoint(&certified).unwrap();
        if persist_transaction {
            state.get_cache_commit().persist_transaction(
                &VerifiedExecutableTransaction::new_from_checkpoint(
                    transaction.clone(),
                    epoch_store.epoch(),
                    sequence,
                ),
            );
        }
        certified
    }

    #[tokio::test]
    async fn randomness_checkpoint_execution_records_key() {
        let state = authority().await;
        let epoch_store = state.epoch_store_for_testing();
        let transaction = randomness(&state, 0);
        let key = transaction.key();
        let digest = *transaction.digest();
        let certificate = VerifiedExecutableTransaction::new_from_checkpoint(transaction, 0, 1);
        let assigned = epoch_store
            .assign_shared_object_versions_for_tests(
                state.get_object_cache_reader().as_ref(),
                &[certificate.clone()],
            )
            .unwrap()
            .into_map()
            .remove(&key)
            .unwrap();
        assert_eq!(epoch_store.tx_key_to_digest(&key).unwrap(), None);
        let (effects, error) = state
            .try_execute_immediately(
                &certificate,
                ExecutionEnv::new().with_assigned_versions(assigned),
                &epoch_store,
            )
            .await
            .unwrap();
        assert!(error.is_none(), "{error:?}");
        assert!(effects.status().is_ok(), "{:?}", effects.status());
        assert_eq!(epoch_store.tx_key_to_digest(&key).unwrap(), Some(digest));
    }

    #[tokio::test]
    async fn recovery_wakes_missing_key_waiter_and_is_idempotent() {
        let state = authority().await;
        let epoch_store = state.epoch_store_for_testing();
        let directory = tempfile::tempdir().unwrap();
        let store = CheckpointStore::new(directory.path());
        let transaction = randomness(&state, 0);
        let certified = checkpoint(&state, &store, 0, &transaction, true);
        store
            .update_highest_executed_checkpoint(&certified)
            .unwrap();
        let original_summary = certified.data().clone();
        let keys = [transaction.key()];
        let mut waiting = Box::pin(epoch_store.notify_read_tx_key_to_digest(&keys));
        assert!(matches!(poll!(&mut waiting), Poll::Pending));
        let cache = state.get_transaction_cache_reader();
        let stats = recover_randomness_keys(&store, &epoch_store, cache.as_ref())
            .await
            .unwrap();
        assert_eq!(stats.inserted, 1);
        assert_eq!(stats.skipped, 0);
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), waiting)
                .await
                .unwrap()
                .unwrap(),
            vec![*transaction.digest()]
        );
        let again = recover_randomness_keys(&store, &epoch_store, cache.as_ref())
            .await
            .unwrap();
        assert_eq!(again.inserted, 0);
        assert_eq!(again.existing, 1);
        assert_eq!(
            store
                .get_checkpoint_by_sequence_number(0)
                .unwrap()
                .unwrap()
                .data(),
            &original_summary
        );
    }

    #[tokio::test]
    async fn recovery_preserves_conflicts_and_execution_boundary() {
        let state = authority().await;
        let epoch_store = state.epoch_store_for_testing();
        let directory = tempfile::tempdir().unwrap();
        let store = CheckpointStore::new(directory.path());
        let executed = randomness(&state, 0);
        let future = randomness(&state, 1);
        let certified = checkpoint(&state, &store, 0, &executed, true);
        let synced = checkpoint(&state, &store, 1, &future, true);
        store.update_highest_synced_checkpoint(&synced).unwrap();
        store
            .update_highest_executed_checkpoint(&certified)
            .unwrap();
        let conflict = TransactionDigest::random();
        epoch_store.insert_tx_key(executed.key(), conflict).unwrap();
        let stats = recover_randomness_keys(
            &store,
            &epoch_store,
            state.get_transaction_cache_reader().as_ref(),
        )
        .await
        .unwrap();
        assert_eq!(stats.scanned, 2);
        assert_eq!(stats.inserted, 0);
        assert_eq!(stats.skipped, 1);
        assert_eq!(
            epoch_store.tx_key_to_digest(&executed.key()).unwrap(),
            Some(conflict)
        );
        assert_eq!(epoch_store.tx_key_to_digest(&future.key()).unwrap(), None);
    }

    #[tokio::test]
    async fn recovery_skips_missing_transactions_and_continues() {
        let state = authority().await;
        let epoch_store = state.epoch_store_for_testing();
        let directory = tempfile::tempdir().unwrap();
        let store = CheckpointStore::new(directory.path());
        let absent = randomness(&state, 0);
        let available = randomness(&state, 1);
        let first = checkpoint(&state, &store, 0, &absent, false);
        let second = checkpoint(&state, &store, 1, &available, true);
        store.update_highest_executed_checkpoint(&first).unwrap();
        store.update_highest_executed_checkpoint(&second).unwrap();
        let stats = recover_randomness_keys(
            &store,
            &epoch_store,
            state.get_transaction_cache_reader().as_ref(),
        )
        .await
        .unwrap();
        assert_eq!(stats.inserted, 1);
        assert_eq!(stats.skipped, 1);
        assert_eq!(epoch_store.tx_key_to_digest(&absent.key()).unwrap(), None);
        assert_eq!(
            epoch_store.tx_key_to_digest(&available.key()).unwrap(),
            Some(*available.digest())
        );
    }

    #[tokio::test]
    async fn recovery_skips_pruned_contents_and_foreign_epoch_transactions() {
        let state = authority().await;
        let epoch_store = state.epoch_store_for_testing();
        let directory = tempfile::tempdir().unwrap();
        let store = CheckpointStore::new(directory.path());
        let pruned = randomness(&state, 0);
        let foreign = VerifiedTransaction::new_randomness_state_update(
            1,
            RandomnessRound(0),
            vec![42; 48],
            epoch_store
                .epoch_start_config()
                .randomness_obj_initial_shared_version()
                .unwrap(),
        );
        let first = checkpoint(&state, &store, 0, &pruned, true);
        let second = checkpoint(&state, &store, 1, &foreign, true);
        store
            .tables
            .checkpoint_content
            .remove(&first.content_digest)
            .unwrap();
        store.update_highest_executed_checkpoint(&first).unwrap();
        store.update_highest_executed_checkpoint(&second).unwrap();
        let stats = recover_randomness_keys(
            &store,
            &epoch_store,
            state.get_transaction_cache_reader().as_ref(),
        )
        .await
        .unwrap();
        assert_eq!(stats.inserted, 0);
        assert_eq!(stats.skipped, 2);
        assert_eq!(epoch_store.tx_key_to_digest(&pruned.key()).unwrap(), None);
        assert_eq!(epoch_store.tx_key_to_digest(&foreign.key()).unwrap(), None);
    }

    #[tokio::test]
    async fn recovery_rejects_a_transaction_with_the_wrong_digest() {
        let state = authority().await;
        let epoch_store = state.epoch_store_for_testing();
        let directory = tempfile::tempdir().unwrap();
        let store = CheckpointStore::new(directory.path());
        let expected = randomness(&state, 0);
        let wrong = randomness(&state, 1);
        let certified = checkpoint(&state, &store, 0, &expected, false);
        // Deliberately corrupt only this temporary test store's lookup entry.
        state
            .database_for_testing()
            .perpetual_tables
            .transactions
            .insert(expected.digest(), wrong.serializable_ref())
            .unwrap();
        store
            .update_highest_executed_checkpoint(&certified)
            .unwrap();
        let stats = recover_randomness_keys(
            &store,
            &epoch_store,
            state.get_transaction_cache_reader().as_ref(),
        )
        .await
        .unwrap();
        assert_eq!(stats.inserted, 0);
        assert_eq!(stats.skipped, 1);
        assert_eq!(epoch_store.tx_key_to_digest(&expected.key()).unwrap(), None);
        assert_eq!(epoch_store.tx_key_to_digest(&wrong.key()).unwrap(), None);
    }
}
