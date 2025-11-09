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
use tracing::{info, warn};
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
    nonces: Arc<Mutex<std::collections::HashMap<String, u64>>>, // for signing demo
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

// Signed order support (feature gated for signing)
#[cfg(feature = "signing")]
use ethers::core::types::{Address as EthAddress, Signature, H256};
#[cfg(feature = "signing")]
use ethers::types::transaction::eip712::{TypedData, Eip712};

#[cfg(feature = "signing")]
#[derive(Debug, Clone, Deserialize)]
struct SignedOrder {
    trader: EthAddress,
    side: String,
    price: i128,
    qty: i128,
    leverage: u32,
    ttl_secs: u64,
    is_limit: bool,
    nonce: u64,
    // hex signature (65 bytes r,s,v) 
}

#[cfg(feature = "signing")]
#[derive(Debug, Deserialize)]
struct SignedOrderReq { order: SignedOrder, signature: String }

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_env_filter("info").init();
    // Serve static files from this crate's static/ folder regardless of process CWD
    let static_dir = ServeDir::new(concat!(env!("CARGO_MANIFEST_DIR"), "/static"));
    // Build shared app state first so we can run background tasks (oracle jitter)
    let app_state = AppState { 
            orderbook: Default::default(),
            accounts: Default::default(),
            positions: Default::default(),
            oracle: Arc::new(Mutex::new(OraclePrice{ price:100, conf:0, ts:0 })),
            fee_bps: Arc::new(Mutex::new((2,5))),
            chain: ChainClient::new(std::env::var("CONTRACT_ADDRESS").ok()),
        nonces: Default::default(),
        };
    // Background: simple price jitter for $singu to mimic a live feed
    {
        let st = app_state.clone();
        tokio::spawn(async move {
            let mut dir: i128 = 1; // up or down
            let mut tick: u64 = 0;
            loop {
                {
                    
                    let m = &st.oracle;
                    let mut o = match m.lock() { Ok(g) => g, Err(e) => { warn!(target="arbz","Recovered from poisoned mutex: oracle"); e.into_inner() } };
                    // simple 
                    let step = 1 + ((tick % 3) as i128); // 1..3
                    let next = o.price + dir * step;
                    let clamped = next.clamp(50, 150);
                    o.price = clamped;
                    o.ts = o.ts.saturating_add(1);
                    // occasionally flip direction
                    if tick % 7 == 0 || clamped == 50 || clamped == 150 { dir = -dir; }
                    tick = tick.wrapping_add(1);
                }
                tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
            }
        });
    }
   
    let app = {
        let r = Router::new()
            .route("/orders", post(place_order))
            .route("/ws", get(ws))
            .route("/deposit", post(deposit))
            .route("/withdraw", post(withdraw))
            .route("/oracle", post(update_oracle))
            .route("/fees", post(update_fees))
            .route("/status", get(status))
            .route("/state", get(get_state));
        #[cfg(feature = "signing")]
        let r = r.route("/orders/signed", post(place_signed_order));
        #[cfg(not(feature = "signing"))]
        let r = r;
        r
            .nest_service("/", get_service(static_dir).handle_error(|e| async move {
                (StatusCode::INTERNAL_SERVER_ERROR, format!("static error: {}", e))
            }))
            .with_state(app_state)
    };
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

