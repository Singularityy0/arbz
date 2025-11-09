# Singularity ($singu)

## Overview
Singularity is a minimal, end-to-end prototype of intraday futures exchange. 
This is a minimal prototype combining an off-chain Rust matching engine and an Arbitrum Stylus smart contract skeleton for a single synthetic futures instrument ($singu).



## 2. Why Arbitrum (and Stylus)
Arbitrum offers low-cost, high-throughput execution suitable for frequent trading and oracle updates. Stylus extends Arbitrum by allowing WASM smart contracts coded in Rust (or cpp) rather than only Solidity.I believe it leverages Reusing Rust types and logic across on-chain and off-chain components.
and efficient WASM execution, potentially lower gas for complex logic.

For a zero-day futures product, latency and cost constraints mean perpetual on-chain matching is often impractical early. Stylus enables selective migration: margin settlement or finalization of matches can move on-chain while keeping higher-frequency experimentation off-chain.
- Frontend (`offchain/matcher_api/static/index.html`): Displays live oracle price, last match, last liquidation, positions table with PnL & health, and account deposit/withdraw controls.
- Signer Utility (`offchain/matcher_api/src/bin/sign_order.rs`): CLI for creating EIP-712 signed orders with nonce.
1. Trader deposits collateral (REST) → updates account state.
2. Trader creates (plain or signed) order → margin locked (notional/leverage) → order enqueued in local book.
5. `/state` polled for aggregated risk view (mark price + trader snapshots).


## 4. Why an Off-Chain Demo First
after developing a very early working off chain demo , we tried writing it on chain by mapping the following 
HashMap state → Contract storage mappings.
Matching loop → Batch settlement function.
Oracle jitter → Trusted feed / aggregator contract.
Nonce HashMap → On-chain per-address nonce mapping.
Signature recovery off-chain → On-chain verification on each consumed order.
Liquidation in WebSocket loop → Explicit liquidate function callable by anyone.
Fee deduction logic → Fee computation + transfer/booking in contract.
Health calculation function → Pure/view function in contract.
But I had trouble deploying the contract , just using 
## 5. Planned On-Chain Evolution
Phased migration roadmap:
2. Margin & Collateral Escrow: Move deposit/withdraw and locked margin enforcement on-chain; off-chain engine only proposes matches referencing on-chain balances.
3. Order Authentication: Signed orders posted on-chain (or sequenced in a rollup inbox) enabling trust-minimized off-chain match proofs.
4. Oracle Integration: Replace synthetic random walk with decentralized price feed (Chainlink or custom aggregator) anchored on-chain.
5. Fraud Proofs / Challenge Window: Introduce a mechanism to dispute incorrect off-chain matches before settlement finalization.
## 6. Trading & Finance Variables 
- Price: Current oracle mark (`mark`); drives unrealized PnL.
- Notional: `abs(price) * abs(qty)` — gross exposure.
- Leverage: Intent parameter; margin locked = notional / leverage (simplified; leverage=0 means fully collateralized).
- Collateral: Liquid funds minus fees plus realized PnL; adjusted directly by fees and liquidation settlement.
- Locked Margin: Amount reserved to support open orders/positions; released on liquidation or (future) order completion logic.
- PnL: `(mark - entry_price) * qty` (qty sign encodes direction; short gets negative qty so formula naturally flips).
- Equity (internal): `collateral + PnL - locked_margin` (simplified view of usable funds after obligations).
- Fees: Percentage (basis points) of notional; maker vs taker applied symmetrically in this midpoint model for illustration.
- Nonce: Sequential number per trader to prevent replay of signed orders.
Current State: Centralized off-chain engine with client-verifiable signatures (EIP-712) for authenticity; oracle simulated(custom) data ephemeral.

- Multi-Relayer: Allow multiple independent matchers to submit candidate batches; consensus (first valid or majority) chosen on-chain.
- Oracle Decentralization: Replace local jitter with a multi-source median aggregator publishing signed price updates consumed by both off-chain and on-chain components.
- Release locked margin after order fully fills; differentiate order vs position margin.
- Background matcher independent of an active WebSocket client.
- Full on-chain margin & trade settlement using Stylus contract logic.
- Dispute mechanism for off-chain batches (fraud proofs, validity proofs).

## 7. How to run!

Prerequisites: Rust toolchain (stable), optionally Arbitrum Stylus environment for contract experiments.
```cargo build -p matcher_api --features signing```
```
cargo run -p matcher_api --features signing --bin matcher_api
```

In another terminal, place a plain order:
```powershell
Invoke-RestMethod -Uri http://localhost:8787/deposit -Method POST -Body '{"trader":"alice","amount":100000}' -ContentType 'application/json'
Invoke-RestMethod -Uri http://localhost:8787/orders -Method POST -Body '{"trader":"alice","side":"buy","price":101,"qty":500,"leverage":10,"ttl_secs":600,"is_limit":true}' -ContentType 'application/json'
```

Web UI: Navigate to `http://localhost:8787/` for live metrics.

Signed order flow (example):
```powershell
cargo run -p matcher_api --features signing --bin sign_order -- --privkey <hex_privkey> --side buy --price 101 --qty 500 --leverage 10 --ttl_secs 600 --is_limit true
# Output JSON: {"order":{...},"signature":"0x..."}
Invoke-RestMethod -Uri http://localhost:8787/orders/signed -Method POST -Body '<JSON FROM ABOVE>' -ContentType 'application/json'
```

Fetch state:
```powershell
Invoke-RestMethod -Uri http://localhost:8787/state -Method GET
```

