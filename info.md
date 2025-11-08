# arbz: Zero‑Day Futures Demo — Finance, Tech, and Algorithms

This document explains the product concept, the financial mechanics (margin, PnL, fees, liquidations), the system architecture (on‑chain Stylus contract + off‑chain matcher), and the core algorithms used in this repository.

## TL;DR
- Product: A minimal zero‑day futures (ZDF) trading demo.
- On‑chain (Stylus): Custody vault, order storage, simple matching, fees, liquidation helpers, oracle setter, admin controls.
- Off‑chain (Axum server): REST + WebSocket API, in‑memory orderbook, matching loop, positions, fees, liquidation simulation, optional on‑chain submissions.
- Risk math: Simple margin calculation, unrealized PnL, margin health with basis points.

---

## Product Concept
- Instrument: Zero‑day futures on a single synthetic product (demo uses “price” with whole units for simplicity).
- Goal: Show end‑to‑end flow from deposit → order placement → matching → positions/PnL → liquidations, with a path to secure on‑chain settlement using Arbitrum Stylus.

Why zero‑day futures?
- Simpler operationally (short horizon, limited state), fast iteration, tight demonstration of core exchange mechanics.

---

## Units and Scaling
- Price: whole integer units in demo (e.g., `100`).
- Quantity (qty): integer signed; long > 0, short < 0 (demo consistently uses signed math for positions; orders use side + positive qty).
- Leverage: positive integer (e.g., `1`, `5`, `10`).
- Basis points (bps): 1 bps = 0.01%. Fee and health thresholds are expressed in bps.

---

## Margin, PnL, Health (Risk Engine)
Core formulas (see `engine/src/risk.rs`):

- Notional: $N = |\text{price}| \times |\text{qty}|$
- Required margin (simplified):
  $$ \text{margin}_\text{req} = \begin{cases}
  N, & \text{if leverage} = 0 \\
  \left\lfloor \dfrac{N}{\text{leverage}} \right\rfloor, & \text{otherwise}
  \end{cases} $$
- Unrealized PnL: $$ \text{PnL} = (\text{mark} - \text{entry}) \times \text{qty} $$
- Equity: $$ \text{equity} = \text{collateral} + \text{PnL} - \text{locked\_margin} $$
- Margin health (bps): $$ \text{health\_bps} = \begin{cases}
  +\infty, & \text{if locked\_margin} = 0 \\
  \left\lfloor \dfrac{\text{equity} \times 10{,}000}{\text{locked\_margin}} \right\rfloor, & \text{otherwise}
  \end{cases} $$

Edge cases handled:
- Zero leverage → margin equals notional (fully collateralized).
- Zero locked margin → health is effectively infinite (no position risk).
- Negative equity → health floored at 0.

In-contract threshold (demo): liquidation when `health_bps < 5000` (i.e., equity < 50% of locked margin).

---

## Fees
- Maker fee bps (default 2 = 0.02%).
- Taker fee bps (default 5 = 0.05%).
- Demo heuristic: older order ID is maker; the other is taker.
- Fee accrual: On‑chain contract adds `maker_fee + taker_fee` to `accrued_fees` and emits an event. Off‑chain demo deducts fees from account collateral and logs over WS.

---

## Order Lifecycle
1. Deposit collateral.
2. Place order (side, price, qty, leverage, TTL):
   - Locks required margin.
   - Assigns an order ID (on‑chain persistent counter or local fallback off‑chain ID).
3. Matching:
   - Buy/Sell at the front of each queue are paired.
   - Trade price = midpoint of buy and sell quotes (demo); trade qty = min of two.
   - Positions updated: net qty and average entry price.
   - Fees applied.
4. Oracle update (admin): sets mark price used for PnL & liquidation.
5. Liquidation check: if `health_bps` below threshold → settle & close.

---

## Matching Algorithm (Demo)
- Structure: FIFO queues per side.
- Pairing rule: front buy with front sell.
- Trade price: midpoint `((buy.price + sell.price) / 2)`.
- Trade quantity: `min(buy.qty, sell.qty)`.
- Maker: older order id.

Pseudocode (off‑chain loop):
```
while true:
  buy = buys.front(); sell = sells.front();
  if both exist:
    price = (buy.price + sell.price)/2
    qty   = min(buy.qty, sell.qty)
    apply_fees_and_update_positions(buy.trader, sell.trader, price, qty)
    if onchain_active:
      if ext_match(buy_id, sell_id, price) succeeds:
        pop both orders
        emit WS match event with tx
      else:
        keep orders (retry later)
    else:
      pop both orders (off-chain only)
      emit WS match event
  sleep 300ms
```

Complexity: O(1) per match (naive queues). Real orderbooks will require price‑time priority structures (heaps/trees) and crossing logic.

---

