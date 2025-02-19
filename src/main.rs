use anyhow::Result;
use clap::Parser;
use serde::Deserialize;

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

fn main() -> Result<()> {
    let args = Args::parse();
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .trim(csv::Trim::All)
        .from_path(args.filename)?;
    for record in reader.deserialize() {
        let record: Transaction = record?;
        println!("{:?}", record);
    }
    Ok(())
}

fn deserialize_fixed<'de, D>(deserializer: D) -> Result<Amount, D::Error>
where
    D: serde::Deserializer<'de>,
{
    // Deserialize to string first
    let s: String = String::deserialize(deserializer)?;

    // Parse the string to fixed point. TODO: Check if excess precision should be an error. Without
    // this we lose precision.
    let value = Amount::from_str(&s).map_err(serde::de::Error::custom)?;

    Ok(value)
}