## 10. Algorithms & Design Rationale
Matching Algorithm: Simple midpoint of best bid and best ask; both orders fully consume min qty; chosen for clarity and deterministic fills rather than price-time priority complexity.

Order Book Representation: Two `VecDeque`s for buys/sells; minimal operations (front peek & pop) suit prototype and make matching loop O(1) per iteration.

Oracle Jitter: Bounded random walk (clamped between 50–150) with periodic direction flips; avoids external dependencies while providing dynamic PnL changes for demo.

Fee Calculation: Maker/taker basis points on notional; symmetrical deduction from counterparties for educational transparency. Real systems might credit maker rebates rather than charge.

PnL & Health Computation: Direct arithmetic on signed qty; health expressed in basis points to normalize risk across leverage settings and allow threshold-based liquidation.

Liquidation Logic: Trigger when `health_bps < 5000` (50%). Performs settlement by adding PnL to collateral, releasing locked margin, and closing position. Simplified no partial liquidation or grace periods.

Signature Verification: EIP-712 domain separation with `TypedData::encode_eip712()`; uses ethers-rs `Signature::recover` for public key recovery. Nonce ensures forward-only sequence and mitigates replay.




## 11. Limitations 
- Matching only advances when at least one WebSocket client is connected (current implementation); planned decoupling will run matching separately.
- Locked margin release is simplified; real implementations differentiate between order margin vs position maintenance margin.
- Oracle is synthetic; price integrity not guaranteed until decentralized feed integrated.
- No persistence,state lost on restart; event buffer/history planned.

## 12. Margin, PnL, Health (Risk Engine) | Example

Core formulas (current MVP):
- Notional = |price| × |qty|
- Locked Margin = Notional ÷ leverage (if leverage = 0 treat as fully collateralized and lock full notional)
- PnL = (mark − entry_price) × qty (qty sign encodes direction; negative qty means short so formula auto-adjusts)
- Equity = collateral + PnL − locked_margin
- Health (bps) = if locked_margin == 0 → None; else 10_000 × Equity ÷ locked_margin
- Liquidation trigger: health_bps < 5000 (50%)
- Fees: maker_fee = notional × maker_bps / 10000; taker_fee = notional × taker_bps / 10000

Scenario:
1. Alice deposits 100,000 collateral. Bob deposits 100,000.
2. Alice submits BUY limit (price=101, qty=500, leverage=10). Bob submits SELL limit (price=99, qty=500, leverage=10).
3. Margin lock per side:
	- Notional = 101 × 500 = 50,500
	- Locked Margin = 50,500 ÷ 10 = 5,050
4. Matching (midpoint): price = (101 + 99)/2 = 100; qty = 500.
5. Fees (maker_bps=2, taker_bps=5 default): Notional at match = 100 × 500 = 50,000
	- maker_fee = 50,000 × 2 / 10,000 = 10
	- taker_fee = 50,000 × 5 / 10,000 = 25
6. Collateral after fees:
	- If Alice took (taker): 100,000 − 25 = 99,975
	- Bob (maker): 100,000 − 10 = 99,990
7. Positions:
	- Alice qty = +500 @ entry_price 100
	- Bob qty = −500 @ entry_price 100
8. If mark moves to 103:
	- Alice PnL = (103 − 100) × 500 = +1,500 → Equity = 99,975 + 1,500 − 5,050 = 96,425 → Health ≈ 10,000 × 96,425 / 5,050 ≈ 191,100 bps
	- Bob PnL = (103 − 100) × (−500) = −1,500 → Equity = 99,990 − 1,500 − 5,050 = 93,440 → Health ≈ 10,000 × 93,440 / 5,050 ≈ 185,100 bps
9. If mark declines enough that Equity < 0.5 × locked_margin (health_bps < 5,000), liquidation is triggered: position closed, PnL realized, locked margin released.

Notes:
- Health None when locked_margin == 0 avoids misleading large ratios.
- MVP keeps order margin locked after fill; planned fix: release order margin and only keep maintenance margin tied to open position.

## 13. Order Lifecycle (Current Off-Chain vs Planned On-Chain)

Steps today:
1. Deposit: /deposit increments collateral (on-chain: contract function deposit()).
2. Nonce Fetch (for signing): /state returns per-trader nonce (on-chain: view getNonce(address)).
3. Sign (optional): CLI builds EIP-712 typed order (domain must match chainId & contract address in on-chain version).
4. Place Order: /orders or /orders/signed; locks margin = notional ÷ leverage.
5. Queue: Order stored in VecDeque (buys/sells) off-chain.
6. Match Loop: WebSocket handler scans top of book; chooses midpoint price; computes fees; updates positions & collateral.(FIFO)
7. Risk Evaluation: Each iteration + oracle tick recalculates PnL & health; if health_bps < threshold → liquidation.
8. Withdraw: /withdraw checks available collateral (collateral − locked_margin) and reduces it.

On-chain transformation targets:
- Batch Settlement: Instead of implicit midpoint, off-chain matcher forms a batch of matched signed orders and calls settleMatches(matches[]).
- Signature & Nonce Verification: Contract validates EIP-712 signatures and nonce monotonicity per trader.
- Margin Accounting: Contract calculates and stores locked vs maintenance margin; releases excess after fill.
- Oracle Source: Trusted feed updates price; view functions expose mark for UI; events log changes.
- Liquidation: Anyone can invoke liquidate(trader) if health below threshold.

Condensed Example Recap:
Alice buy 101×500 @10× vs Bob sell 99×500 @10× → midpoint fill 100×500; fees 25 (taker) & 10 (maker); margins locked 5,050 each; health reacts to mark. Migration replaces midpoint logic with explicit price from matched orders and verifiable signatures on-chain.

