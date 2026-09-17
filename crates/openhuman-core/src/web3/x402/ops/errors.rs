//! The x402 client's error type.

#[derive(Debug)]
pub enum X402Error {
    Transport(reqwest::Error),
    NoPaymentHeader,
    NoPaymentOption,
    AmountExceedsCap {
        requested: u64,
        cap: u64,
    },
    BudgetExceeded {
        period: &'static str,
        current: u64,
        cap: u64,
    },
    Protocol(String),
    Wallet(String),
}

impl std::fmt::Display for X402Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(e) => write!(f, "x402 transport: {e}"),
            Self::NoPaymentHeader => write!(f, "402 response missing PAYMENT-REQUIRED header"),
            Self::NoPaymentOption => {
                write!(
                    f,
                    "no supported payment option (Solana exact or EVM exact) in 402 challenge"
                )
            }
            Self::AmountExceedsCap { requested, cap } => {
                write!(f, "x402 amount {requested} exceeds per-request cap {cap}")
            }
            Self::BudgetExceeded {
                period,
                current,
                cap,
            } => {
                write!(
                    f,
                    "x402 {period} budget exceeded: {current}/{cap} atomic units"
                )
            }
            Self::Protocol(msg) => write!(f, "x402 protocol: {msg}"),
            Self::Wallet(msg) => write!(f, "x402 wallet: {msg}"),
        }
    }
}

impl std::error::Error for X402Error {}
