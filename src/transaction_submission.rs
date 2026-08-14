use std::path::{Path, PathBuf};

use crate::bootstrap::canonical_bootstrap;
use crate::core::Transaction;
use crate::economy::Economy;
use crate::network::tcp::TcpTransport;
use crate::network::{
    Network,
    NetworkMessage,
    ONE_SHOT_CLIENT_LISTEN_ADDRESS,
};
use crate::node::Node;
use crate::protocol::{is_fixed_hex, ADDRESS_HEX_LENGTH};
use crate::state::State;
use crate::storage::Storage;
use crate::validator::ValidatorKeystore;
use crate::wallet::Wallet;

pub fn prepare_signed_transaction(
    signer: &Wallet,
    recipient: &str,
    amount_micro_kbn: u64,
    state: &State,
    economy: &Economy,
) -> Result<Transaction, String> {
    if !is_fixed_hex(recipient, ADDRESS_HEX_LENGTH) {
        return Err("Recipient adresi 64 karakter hex olmalı".into());
    }

    if amount_micro_kbn == 0 {
        return Err("Transaction miktarı sıfır olamaz".into());
    }

    let fee = economy.calculate_fee(amount_micro_kbn);
    let total_cost = amount_micro_kbn
        .checked_add(fee)
        .ok_or("Transaction miktarı ve fee toplamı overflow")?;
    let sender = signer.address();
    let balance = state.balance_of(sender);

    if balance < total_cost {
        return Err("Transaction için bakiye yetersiz".into());
    }

    let mut transaction = Transaction::new(
        sender.to_string(),
        signer.public_key_hex(),
        recipient.to_string(),
        amount_micro_kbn,
        fee,
        state.nonce_of(sender),
    );
    transaction.sign(signer.sign(&transaction.message()));

    let signature = transaction
        .signature
        .as_deref()
        .ok_or("Transaction imzası oluşturulamadı")?;
    let derived_address = Wallet::address_from_public_key(&transaction.public_key)
        .ok_or("Transaction public key adresi türetilemedi")?;

    if derived_address != transaction.from
        || transaction.id != transaction.calculate_id()
        || !Wallet::verify(
            &transaction.public_key,
            &transaction.message(),
            signature,
        )
    {
        return Err("Transaction imza veya kimlik doğrulaması başarısız".into());
    }

    Ok(transaction)
}

fn load_replayed_state(
    data_directory: &Path,
) -> Result<(State, Economy), String> {
    let chain = Storage::load_blockchain_from(data_directory)?
        .ok_or("Blockchain dosyası bulunamadı")?;
    let bootstrap = canonical_bootstrap()?;
    let stored_genesis = chain
        .first()
        .ok_or("Diskteki blockchain boş")?;
    let canonical_genesis = bootstrap
        .blockchain
        .chain
        .first()
        .ok_or("Canonical genesis bulunamadı")?;

    if stored_genesis.hash != stored_genesis.calculate_hash() {
        return Err("Diskteki genesis hash bütünlüğü geçersiz".into());
    }

    if stored_genesis.hash != canonical_genesis.hash {
        return Err("Diskteki genesis canonical Kybernetes genesis ile uyuşmuyor".into());
    }

    let mut replay_node = Node::new_with_data_directory(
        bootstrap.blockchain,
        bootstrap.state,
        bootstrap.consensus,
        PathBuf::new(),
    );

    for block in chain.into_iter().skip(1) {
        replay_node.apply_block_to_sync_candidate(block)?;
    }

    Ok((
        replay_node.state,
        replay_node.blockchain.economy,
    ))
}

pub fn prepare_from_active_validator(
    data_directory: &Path,
    validator_password: &str,
    recipient: &str,
    amount_micro_kbn: u64,
) -> Result<Transaction, String> {
    let bootstrap = canonical_bootstrap()?;
    let identity = ValidatorKeystore::at(data_directory)
        .load_authorized(
            validator_password,
            &bootstrap.consensus,
            &bootstrap.genesis_fingerprint,
        )?
        .ok_or("Aktif validator keystore bulunamadı")?;
    let (state, economy) = load_replayed_state(data_directory)?;

    prepare_signed_transaction(
        identity.wallet(),
        recipient,
        amount_micro_kbn,
        &state,
        &economy,
    )
}

pub async fn submit_from_active_validator(
    data_directory: &Path,
    validator_password: &str,
    peer_address: &str,
    recipient: &str,
    amount_micro_kbn: u64,
) -> Result<Transaction, String> {
    let transaction = prepare_from_active_validator(
        data_directory,
        validator_password,
        recipient,
        amount_micro_kbn,
    )?;
    let transaction_id = transaction.id.clone();
    let p2p_identity = Wallet::new();

    let response = TcpTransport::send_authenticated_request(
        peer_address,
        &p2p_identity,
        ONE_SHOT_CLIENT_LISTEN_ADDRESS,
        &NetworkMessage::Transaction(transaction.clone()),
    )
    .await?;

    if !Network::validate_transaction_ack(
        &response,
        &transaction_id,
    ) {
        return Err(
            "Peer geçerli ve eşleşen TransactionAck döndürmedi"
                .into(),
        );
    }

    match response {
        NetworkMessage::TransactionAck {
            accepted: true,
            reason: None,
            ..
        } => Ok(transaction),

        NetworkMessage::TransactionAck {
            accepted: false,
            reason: Some(reason),
            ..
        } => Err(format!(
            "Transaction node tarafından reddedildi: {}",
            reason
        )),

        _ => Err(
            "Peer geçersiz TransactionAck döndürdü"
                .into(),
        ),
    }
}
