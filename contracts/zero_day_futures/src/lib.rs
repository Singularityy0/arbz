//! Minimal demo Stylus contract for Zero Day Futures Platform.
//! Focus: collateral vault, order storage, simple matching, settlement & liquidation stubs.

#![no_std]
extern crate alloc;
use alloc::{string::String, vec::Vec, collections::BTreeMap};
use stylus_sdk::{prelude::*, storage::{StorageMap, StorageU128, StorageU64, StorageBool}};
use engine::{Side, required_margin};

#[derive(SolidityError, Debug)]
pub enum ContractError {
    #[solidity_error("InsufficientCollateral")]
    InsufficientCollateral,
    #[solidity_error("OrderExpired")]
    OrderExpired,
    #[solidity_error("NotOwner")]
    NotOwner,
    #[solidity_error("Paused")]
    Paused,
}

#[derive(SolidityEvent)]
pub struct DepositEvent { #[solidity(indexed)] pub trader: Address, pub amount: u128 }
#[derive(SolidityEvent)]
pub struct WithdrawEvent { #[solidity(indexed)] pub trader: Address, pub amount: u128 }
#[derive(SolidityEvent)]
pub struct OrderPlaced { #[solidity(indexed)] pub trader: Address, pub id: u64 }
#[derive(SolidityEvent)]
pub struct TradeEvent { pub buy: Address, pub sell: Address, pub price: i128, pub qty: i128 }
#[derive(SolidityEvent)]
pub struct LiquidationEvent { #[solidity(indexed)] pub trader: Address, pub mark_price: i128 }
#[derive(SolidityEvent)]
pub struct FeeAccrued { pub maker_fee: u128, pub taker_fee: u128 }
#[derive(SolidityEvent)]
pub struct FeesWithdrawn { pub to: Address, pub amount: u128 }

#[derive(Clone)]
pub struct OrderData { pub trader: Address, pub side: Side, pub price: i128, pub qty: i128, pub leverage: u32, pub expiry_ts: u64 }

#[storage]
pub struct ZeroDayFutures {
    owner: Address,
    paused: StorageBool,
    // persistent counter for order ids
    next_order_id: StorageU64,
    collateral: StorageMap<Address, StorageU128>,
    locked_margin: StorageMap<Address, StorageU128>,
    orders: StorageMap<u64, OrderSlot>,
    // simplistic positions: net qty & avg entry price per trader
    position_qty: StorageMap<Address, i128>,
    position_entry: StorageMap<Address, i128>,
    position_margin: StorageMap<Address, StorageU128>,
    // oracle stub: product id => price (whole units) and timestamp
    oracle_price: StorageMap<u64, i128>,
    oracle_ts: StorageMap<u64, u64>,
    default_expiry_secs: StorageU128,
    liquidation_threshold_bps: StorageU128, // e.g. 5000 = 50%
    maker_fee_bps: StorageU128,
    taker_fee_bps: StorageU128,
    accrued_fees: StorageU128,
}

#[derive(Clone)]
pub struct OrderSlot { pub exists: bool, pub data: OrderData }

impl ZeroDayFutures {
    pub fn init(&mut self, owner: Address) { 
        self.owner = owner; 
        self.default_expiry_secs.set(86_400); 
        self.liquidation_threshold_bps.set(5_000); 
        self.maker_fee_bps.set(2); // 0.02%
        self.taker_fee_bps.set(5); // 0.05%
    }

    fn ensure_owner(&self) -> Result<(), ContractError> { if stylus_sdk::msg::sender() != self.owner { return Err(ContractError::NotOwner);} Ok(()) }
    fn ensure_not_paused(&self) -> Result<(), ContractError>{ if self.paused.get(){ return Err(ContractError::Paused);} Ok(()) }

    pub fn pause(&mut self) -> Result<(), ContractError> { self.ensure_owner()?; self.paused.set(true); Ok(()) }
    pub fn unpause(&mut self) -> Result<(), ContractError> { self.ensure_owner()?; self.paused.set(false); Ok(()) }

    pub fn deposit(&mut self) -> Result<(), ContractError> {
        self.ensure_not_paused()?;
        let amount = stylus_sdk::msg::value();
        let sender = stylus_sdk::msg::sender();
        let bal = self.collateral.get(&sender).unwrap_or_default();
        self.collateral.insert(sender, bal + amount);
        DepositEvent { trader: sender, amount }.emit();
        Ok(())
    }

