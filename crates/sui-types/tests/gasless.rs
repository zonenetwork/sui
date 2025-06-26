use sui_types::transaction::{TransactionData, TransactionKind, TransactionDataAPI};
use sui_types::transaction::TransactionExpiration;
use sui_types::base_types::SuiAddress;
use sui_protocol_config::ProtocolConfig;
use sui_types::transaction::ProgrammableTransaction;

#[test]
fn gasless_flag_default_is_false() {
    let config = ProtocolConfig::get_for_max_version_UNSAFE();
    assert!(!config.gasless_transactions());
}

#[test]
fn gasless_transaction_validity_skip_missing_gas() {
    // Prepare a dummy sender address.
    let sender = SuiAddress::random_for_testing_only();

    // Construct a dummy (empty) programmable tx – good enough for validity check focus on gas.
    let pt = ProgrammableTransaction { inputs: vec![], commands: vec![] };
    let kind = TransactionKind::ProgrammableTransaction(pt);

    // Build a tx with NO gas payment using the helper.
    let tx = TransactionData::new_gasless(kind, sender, TransactionExpiration::None);

    // 1. In normal (gas-metered) mode we expect MissingGasPayment error.
    let config_normal = ProtocolConfig::get_for_max_version_UNSAFE();
    let err = tx.validity_check(&config_normal).err().expect("should fail");
    let missing = format!("{:?}", err);
    assert!(missing.contains("MissingGasPayment"));

    // 2. Enable gasless flag – the same tx should pass the gas-payment gate.
    let mut config_gasless = ProtocolConfig::get_for_max_version_UNSAFE();
    config_gasless.set_gasless_transactions_for_testing(true);
    let res = tx.validity_check(&config_gasless);
    if let Err(e) = res {
        // It is acceptable for other validation errors to happen; we only care that
        // MissingGasPayment is NOT the reason in gasless mode.
        let msg = format!("{:?}", e);
        assert!(!msg.contains("MissingGasPayment"), "MissingGasPayment should be bypassed in gasless mode");
    }
} 