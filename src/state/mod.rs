use std::collections::HashMap;

use crate::core::Transaction;

#[derive(Debug, Clone)]
pub struct Account {
    pub balance: u64,
    pub nonce: u64,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct State {
    pub accounts: HashMap<String, Account>,
    pub treasury_balance: u64,
    pub burned_amount: u64,
}

#[allow(dead_code)]
impl State {
    pub fn new() -> Self {
        Self {
            accounts: HashMap::new(),
            treasury_balance: 0,
            burned_amount: 0,
        }
    }

    pub fn create_account(&mut self, address: String, balance: u64) {
        self.accounts.insert(address, Account { balance, nonce: 0 });
    }

    pub fn balance_of(&self, address: &str) -> u64 {
        self.accounts
            .get(address)
            .map(|account| account.balance)
            .unwrap_or(0)
    }

    pub fn nonce_of(&self, address: &str) -> u64 {
        self.accounts
            .get(address)
            .map(|account| account.nonce)
            .unwrap_or(0)
    }

    pub fn add_balance(&mut self, address: &str, amount: u64) -> Result<(), String> {
        let account = self
            .accounts
            .get_mut(address)
            .ok_or("Hesap bulunamadı")?;

        account.balance = account
            .balance
            .checked_add(amount)
            .ok_or("Bakiye overflow")?;

        Ok(())
    }

    pub fn add_treasury(&mut self, amount: u64) -> Result<(), String> {
        self.treasury_balance = self
            .treasury_balance
            .checked_add(amount)
            .ok_or("Treasury overflow")?;

        Ok(())
    }

    pub fn treasury(&self) -> u64 {
        self.treasury_balance
    }

    pub fn burn(&mut self, amount: u64) -> Result<(), String> {
        self.burned_amount = self
            .burned_amount
            .checked_add(amount)
            .ok_or("Burn overflow")?;

        Ok(())
    }

    pub fn burned(&self) -> u64 {
        self.burned_amount
    }

    // ==========================
    // TRANSACTION APPLY
    // ==========================

    pub fn apply_transaction(
        &mut self,
        transaction: &Transaction,
    ) -> Result<(), String> {
        // COINBASE
        // Sistem üretimi

        if transaction.coinbase {
            let receiver = self
                .accounts
                .entry(transaction.to.clone())
                .or_insert(Account {
                    balance: 0,
                    nonce: 0,
                });

            receiver.balance = receiver
                .balance
                .checked_add(transaction.amount)
                .ok_or("Coinbase overflow")?;

            return Ok(());
        }

        let sender = self
            .accounts
            .get(&transaction.from)
            .ok_or("Gönderen hesap bulunamadı")?
            .clone();

        let total_cost = transaction
            .amount
            .checked_add(transaction.fee)
            .ok_or("Transaction overflow")?;

        if sender.balance < total_cost {
            return Err("Yetersiz bakiye".into());
        }

        if sender.nonce != transaction.nonce {
            return Err("Nonce hatalı".into());
        }

        let sender_account = self
            .accounts
            .get_mut(&transaction.from)
            .ok_or("Gönderen hesap bulunamadı")?;

        sender_account.balance -= total_cost;
        sender_account.nonce += 1;

        let receiver_account = self
            .accounts
            .entry(transaction.to.clone())
            .or_insert(Account {
                balance: 0,
                nonce: 0,
            });

        receiver_account.balance = receiver_account
            .balance
            .checked_add(transaction.amount)
            .ok_or("Alıcı overflow")?;

        Ok(())
    }

    pub fn apply_transactions_atomically(
        &mut self,
        transactions: &[Transaction],
    ) -> Result<(), String> {
        let mut temp = self.clone();

        for tx in transactions {
            temp.apply_transaction(tx)?;
        }

        *self = temp;

        Ok(())
    }
}