    pub fn withdraw(&mut self, amount: u128) -> Result<(), ContractError> {
        self.ensure_not_paused()?;
        let sender = stylus_sdk::msg::sender();
        let bal = self.collateral.get(&sender).unwrap_or_default();
        let locked = self.locked_margin.get(&sender).unwrap_or_default();
        if bal < amount + locked { return Err(ContractError::InsufficientCollateral); }
        self.collateral.insert(sender, bal - amount);
        // transfer native token back (Stylus helper) - pseudo, actual transfer via msg::send
        stylus_sdk::msg::send(sender, amount);
        WithdrawEvent { trader: sender, amount }.emit();
        Ok(())
    }

    pub fn place_order(&mut self, side: u8, price: i128, qty: i128, leverage: u32) -> Result<u64, ContractError> {
        self.ensure_not_paused()?;
        let trader = stylus_sdk::msg::sender();
        let now = stylus_sdk::block::timestamp();
        let expiry = now + self.default_expiry_secs.get();
        // margin requirement (simplified) price scaled 1e8 assumed
        let margin = required_margin(qty, price, leverage) as u128;
        let free = self.collateral.get(&trader).unwrap_or_default() - self.locked_margin.get(&trader).unwrap_or_default();
        if free < margin { return Err(ContractError::InsufficientCollateral); }
        let locked = self.locked_margin.get(&trader).unwrap_or_default();
        self.locked_margin.insert(trader, locked + margin);
    let id = self.next_order_id.get() + 1; self.next_order_id.set(id);
        let data = OrderData { trader, side: if side==0 { Side::Buy } else { Side::Sell }, price, qty, leverage, expiry_ts: expiry };
    self.orders.insert(id, OrderSlot{ exists: true, data: data.clone()});
    OrderPlaced { trader, id }.emit();
    Ok(id)
    }

    pub fn match_orders(&mut self, buy_id: u64, sell_id: u64, price: i128) -> Result<(), ContractError> {
        self.ensure_not_paused()?;
        let now = stylus_sdk::block::timestamp();
        let buy = self.orders.get(&buy_id).ok_or(ContractError::OrderExpired)?;
        let sell = self.orders.get(&sell_id).ok_or(ContractError::OrderExpired)?;
        if now > buy.data.expiry_ts || now > sell.data.expiry_ts { return Err(ContractError::OrderExpired); }
        // adjust positions (simplified netting)
        let qty = core::cmp::min(buy.data.qty.abs(), sell.data.qty.abs());
        self.apply_fill(&buy.data, price, qty);
        self.apply_fill(&sell.data, price, qty);
        // fee calc (simplified: maker = order with older id)
        let maker_is_buy = buy_id < sell_id; // naive heuristic
        let notional = (price.abs() as u128) * (qty.abs() as u128);
        let maker_fee = notional * self.maker_fee_bps.get() / 10_000;
        let taker_fee = notional * self.taker_fee_bps.get() / 10_000;
        let total = maker_fee + taker_fee;
        let accrued = self.accrued_fees.get();
        self.accrued_fees.set(accrued + total);
        FeeAccrued { maker_fee, taker_fee }.emit();
        TradeEvent { buy: buy.data.trader, sell: sell.data.trader, price, qty }.emit();
        // remove orders for demo
        self.orders.remove(&buy_id);
        self.orders.remove(&sell_id);
        Ok(())
    }

    fn apply_fill(&mut self, order: &OrderData, price: i128, qty: i128) {
        let pos_qty = self.position_qty.get(&order.trader).unwrap_or_default();
        let entry = self.position_entry.get(&order.trader).unwrap_or_default();
        let new_qty = if matches!(order.side, Side::Buy) { pos_qty + qty } else { pos_qty - qty };
        let new_entry = if pos_qty == 0 { price } else { (entry * pos_qty + price * qty) / (pos_qty + qty) }; // naive
        self.position_qty.insert(order.trader, new_qty);
        self.position_entry.insert(order.trader, new_entry);
    }