## Liquidation Algorithm (Demo)
- After each trade (and periodically), compute `health_bps` per trader using mark price.
- If below threshold (e.g., 5000 bps), close the position: apply PnL, release margin, set qty to 0, emit liquidation event.

Pseudocode:
```
for trader in traders_with_positions:
  pnl = (mark - entry)*qty
  equity = collateral + pnl - locked
  if locked > 0 and (equity * 10_000 / locked) < threshold:
    settle_position(trader, mark)
    emit liquidation
```

---

## Oracle (Demo Stub)
- Owner‑only method to set mark price (single product for now).
- Production: aggregate multiple sources, track staleness and deviation, and require signatures or on‑chain verified feeds.

---

## System Architecture

### On‑chain (Stylus contract)
Location: `contracts/zero_day_futures/src/lib.rs`

Responsibilities:
- Custody: `deposit()`, `withdraw()` (with checks against locked margin).
- Orders: `ext_place_order()` stores order and returns persistent ID (StorageU64 counter).
- Matching: `ext_match(buy_id, sell_id, price)` applies fills, updates net positions, accrues fees, emits events, removes orders.
- Liquidation helpers: `try_liquidate()`, `batch_liquidate()`.
- Oracle admin: `ext_update_oracle()`.
- Admin: `pause/unpause`, `set_fees`, `withdraw_fees`.
- Events: `DepositEvent`, `WithdrawEvent`, `OrderPlaced`, `TradeEvent`, `LiquidationEvent`, `FeeAccrued`, `FeesWithdrawn`.

### Off‑chain (Matcher API + UI)
Location: `offchain/matcher_api`
- Tech: Rust (axum, tokio, tower‑http), WebSocket for streams.
- REST endpoints:
  - `POST /deposit`, `POST /withdraw`
  - `POST /orders` (returns `{ id, tx? }`)
  - `POST /oracle`, `POST /fees`
  - `GET /status` (shows on‑chain feature & contract address)
- WS endpoint: `GET /ws`
- State (demo): in‑memory HashMaps for accounts/positions; VecDeque order queues.
- Optional on‑chain client (feature `onchain`): `abigen!` contract bindings (ethers‑rs) to call `ext_place_order`, `ext_match`, `ext_update_oracle`, `ext_deposit`.

Interplay:
- Off‑chain proposes the match and, when configured, submits the match to the contract.
- The contract is the source of truth when enabled; otherwise, the off‑chain state models exchange behavior for demo/testing.

---

## Security & Trust Model
- Off‑chain‑only mode: fully trusted operator, ephemeral state, no cryptographic guarantees.
- On‑chain‑enabled: custody and settlement are enforced by the contract; off‑chain becomes a relayer/matcher, reducing trust requirements.
- Missing (by design for demo): signed orders (EIP‑712), authentication, durable storage, slippage & risk controls.

---

## Run Modes
- Off‑chain demo: `cargo run -p matcher_api`; use REST + WS as per `postman.md`.
- On‑chain enabled: set `ARBITRUM_RPC`, `PRIVATE_KEY`, `CONTRACT_ADDRESS`, build with `--features onchain`, and the server will submit orders/matches to the contract.

---

## Data Structures (Key Types)
- `engine::Order { trader, side, price, qty, leverage, ts, expiry_ts, is_limit }`
- `engine::Position { trader, entry_price, qty, leverage, margin, opened_ts, expiry_ts }`
- `engine::Account { collateral, locked_margin }`
- `engine::OraclePrice { price, conf, ts }`

---

## Known Limitations (Demo)
- Naive orderbook (no price levels, no partial remainders beyond single pairing step).
- Single product, unit‑scaled prices, minimal risk controls.
- No persistence; process restart loses state.
- No authentication or signed orders.
- Oracle is a stub without aggregation/validation.

---

## Roadmap (Suggested Next Steps)
- Persistence (SQLite/Postgres) for accounts, orders, positions, and events.
- Signed orders and signature verification (EIP‑712) with wallet addresses as trader IDs.
- Robust orderbook data structure (price‑time priority, partial fills, cancels, AMM interactions if desired).
- Oracle aggregator with deviation/staleness thresholds and signed feeds.
- Batched matches and gas optimizations for on‑chain calls.
- Health monitors: metrics, alerts, and dashboards.

---

## Glossary
- Collateral: Funds posted by trader to back positions.
- Locked margin: Portion of collateral reserved to support open orders/positions.
- Equity: Collateral + PnL – Locked margin.
- Health bps: Equity/Locked margin in basis points (×10,000).
- Maker/Taker: Maker adds liquidity (older order); taker consumes liquidity (newer order).

---

For API details and ready‑to‑use requests, see `postman.md`. For build/run instructions and deployment notes, see `README.md` and `scripts/deploy_stylus.ps1`.
