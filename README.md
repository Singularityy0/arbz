<div align="center">

# Arbz / SinguX – Zero‑Day Futures Exchange (MVP)

Rust + Arbitrum Stylus prototype for intraday ("zero‑day") futures on the synthetic asset $singu.

"Low-latency off‑chain matching today, verifiable on‑chain settlement tomorrow."

</div>

## 1. Overview
This MVP implements a hybrid futures exchange:
- Off‑chain matcher & risk engine (`matcher_api`, Axum + Tokio) with REST & WebSocket.
- Optional Stylus Rust contract scaffold for eventual on‑chain margin & settlement.
- EIP‑712 signed order flow (feature `signing`) with per‑trader nonce tracking.
- Live HTML frontend (static) showing oracle price, last match, liquidations, and positions (PnL + health).

Full deep dive: see `final.md`.

## 2. Features
- Collateral management: deposit / withdraw (off‑chain now; on‑chain planned).
- Limit orders with leverage & TTL (expiry semantics groundwork).
- Deterministic midpoint matching between best bid & ask.
- Real‑time PnL & health_bps calculation; liquidation threshold at 50%.
- Maker / taker fees (basis points) configurable via `/fees`.
- EIP‑712 signed orders: server verifies signature + nonce (replay protection).
- Simulated oracle price random walk (bounded) for continuous PnL movement.
- Tolerant mutex recovery to avoid cascade panics during development.

## 3. Architecture Snapshot
```
Browser UI <-> Axum REST (/orders, /deposit, /state, ...) + WS (/ws events)
				 |          
				 | (optional onchain feature flag)
			 Stylus Contract (future: collateral, settlement, oracle)
```
Core Crates:
- `engine`: Order/Position/Account types; risk helpers.
- `offchain/matcher_api`: server + signer CLI.
- `contracts/zero_day_futures`: Stylus contract scaffold.

## 4. Repo Layout
| Path | Purpose |
|------|---------|
| `contracts/zero_day_futures` | Stylus Rust contract skeleton |
| `engine` | Shared types & risk logic |
| `offchain/matcher_api/src/main.rs` | Axum server & matching loop |
| `offchain/matcher_api/src/bin/sign_order.rs` | Signed order CLI utility |
| `offchain/matcher_api/static/index.html` | Minimal live trading UI |
| `final.md` | Comprehensive documentation |

## 5. Quickstart (Windows PowerShell)
```powershell
# Build with signing feature (signed orders enabled)
cargo build -p matcher_api --features signing

# Run matcher API
cargo run -p matcher_api --features signing

# Deposit collateral
Invoke-RestMethod -Uri http://localhost:8787/deposit -Method POST -Body '{"trader":"alice","amount":100000}' -ContentType 'application/json'

# Place plain order
Invoke-RestMethod -Uri http://localhost:8787/orders -Method POST -Body '{"trader":"alice","side":"buy","price":101,"qty":500,"leverage":10,"ttl_secs":600,"is_limit":true}' -ContentType 'application/json'

# Fetch state snapshot
Invoke-RestMethod -Uri http://localhost:8787/state -Method GET
```
Open browser: http://localhost:8787/

## 6. Endpoints Summary
| Method | Path | Description |
|--------|------|-------------|
| POST | `/deposit` | Add collateral for trader |
| POST | `/withdraw` | Withdraw free collateral (collateral - locked_margin) |
| POST | `/orders` | Place plain order |
| POST | `/orders/signed` | Place EIP‑712 signed order (feature `signing`) |
| POST | `/oracle` | Set mark price (demo) |
| POST | `/fees` | Configure maker/taker bps |
| GET | `/state` | Current mark + trader views (collateral, locked, qty, entry, pnl, health, nonce) |
| GET | `/status` | On-chain feature status (compiled/active, contract address) |
| WS | `/ws` | Stream oracle ticks, match and liquidation events |

## 7. Signed Order Flow (CLI)
```powershell
# Generate signed order JSON (prints {"order":...,"signature":...})
cargo run -p matcher_api --features signing --bin sign_order -- --privkey <hex_privkey> --side buy --price 101 --qty 500 --leverage 10 --ttl_secs 600 --is_limit true

# Submit signed order
Invoke-RestMethod -Uri http://localhost:8787/orders/signed -Method POST -Body <PASTE_JSON> -ContentType 'application/json'
```
Domain fields inside server: name=ArbzZeroDay, version=1, chainId=421614, verifyingContract=0x000...0 (update when real contract deployed).

## 8. Risk Engine Snapshot
Key metrics per trader (see `final.md` §14):
- Locked Margin = |price|×|qty| ÷ leverage
- PnL = (mark − entry_price) × qty
- Equity = collateral + PnL − locked_margin
- Health (bps) = 10_000 × Equity ÷ locked_margin (None if locked_margin==0)
Liquidation triggered when health_bps < 5,000 → position closed, margin released.

## 9. Matching Algorithm (MVP)
- Midpoint between best buy & sell front orders.
- Qty = min(buy.qty, sell.qty).
- Fees charged immediately; weighted average entry price update for partial position changes.
Upcoming: release order margin after fill, partial fills, multi-asset.

## 10. Oracle (Demo)
Bounded random walk 50–150 updated ~1.5s; triggers WS oracle events and drives PnL/health recalculation.
Planned: replace with decentralized feed and on-chain anchoring.

## 11. Limitations / TODO
- Matching loop runs only while at least one WS client connected.
- Order margin not released post-fill yet.
- No persistence (all in-memory).
- Synthetic oracle; no external data integrity.
- Single asset ($singu). Multi-product planned.

## 12. Progressive On-Chain Path
1. On-chain collateral & nonce views.
2. Batch settlement of signed matches.
3. Decentralized oracle integration.
4. Event log anchoring / replay verification.
5. Multi-relayer matching & dispute mechanism.

## 13. Development Notes
Tolerant mutex locking prevents PoisonError crashes after earlier panics. JSON outputs clamp large i128 values to i64.
Feature flags:
- `signing` enables signed orders & signer CLI.
- `onchain` (future) toggles chain client calls.

## 14. References
- Deep dive: `final.md`
- API examples: `postman.md`
- Arbitrum Stylus Docs: https://docs.arbitrum.io/stylus

## 15. License / Usage
Prototype for educational & exploratory purposes; not production secure. Review and enhance before mainnet deployment.


