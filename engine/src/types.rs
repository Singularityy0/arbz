use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum Side { Buy, Sell }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Order {
    pub trader: String,
    pub side: Side,
    pub price: i128, // price in 1e6 (USD 6 decimals)
    pub qty: i128,   // base units
    pub leverage: u32,
    pub ts: u64,
    pub expiry_ts: u64,
    pub is_limit: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Position {
    pub trader: String,
    pub entry_price: i128,
    pub qty: i128,
    pub leverage: u32,
    pub margin: i128,
    pub opened_ts: u64,
    pub expiry_ts: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Account {
    pub collateral: i128,
    pub locked_margin: i128,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OraclePrice {
    pub price: i128, // 1e6
    pub conf: u64,
    pub ts: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TradeExecution {
    pub price: i128,
    pub qty: i128,
    pub fee: i128,
}
