use axum::{extract::State, routing::{get, post}, Json, Router};
use axum::http::StatusCode;
use axum::routing::get_service;
use tower_http::services::ServeDir;
use axum::response::IntoResponse;
use axum::extract::WebSocketUpgrade;
use axum::response::Response;
use axum::extract::ws::{Message, WebSocket};
use engine::{Order, Side, Account, Position, OraclePrice};
use serde::{Deserialize, Serialize};
use std::{collections::VecDeque, sync::{Arc, Mutex}};
use tokio::net::TcpListener;
use tracing::{info};
mod chain;
use chain::ChainClient;

#[derive(Clone)]
struct AppState { 
    orderbook: Arc<Mutex<OrderBook>>,
    accounts: Arc<Mutex<std::collections::HashMap<String, Account>>>,
    positions: Arc<Mutex<std::collections::HashMap<String, Position>>>,
    oracle: Arc<Mutex<OraclePrice>>, // single-product demo
    fee_bps: Arc<Mutex<(u64,u64)>>, // (maker, taker)
    chain: ChainClient,
}

#[derive(Default)]
struct OrderBook {
    // store (on_chain_id, order)
    buys: VecDeque<(u64, Order)>,
    sells: VecDeque<(u64, Order)>,
}

#[derive(Debug, Deserialize)]
struct PlaceOrderReq { trader: String, side: String, price: i128, qty: i128, leverage: u32, ttl_secs: u64, is_limit: bool }
#[derive(Debug, Deserialize)]
struct DepositReq { trader: String, amount: i128 }

#[derive(Debug, Deserialize)]
struct WithdrawReq { trader: String, amount: i128 }

#[derive(Debug, Deserialize)]
struct OracleUpdateReq { price: i128 }

#[derive(Debug, Deserialize)]
struct FeeCfgReq { maker_bps: u64, taker_bps: u64 }


#[derive(Debug, Serialize)]
struct PlaceOrderResp { id: u64, tx: Option<String> }

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_env_filter("info").init();
    // Serve static files from this crate's static/ folder regardless of process CWD
    let static_dir = ServeDir::new(concat!(env!("CARGO_MANIFEST_DIR"), "/static"));
    let app = Router::new()
        .route("/orders", post(place_order))
        .route("/ws", get(ws))
        .route("/deposit", post(deposit))
        .route("/withdraw", post(withdraw))
        .route("/oracle", post(update_oracle))
        .route("/fees", post(update_fees))
        .route("/status", get(status))
        .nest_service("/", get_service(static_dir).handle_error(|e| async move {
            (StatusCode::INTERNAL_SERVER_ERROR, format!("static error: {}", e))
        }))
        .with_state(AppState { 
            orderbook: Default::default(),
            accounts: Default::default(),
            positions: Default::default(),
            oracle: Arc::new(Mutex::new(OraclePrice{ price:100, conf:0, ts:0 })),
            fee_bps: Arc::new(Mutex::new((2,5))),
            chain: ChainClient::new(std::env::var("CONTRACT_ADDRESS").ok()),
        });
    let addr = "0.0.0.0:8787";
    let listener = TcpListener::bind(addr).await.unwrap();
    info!("Listening on {}", addr);
    axum::serve(listener, app).await.unwrap();
}