#[cfg(feature = "signing")]
async fn place_signed_order(State(state): State<AppState>, Json(req): Json<SignedOrderReq>) -> Response {
    // 1. Check nonce
    {
        let mut nonces = state.nonces.lock().unwrap();
        let cur = nonces.get(&format!("{:?}", req.order.trader)).cloned().unwrap_or(0);
        if req.order.nonce != cur { return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error":"bad nonce","expected":cur}))).into_response(); }
        nonces.insert(format!("{:?}", req.order.trader), cur + 1);
    }
    // 2. Recreate digest per EIP-712 using TypedData
    let td_json = serde_json::json!({
        "types": {
            "EIP712Domain": [
                {"name":"name","type":"string"},
                {"name":"version","type":"string"},
                {"name":"chainId","type":"uint256"},
                {"name":"verifyingContract","type":"address"}
            ],
            "SignedOrder": [
                {"name":"trader","type":"address"},
                {"name":"side","type":"string"},
                {"name":"price","type":"int128"},
                {"name":"qty","type":"int128"},
                {"name":"leverage","type":"uint32"},
                {"name":"ttl_secs","type":"uint64"},
                {"name":"is_limit","type":"bool"},
                {"name":"nonce","type":"uint64"}
            ]
        },
        "primaryType": "SignedOrder",
        "domain": {
            "name":"ArbzZeroDay","version":"1","chainId":421614,
            "verifyingContract":"0x0000000000000000000000000000000000000000"
        },
        "message": {
            "trader": format!("{:?}", req.order.trader),
            "side": req.order.side,
            "price": req.order.price,
            "qty": req.order.qty,
            "leverage": req.order.leverage,
            "ttl_secs": req.order.ttl_secs,
            "is_limit": req.order.is_limit,
            "nonce": req.order.nonce
        }
    });
    let typed: TypedData = match serde_json::from_value(td_json) { Ok(v) => v, Err(_) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error":"typed data"}))).into_response() };
    let digest: H256 = match typed.encode_eip712() { Ok(h) => H256::from(h), Err(_) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error":"encode failed"}))).into_response() };
    // 3. Parse signature using ethers::core::types::Signature
    let sig_bytes = match hex::decode(req.signature.trim_start_matches("0x")) { Ok(b) => b, Err(_) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error":"bad sig hex"}))).into_response() };
    if sig_bytes.len() != 65 { return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error":"len"}))).into_response(); }
    let sig = match Signature::try_from(sig_bytes.as_slice()) { Ok(s) => s, Err(_) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error":"sig parse"}))).into_response() };
    let recovered_addr = match sig.recover(digest) { Ok(a) => a, Err(_) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error":"recover"}))).into_response() };
    if recovered_addr != req.order.trader { return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error":"signature mismatch"}))).into_response(); }
    // 4. Convert to internal PlaceOrderReq and delegate
    let inner = PlaceOrderReq { trader: format!("{:?}", req.order.trader), side: req.order.side.clone(), price: req.order.price, qty: req.order.qty, leverage: req.order.leverage, ttl_secs: req.order.ttl_secs, is_limit: req.order.is_limit };
    place_order(State(state), Json(inner)).await.into_response()
}

async fn ws(State(state): State<AppState>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(|socket| async move { handle_ws(state, socket).await })
}

async fn handle_ws(state: AppState, mut socket: WebSocket) {
    // Track last price sent to avoid spamming identical oracle events
    let mut last_mark: Option<i128> = None;
    // local helper to recover from poisoned mutexes without panicking
    fn lock<'a, T>(m: &'a Mutex<T>, name: &str) -> std::sync::MutexGuard<'a, T> {
        match m.lock() {
            Ok(g) => g,
            Err(e) => {
                warn!(target = "arbz", "Recovered from poisoned mutex: {}", name);
                e.into_inner()
            }
        }
    }
    loop {
        // Always read current oracle price; if changed, emit an oracle tick event
        let current_mark = { lock(&state.oracle, "oracle").price };
        if last_mark.map(|p| p != current_mark).unwrap_or(true) {
            let tick = serde_json::json!({
                "event": "oracle",
                "symbol": "$singu",
                "price": current_mark
            });
            if socket.send(Message::Text(tick.to_string())).await.is_err() { break; }
            last_mark = Some(current_mark);
        }
        let (buy_opt, sell_opt) = {
            let ob = lock(&state.orderbook, "orderbook");
            (ob.buys.front().cloned(), ob.sells.front().cloned())
        };
        if let (Some((buy_id, buy)), Some((sell_id, sell))) = (buy_opt, sell_opt) {
            let price = (buy.price + sell.price) / 2;
            let qty = buy.qty.min(sell.qty);
            // fee calc and position update (toy)
            let (maker_bps, taker_bps) = *lock(&state.fee_bps, "fee_bps");
            let notional = (price.abs() as i128) * (qty.abs() as i128);
            let maker_fee = notional * maker_bps as i128 / 10_000;
            let taker_fee = notional * taker_bps as i128 / 10_000;
            // book-keeping to accounts and positions (do not hold locks across await)
            {
                let mut accts = lock(&state.accounts, "accounts");
                accts.entry(buy.trader.clone()).and_modify(|a| a.collateral -= taker_fee).or_insert(Account{collateral: -taker_fee, locked_margin:0});
                accts.entry(sell.trader.clone()).and_modify(|a| a.collateral -= maker_fee).or_insert(Account{collateral: -maker_fee, locked_margin:0});
            }
            {
                let mut pos = lock(&state.positions, "positions");
                // buyer long +qty at price
                let pb = pos.entry(buy.trader.clone()).or_insert(Position{ trader: buy.trader.clone(), entry_price: price, qty: 0, leverage: buy.leverage, margin: 0, opened_ts: 0, expiry_ts: 86_400 });
                let new_qty_b = pb.qty + qty;
                if new_qty_b == 0 {
                    pb.entry_price = 0; // flat position
                    pb.qty = 0;
                } else if pb.qty == 0 {
                    pb.entry_price = price;
                    pb.qty = new_qty_b;
                } else {
                    // weighted average price
                    pb.entry_price = (pb.entry_price * pb.qty + price * qty) / new_qty_b;
                    pb.qty = new_qty_b;
                }
                // seller short -qty at price
                let ps = pos.entry(sell.trader.clone()).or_insert(Position{ trader: sell.trader.clone(), entry_price: price, qty: 0, leverage: sell.leverage, margin: 0, opened_ts: 0, expiry_ts: 86_400 });
                let new_qty_s = ps.qty - qty;
                if new_qty_s == 0 {
                    ps.entry_price = 0;
                    ps.qty = 0;
                } else if ps.qty == 0 {
                    ps.entry_price = price;
                    ps.qty = new_qty_s;
                } else {
                    ps.entry_price = (ps.entry_price * ps.qty + price * (-qty)) / new_qty_s;
                    ps.qty = new_qty_s;
                }
            }
            #[allow(unused_mut)]
            let mut obj = serde_json::json!({"event":"match","price":price,"qty":qty,"buy_trader":buy.trader,"sell_trader":sell.trader,"maker_fee":maker_fee,"taker_fee":taker_fee,"buy_id":buy_id,"sell_id":sell_id});
            // in on-chain mode, only match when chain is active and call succeeds; otherwise keep orders queued
            #[allow(unused_mut)]
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
                // here pop from the book only after successful match 
                {
                    let mut ob = lock(&state.orderbook, "orderbook");
                    ob.buys.pop_front();
                    ob.sells.pop_front();
                }
                if socket.send(Message::Text(obj.to_string())).await.is_err() { break; }
            } else {
                // chain inactive or match failed, retry?
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                continue;
            }

            // simple liquidation checks for both traders using current oracle price
            let mark = current_mark;
            for who in [buy.trader, sell.trader] {
                let (qty_w, entry_w) = {
                    let pos = lock(&state.positions, "positions");
                    if let Some(p) = pos.get(&who) { (p.qty, p.entry_price) } else { (0, 0) }
                };
                if qty_w != 0 {
                    let pnl = (mark - entry_w) * qty_w; // here short if qty negative
                    let (collateral, locked) = {
                        let ac = lock(&state.accounts, "accounts");
                        if let Some(a) = ac.get(&who) { (a.collateral, a.locked_margin) } else { (0,0) }
                    };
                    if locked > 0 {
                        let equity = collateral + pnl - locked;
                        let health_bps = if locked == 0 { i128::MAX } else { (equity * 10_000) / locked };
                        if health_bps < 5_000 { // thrhdolf 50%
                            
                            {
                                let mut ac = lock(&state.accounts, "accounts");
                                if let Some(a) = ac.get_mut(&who) { a.collateral += pnl; a.locked_margin = 0; }
                            }
                            {
                                let mut pos = lock(&state.positions, "positions");
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
    fn lock<'a, T>(m: &'a Mutex<T>, name: &str) -> std::sync::MutexGuard<'a, T> { match m.lock() { Ok(g) => g, Err(e) => { warn!(target="arbz","Recovered from poisoned mutex: {}", name); e.into_inner() } } }
    let mut a = lock(&state.accounts, "accounts");
    a.entry(req.trader).and_modify(|x| x.collateral += req.amount).or_insert(Account{collateral:req.amount, locked_margin:0});
    Json(serde_json::json!({"ok":true}))
}

async fn withdraw(State(state): State<AppState>, Json(req): Json<WithdrawReq>) -> impl IntoResponse {
    fn lock<'a, T>(m: &'a Mutex<T>, name: &str) -> std::sync::MutexGuard<'a, T> { match m.lock() { Ok(g) => g, Err(e) => { warn!(target="arbz","Recovered from poisoned mutex: {}", name); e.into_inner() } } }
    let mut a = lock(&state.accounts, "accounts");
    let ok = if let Some(acc) = a.get_mut(&req.trader) { if acc.collateral - acc.locked_margin >= req.amount { acc.collateral -= req.amount; true } else { false } } else { false };
    Json(serde_json::json!({"ok":ok}))
}

async fn update_oracle(State(state): State<AppState>, Json(req): Json<OracleUpdateReq>) -> impl IntoResponse {
    fn lock<'a, T>(m: &'a Mutex<T>, name: &str) -> std::sync::MutexGuard<'a, T> { match m.lock() { Ok(g) => g, Err(e) => { warn!(target="arbz","Recovered from poisoned mutex: {}", name); e.into_inner() } } }
    let mut o = lock(&state.oracle, "oracle");
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
    fn lock<'a, T>(m: &'a Mutex<T>, name: &str) -> std::sync::MutexGuard<'a, T> { match m.lock() { Ok(g) => g, Err(e) => { warn!(target="arbz","Recovered from poisoned mutex: {}", name); e.into_inner() } } }
    *lock(&state.fee_bps, "fee_bps") = (req.maker_bps, req.taker_bps);
    Json(serde_json::json!({"ok":true}))
}

async fn status(State(_state): State<AppState>) -> impl IntoResponse {
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

#[derive(Serialize)]
struct TraderView {
    trader: String,
    collateral: i64,
    locked_margin: i64,
    qty: i64,
    entry_price: i64,
    pnl: i64,
    health_bps: Option<i64>,
    nonce: u64,
}

fn clamp_i128_to_i64(v: i128) -> i64 {
    if v > i64::MAX as i128 { i64::MAX } else if v < i64::MIN as i128 { i64::MIN } else { v as i64 }
}

fn compute_health_and_pnl(acc: &Account, pos: Option<&Position>, mark: i128) -> (i64, Option<i64>) {
    let (qty, entry) = if let Some(p) = pos { (p.qty, p.entry_price) } else { (0,0) };
    let pnl_i128 = (mark - entry) * qty;
    let equity_i128 = acc.collateral + pnl_i128 - acc.locked_margin;
    let pnl = clamp_i128_to_i64(pnl_i128);
    let health_bps = if acc.locked_margin == 0 {
        None
    } else {
        let hbps_i128 = (equity_i128 * 10_000) / acc.locked_margin;
        Some(clamp_i128_to_i64(hbps_i128))
    };
    (pnl, health_bps)
}

async fn get_state(State(state): State<AppState>) -> impl IntoResponse {
    fn lock<'a, T>(m: &'a Mutex<T>, name: &str) -> std::sync::MutexGuard<'a, T> { match m.lock() { Ok(g) => g, Err(e) => { warn!(target="arbz","Recovered from poisoned mutex: {}", name); e.into_inner() } } }
    let mark = { lock(&state.oracle, "oracle").price };
    let accounts = lock(&state.accounts, "accounts");
    let positions = lock(&state.positions, "positions");
    let nonces = lock(&state.nonces, "nonces");
    let mut out: Vec<TraderView> = Vec::new();
    for (tr, acc) in accounts.iter() {
        let pos = positions.get(tr);
        let (pnl, hbps) = compute_health_and_pnl(acc, pos, mark);
        let (qty_i128, entry_i128) = pos.map(|p| (p.qty, p.entry_price)).unwrap_or((0,0));
        let qty = clamp_i128_to_i64(qty_i128);
        let entry_price = clamp_i128_to_i64(entry_i128);
        let nonce = *nonces.get(tr).unwrap_or(&0);
        out.push(TraderView{
            trader: tr.clone(),
            collateral: clamp_i128_to_i64(acc.collateral),
            locked_margin: clamp_i128_to_i64(acc.locked_margin),
            qty,
            entry_price,
            pnl,
            health_bps: hbps,
            nonce,
        });
    }
    Json(serde_json::json!({"mark":mark, "traders": out}))
}