    pub fn settle_expired(&mut self, trader: Address, mark_price: i128) {
        // simplistic immediate settle and free margin
        let qty = self.position_qty.get(&trader).unwrap_or_default();
        if qty == 0 { return; }
        let entry = self.position_entry.get(&trader).unwrap_or_default();
        let pnl = (mark_price - entry) * qty; // price & qty whole units for demo
        let coll = self.collateral.get(&trader).unwrap_or_default() as i128 + pnl;
        self.collateral.insert(trader, if coll<0 {0} else {coll as u128});
        self.position_qty.insert(trader, 0);
        self.locked_margin.insert(trader, 0); // release margin post settlement
    }

    fn margin_health(&self, trader: Address, mark_price: i128) -> u128 {
        let coll = self.collateral.get(&trader).unwrap_or_default() as i128;
        let locked = self.locked_margin.get(&trader).unwrap_or_default() as i128;
        let qty = self.position_qty.get(&trader).unwrap_or_default();
        if locked == 0 { return u128::MAX; }
        let entry = self.position_entry.get(&trader).unwrap_or_default();
        let pnl = (mark_price - entry) * qty;
        let equity = coll + pnl - locked;
        if equity <= 0 { return 0; }
        // return basis points equity/locked
        ((equity * 10_000) / locked) as u128
    }

    pub fn try_liquidate(&mut self, trader: Address, mark_price: i128) {
        let health_bps = self.margin_health(trader, mark_price);
        if health_bps < self.liquidation_threshold_bps.get() {
            self.settle_expired(trader, mark_price);
            LiquidationEvent { trader, mark_price }.emit();
        }
    }

    pub fn batch_liquidate(&mut self, traders: Vec<Address>, mark_price: i128) {
        for t in traders.into_iter() { self.try_liquidate(t, mark_price); }
    }

    pub fn set_fees(&mut self, maker_bps: u128, taker_bps: u128) -> Result<(), ContractError> { self.ensure_owner()?; self.maker_fee_bps.set(maker_bps); self.taker_fee_bps.set(taker_bps); Ok(()) }
    pub fn withdraw_fees(&mut self, to: Address, amount: u128) -> Result<(), ContractError> { self.ensure_owner()?; let acc = self.accrued_fees.get(); let a = if amount>acc {acc} else {amount}; self.accrued_fees.set(acc - a); stylus_sdk::msg::send(to, a); FeesWithdrawn{ to, amount:a }.emit(); Ok(()) }

    pub fn update_oracle_price(&mut self, product_id: u64, price: i128) -> Result<(), ContractError> {
        self.ensure_owner()?; // access control
        let now = stylus_sdk::block::timestamp();
        self.oracle_price.insert(product_id, price);
        self.oracle_ts.insert(product_id, now);
        Ok(())
    }
}

#[external]
impl ZeroDayFutures {
    pub fn ext_init(&mut self) { self.init(stylus_sdk::msg::sender()); }
    pub fn ext_deposit(&mut self) -> Result<(), ContractError> { self.deposit() }
    pub fn ext_withdraw(&mut self, amount: u128) -> Result<(), ContractError> { self.withdraw(amount) }
    pub fn ext_place_order(&mut self, side: u8, price: i128, qty: i128, leverage: u32) -> Result<u64, ContractError> { self.place_order(side, price, qty, leverage) }
    pub fn ext_match(&mut self, buy_id: u64, sell_id: u64, price: i128) -> Result<(), ContractError> { self.match_orders(buy_id, sell_id, price) }
    pub fn ext_liquidate(&mut self, trader: Address, mark_price: i128) { self.try_liquidate(trader, mark_price) }
    pub fn ext_update_oracle(&mut self, product_id: u64, price: i128) -> Result<(), ContractError> { self.update_oracle_price(product_id, price) }
    pub fn ext_batch_liquidate(&mut self, traders: Vec<Address>, mark_price: i128) { self.batch_liquidate(traders, mark_price) }
    pub fn ext_set_fees(&mut self, maker_bps: u128, taker_bps: u128) -> Result<(), ContractError> { self.set_fees(maker_bps, taker_bps) }
    pub fn ext_withdraw_fees(&mut self, to: Address, amount: u128) -> Result<(), ContractError> { self.withdraw_fees(to, amount) }
}
