use anyhow::Result;
use clap::Parser;
use serde::{Deserialize, Serialize};
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

// This could probably have custom Serialize/Deserialize implemented to avoid having to using
// {d,}serialize_with everywhere.
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
#[derive(Serialize)]
struct OutputRecord {
    client: ClientId,
    #[serde(serialize_with = "serialize_fixed")]
    available: Amount,
    #[serde(serialize_with = "serialize_fixed")]
    held: Amount,
    #[serde(serialize_with = "serialize_fixed")]
    total: Amount,
    locked: bool,
}

impl OutputRecord {
    fn from_account(account: Account, client: ClientId) -> Self {
        Self {
            client,
            available: account.available(),
            held: account.held,
            locked: account.locked,
            total: account.total,
        }
    }
}

/// Working account state. ID is used to locate this and not duplicated internally.
#[derive(Default, Clone,PartialEq)]
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
    fn unused(&self) -> bool {
        self == &Self::default()
    }
}

/// Transaction state required for future transactions. We only need the amount for now.
type TransactionHistory = HashMap<TransactionId, Amount>;

/// A lose collection of fields required to process transactions. I'd move more into the impl but we
/// need separate mutable and immutable references.
struct Engine {
    txs: TransactionHistory,
    // Fixed size array since client IDs are only 16 bit. I tried to use an array but Rust tried to
    // put it on the stack and overflowed it. It's fine on the heap anyway.
    accounts: Vec<Account>,
}

impl Engine {
    fn new() -> Self {
        Self {
            accounts: vec![Default::default(); 1_usize << ClientId::BITS],
            txs: Default::default(),
        }
    }
}

/// Apply transactions to set of accounts.
fn process_transaction(
    accounts: &mut [Account],
    record: Transaction,
    // I separated this out to avoid incompatible references to Engine. It's good abstraction anyway.
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

// Abstract over getting amount from a historical transaction.
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
        // Poke this to stderr for now, since automated tests probably check stdout. Left this in as
        // there's minimal debugging or logging in the project and it's not too noisy for now.
        eprintln!("{:?}", record);
        process_transaction(&mut engine.accounts, record, |id| {
            get_tx_amount(&engine.txs, id)
        });
    }
    let mut writer = csv::Writer::from_writer(std::io::stdout());
    for (client_id, account) in engine.accounts.into_iter().enumerate() {
        if account.unused() {
            continue;
        }
        let record = OutputRecord::from_account(account, client_id.try_into()?);
        writer.serialize(record)?;
    }
    Ok(())
}

// Helpers to serialize fixed integer Amounts as strings as expected. Looks like there is some
// features in the fixed crate that could maybe make this unnecessary.

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

// Serialization function
fn serialize_fixed<S>(fixed: &Amount, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    // Convert to string with desired precision
    let s = fixed.to_string();
    serializer.serialize_str(&s)
}
