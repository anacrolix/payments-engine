use anyhow::Result;
use clap::Parser;
use serde::Deserialize;
use std::collections::HashMap;

/// Simple program to process a file
#[derive(Parser)]
struct Args {
    /// Input file to process
    filename: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
enum TransactionType {
    Deposit,
    Withdrawal,
    Dispute,
    Resolve,
    Chargeback,
}

type Amount = fixed::types::I50F14;
type ClientId = u16;
type TransactionId = u32;

#[derive(Deserialize, Debug)]
struct Transaction {
    #[serde(rename = "type")]
    tx_type: TransactionType,
    client: ClientId,
    tx: TransactionId,
    #[serde(deserialize_with = "deserialize_fixed")]
    amount: Amount,
}

/// Record format to match specification. Many of the fields can be derived from working state.
struct OutputRecord {
    client: ClientId,
    available: Amount,
    held: Amount,
    locked: bool,
}

/// Working account state. ID is used to locate this and not duplicated internally.
#[derive(Default)]
struct Account {
    locked: bool,
    total: Amount,
    // Spec refers to available, but held will be mutated less. Might be faster, not fussed.
    held: Amount,
}

impl Account {
    fn available(&self) -> Amount {
        self.total - self.held
    }
}

/// Transaction state required for future transactions. We only need the amount for now.
type TransactionHistory = HashMap<TransactionId, Amount>;

struct Engine {
    txs: TransactionHistory,
    accounts: [Account; 1_usize << ClientId::BITS],
}

impl Engine {
    fn new() -> Self {
        Self {
            accounts: std::array::from_fn(|_| Default::default()),
            txs: Default::default(),
        }
    }
}

fn process_transaction(
    accounts: &mut [Account],
    record: Transaction,
    get_tx_amount: impl Fn(&TransactionId) -> Option<Amount>,
) {
    use TransactionType::*;
    let account = &mut accounts[record.client as usize];
    match record.tx_type {
        Deposit => {
            account.total += record.amount;
        }
        Withdrawal => {
            if account.available() >= record.amount {
                account.total -= record.amount;
            }
        }
        Dispute => {
            if let Some(amount) = get_tx_amount(&record.tx) {
                account.held += amount;
            }
        }
        Resolve => {
            if let Some(amount) = get_tx_amount(&record.tx) {
                account.held -= amount;
            }
        }
        Chargeback => {
            if let Some(amount) = get_tx_amount(&record.tx) {
                account.held -= amount;
                account.total -= amount;
                account.locked = true;
            }
        }
    }
}

fn get_tx_amount(history: &TransactionHistory, id: &TransactionId) -> Option<Amount> {
    history.get(id).copied()
}

fn main() -> Result<()> {
    let args = Args::parse();
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .trim(csv::Trim::All)
        .from_path(args.filename)?;
    let mut engine = Engine::new();
    for record in reader.deserialize() {
        let record: Transaction = record?;
        println!("{:?}", record);
        process_transaction(&mut engine.accounts, record, |id| {
            get_tx_amount(&engine.txs, id)
        });
    }
    Ok(())
}

fn deserialize_fixed<'de, D>(deserializer: D) -> Result<Amount, D::Error>
where
    D: serde::Deserializer<'de>,
{
    // Deserialize to string first. TODO: See if we can avoid this allocation.
    let s: String = String::deserialize(deserializer)?;

    // Parse the string to fixed point. TODO: Check if excess precision should be an error. Without
    // this we lose precision.
    let value = Amount::from_str(&s).map_err(serde::de::Error::custom)?;

    Ok(value)
}
