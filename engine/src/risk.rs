use crate::{Account, OraclePrice, Position};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RiskError {
    #[error("insufficient collateral: needed {needed}, have {have}")]
    InsufficientCollateral { needed: i128, have: i128 },
}

pub fn required_margin(qty: i128, price: i128, leverage: u32) -> i128 {
    // initial margin = notional / leverage ; price in whole units for demo
    let notional = qty.abs() * price.abs();
    (notional as i128) / (leverage as i128).max(1)
}

pub fn pnl_unrealized(pos: &Position, mark: &OraclePrice) -> i128 {
    let diff = mark.price - pos.entry_price;
    let pnl_per_unit = diff * pos.qty.signum();
    pnl_per_unit * pos.qty.abs()
}

pub fn margin_health(account: &Account, pos: Option<&Position>, mark: &OraclePrice) -> f64 {
    let equity = account.collateral
        - account.locked_margin
        + pos.map(|p| pnl_unrealized(p, mark)).unwrap_or(0);
    if account.locked_margin <= 0 { return f64::INFINITY; }
    equity as f64 / account.locked_margin as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Position;

    #[test]
    fn test_required_margin() {
        assert_eq!(required_margin(1_000, 100, 10), 10_000);
    }

    #[test]
    fn test_pnl_long_gain() {
        let p = Position{ trader:"t".into(), entry_price:100, qty:1_000, leverage:10, margin:10_000, opened_ts:0, expiry_ts:86_400};
        let m = OraclePrice{ price: 110, conf:0, ts:0};
        assert_eq!(pnl_unrealized(&p,&m), 10_000);
    }

    #[test]
    fn test_margin_health() {
        let acc = Account{ collateral: 20_000, locked_margin:10_000};
        let p = Position{ trader:"t".into(), entry_price:100, qty:1_000, leverage:10, margin:10_000, opened_ts:0, expiry_ts:86_400};
        let m = OraclePrice{ price: 100, conf:0, ts:0};
        assert_eq!(margin_health(&acc, Some(&p), &m), 1.0);
    }
}
