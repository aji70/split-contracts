use soroban_sdk::{contracttype, Address, Vec};

/// Status of an invoice lifecycle.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum InvoiceStatus {
    /// Invoice created, awaiting full payment.
    Pending,
    /// All shares paid; funds released to recipients.
    Released,
    /// Deadline passed before full funding; payers refunded.
    Refunded,
}

/// A single payment made toward an invoice.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Payment {
    /// Address of the payer.
    pub payer: Address,
    /// Amount paid in stroops (7 decimal places).
    pub amount: i128,
}

/// An on-chain invoice splitting payment among multiple recipients.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Invoice {
    /// Address that created the invoice.
    pub creator: Address,
    /// Ordered list of recipient addresses.
    pub recipients: Vec<Address>,
    /// Amounts owed to each recipient (parallel to `recipients`).
    pub amounts: Vec<i128>,
    /// USDC token contract address.
    pub token: Address,
    /// Unix timestamp after which unfunded invoices can be refunded.
    pub deadline: u64,
    /// Total amount collected so far.
    pub funded: i128,
    /// Current lifecycle status.
    pub status: InvoiceStatus,
    /// All payments made toward this invoice.
    pub payments: Vec<Payment>,
    /// Optional vesting duration in seconds. When set, recipients claim gradually.
    pub drip_duration: Option<u64>,
    /// Timestamp when the invoice was released (set by `_release` when drip is active).
    pub release_timestamp: Option<u64>,
    /// Amount already claimed by each recipient (parallel to `recipients`).
    pub claimed: Vec<i128>,
}