async fn place_order(State(state): State<AppState>, Json(req): Json<PlaceOrderReq>) -> impl IntoResponse {
    let side = if req.side.eq_ignore_ascii_case("buy") { Side::Buy } else { Side::Sell };
    let now = 0u64; // demo placeholder
    let exp = now + req.ttl_secs;
    let trader = req.trader.clone();
    let order = Order { trader: trader.clone(), side, price: req.price, qty: req.qty, leverage: req.leverage, ts: now, expiry_ts: exp, is_limit: req.is_limit };
    // by default create a local id; if on-chain returns an id, replace it
    #[allow(unused_mut)]
    let mut onchain_id: Option<u64> = None;
    #[allow(unused_mut)]
    let mut onchain_tx: Option<String> = None;
    // lock margin for this order (simple: notional/leverage)
    {
        let mut accts = state.accounts.lock().unwrap();
        let notional = (req.price.abs() as i128) * (req.qty.abs() as i128);
        let margin = if req.leverage == 0 { notional } else { notional / (req.leverage as i128) };
    accts.entry(trader)
            .and_modify(|a| a.locked_margin += margin)
            .or_insert(Account{ collateral: 0, locked_margin: margin });
    }
    // if on-chain is active, synchronously fetch id to rely on it
    #[cfg(feature = "onchain")]
    if state.chain.is_active() {
        if let Ok(Some((oid, txh))) = state.chain.place_order(if req.side.eq_ignore_ascii_case("buy") {0} else {1}, req.price, req.qty, req.leverage).await {
            onchain_id = Some(oid);
            onchain_tx = Some(txh);
        }
    }
    let final_id = onchain_id.unwrap_or_else(|| {
        // fallback local id if on-chain inactive or failed
        let ob = state.orderbook.lock().unwrap();
        let next = (ob.buys.len() + ob.sells.len() + 1) as u64;
        next
    });
    // push into book with the on-chain id (or fallback local id)
    {
        let mut ob = state.orderbook.lock().unwrap();
        if matches!(order.side, Side::Buy) { ob.buys.push_back((final_id, order)); } else { ob.sells.push_back((final_id, order)); }
    }
    Json(PlaceOrderResp { id: final_id, tx: onchain_tx })
}

async fn ws(State(state): State<AppState>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(|socket| async move { handle_ws(state, socket).await })
}

async fn handle_ws(state: AppState, mut socket: WebSocket) {
    loop {
        let (buy_opt, sell_opt) = {
            let ob = state.orderbook.lock().unwrap();
            (ob.buys.front().cloned(), ob.sells.front().cloned())
        };
        if let (Some((buy_id, buy)), Some((sell_id, sell))) = (buy_opt, sell_opt) {
            let price = (buy.price + sell.price) / 2;
            let qty = buy.qty.min(sell.qty);
            // fee calc and position update (toy)
            let (maker_bps, taker_bps) = *state.fee_bps.lock().unwrap();
            let notional = (price.abs() as i128) * (qty.abs() as i128);
            let maker_fee = notional * maker_bps as i128 / 10_000;
            let taker_fee = notional * taker_bps as i128 / 10_000;
            // book-keeping to accounts and positions (do not hold locks across await)
            {
                let mut accts = state.accounts.lock().unwrap();
                accts.entry(buy.trader.clone()).and_modify(|a| a.collateral -= taker_fee).or_insert(Account{collateral: -taker_fee, locked_margin:0});
                accts.entry(sell.trader.clone()).and_modify(|a| a.collateral -= maker_fee).or_insert(Account{collateral: -maker_fee, locked_margin:0});
            }
            {
                let mut pos = state.positions.lock().unwrap();
                // buyer long +qty at price
                let pb = pos.entry(buy.trader.clone()).or_insert(Position{ trader: buy.trader.clone(), entry_price: price, qty: 0, leverage: buy.leverage, margin: 0, opened_ts: 0, expiry_ts: 86_400 });
                let new_qty_b = pb.qty + qty;
                pb.entry_price = if pb.qty == 0 { price } else { (pb.entry_price * pb.qty + price * qty) / (new_qty_b) };
                pb.qty = new_qty_b;
                // seller short -qty at price
                let ps = pos.entry(sell.trader.clone()).or_insert(Position{ trader: sell.trader.clone(), entry_price: price, qty: 0, leverage: sell.leverage, margin: 0, opened_ts: 0, expiry_ts: 86_400 });
                let new_qty_s = ps.qty - qty;
                ps.entry_price = if ps.qty == 0 { price } else { (ps.entry_price * ps.qty + price * (-qty)) / (new_qty_s) };
                ps.qty = new_qty_s;
            }
            let mut obj = serde_json::json!({"event":"match","price":price,"qty":qty,"buy_trader":buy.trader,"sell_trader":sell.trader,"maker_fee":maker_fee,"taker_fee":taker_fee,"buy_id":buy_id,"sell_id":sell_id});
            // in on-chain mode, only match when chain is active and call succeeds; otherwise keep orders queued
            let mut matched_ok = true; // set false if on-chain fails
            #[cfg(feature = "onchain")]
            {
                if state.chain.is_active() {
                    match state.chain.match_orders(buy_id, sell_id, price).await {
                        Ok(Some(txh)) => { obj["tx"] = serde_json::json!(txh); matched_ok = true; }
                        Ok(None) => { matched_ok = false; }
                        Err(_) => { matched_ok = false; }
                    }
                } else {
                    matched_ok = false;
                }
            }
            if matched_ok {
                // pop from the book only after successful match (or when on-chain is not compiled)
                {
                    let mut ob = state.orderbook.lock().unwrap();
                    ob.buys.pop_front();
                    ob.sells.pop_front();
                }
                if socket.send(Message::Text(obj.to_string())).await.is_err() { break; }
            } else {
                // chain inactive or match failed; wait a bit and retry later without removing orders
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                continue;
            }

            // simple liquidation checks for both traders using current oracle price
            let mark = { state.oracle.lock().unwrap().price };
            for who in [buy.trader, sell.trader] {
                let (qty_w, entry_w) = {
                    let pos = state.positions.lock().unwrap();
                    if let Some(p) = pos.get(&who) { (p.qty, p.entry_price) } else { (0, 0) }
                };
                if qty_w != 0 {
                    let pnl = (mark - entry_w) * qty_w; // short if qty negative
                    let (collateral, locked) = {
                        let ac = state.accounts.lock().unwrap();
                        if let Some(a) = ac.get(&who) { (a.collateral, a.locked_margin) } else { (0,0) }
                    };
                    if locked > 0 {
                        let equity = collateral + pnl - locked;
                        let health_bps = if locked == 0 { i128::MAX } else { (equity * 10_000) / locked };
                        if health_bps < 5_000 { // threshold 50%
                            // settle: apply pnl to collateral, release margin, close pos
                            {
                                let mut ac = state.accounts.lock().unwrap();
                                if let Some(a) = ac.get_mut(&who) { a.collateral += pnl; a.locked_margin = 0; }
                            }
                            {
                                let mut pos = state.positions.lock().unwrap();
                                if let Some(p) = pos.get_mut(&who) { p.qty = 0; }
                            }
                            let lmsg = serde_json::json!({"event":"liquidation","trader":who,"mark":mark});
                            if socket.send(Message::Text(lmsg.to_string())).await.is_err() { break; }
                        }
                    }
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
}

async fn deposit(State(state): State<AppState>, Json(req): Json<DepositReq>) -> impl IntoResponse {
    let mut a = state.accounts.lock().unwrap();
    a.entry(req.trader).and_modify(|x| x.collateral += req.amount).or_insert(Account{collateral:req.amount, locked_margin:0});
    Json(serde_json::json!({"ok":true}))
}

async fn withdraw(State(state): State<AppState>, Json(req): Json<WithdrawReq>) -> impl IntoResponse {
    let mut a = state.accounts.lock().unwrap();
    let ok = if let Some(acc) = a.get_mut(&req.trader) { if acc.collateral - acc.locked_margin >= req.amount { acc.collateral -= req.amount; true } else { false } } else { false };
    Json(serde_json::json!({"ok":ok}))
}

async fn update_oracle(State(state): State<AppState>, Json(req): Json<OracleUpdateReq>) -> impl IntoResponse {
    let mut o = state.oracle.lock().unwrap();
    o.price = req.price; o.ts += 1;
    #[cfg(feature = "onchain")]
    {
        if state.chain.is_active() {
            let _ = tokio::spawn({ let cc = state.clone(); let p = req.price; async move { let _ = cc.chain.update_oracle(1, p).await; } });
        }
    }
    Json(serde_json::json!({"ok":true}))
}

async fn update_fees(State(state): State<AppState>, Json(req): Json<FeeCfgReq>) -> impl IntoResponse {
    *state.fee_bps.lock().unwrap() = (req.maker_bps, req.taker_bps);
    Json(serde_json::json!({"ok":true}))
}

async fn status(State(state): State<AppState>) -> impl IntoResponse {
    #[cfg(feature = "onchain")]
    {
        let active = state.chain.is_active();
        let addr = state.chain.contract_address.clone();
        return Json(serde_json::json!({"onchain_feature":true,"active":active,"contract_address":addr}));
    }
    #[cfg(not(feature = "onchain"))]
    {
        return Json(serde_json::json!({"onchain_feature":false}));
    }
}
