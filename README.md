# Zero Day Futures Demo (Arbitrum Stylus + Rust)

This workspace contains a minimal, runnable demo that covers core flows:

- Collateral vault: deposit/withdraw (Stylus contract)
- Order placement with 24h default expiry
- Off-chain matcher with REST+WebSocket that pairs orders and emits a match event
- Basic risk/PNL engine in pure Rust (unit tested)
- Settlement and liquidation very simplified for demo

## Layout

- `contracts/zero_day_futures`: Stylus Rust smart contract (WASM target required)
- `engine`: pure-Rust library for risk calculation and types
- `offchain/matcher_api`: axum-based mock matcher server

## Build (Rust parts)

- Build engine + matcher:

```powershell
cargo build -p engine -p matcher_api
```

- Run matcher API:

```powershell
cargo run -p matcher_api
```

- Place an order (REST):

```powershell
$body = @{ trader = "0xabc"; side = "buy"; price=100000000; qty=1000; leverage=10; ttl_secs=86400; is_limit=$true } | ConvertTo-Json
Invoke-RestMethod -Method Post -Uri http://localhost:8787/orders -ContentType 'application/json' -Body $body
```

- Open a websocket viewer to see matches at ws://localhost:8787/ws

## Stylus Contract notes

The contract is a simplified demo and includes:

- Deposit / Withdraw (Collateral Vault)
- Order placement & matching (simplistic netting)
- Margin health (basis points) with liquidation via `ext_liquidate`
- Oracle price update stub (`ext_update_oracle`) restricted to owner
- Pausable & owner access control

Still missing before production:

- Multi-oracle aggregation (Chainlink/Pyth/Uniswap TWAP)
- Event-rich settlement flows & fee accounting
- Reentrancy guards (Stylus patterns) & comprehensive testing
- Multi-product support & portfolio margin
- Robust math (fixed-point) and overflow checks

See `task.md` for complete requirements. To build/deploy with Stylus, consult:

- https://docs.arbitrum.io/stylus/reference/overview
- https://docs.arbitrum.io/stylus/reference/project-structure
- https://docs.arbitrum.io/stylus-by-example/basic_examples/hello_world

You will need Rust nightly + wasm32 target and the Stylus toolchain.

### Example (pseudo) contract interaction

```text
ext_init()                                # initialize with sender as owner
ext_deposit() value=1000                  # deposit collateral
ext_place_order(side=0, price=100, qty=1000, leverage=10)  # buy order
ext_place_order(side=1, price=101, qty=1000, leverage=10)  # sell order
ext_match(buy_id, sell_id, price=100)     # execute trade
ext_update_oracle(product_id=1, price=99) # update mark price
ext_liquidate(trader, mark_price=80)      # force liquidation if health below threshold
```

### Liquidation threshold

Stored in `liquidation_threshold_bps` (default 5000 = 50%). Health below this triggers `LiquidationEvent`.

## Stylus deploy quickstart

See `contracts/zero_day_futures/README.md` and `scripts/deploy_stylus.ps1`.

Basic flow:
1. Install toolchain: `cargo install --locked stylus`; `rustup target add wasm32-unknown-unknown`.
2. Build: `cargo stylus build -p zero_day_futures --release`.
3. Deploy: set `ARBITRUM_RPC` and `PRIVATE_KEY`, then run the script.
4. Set `CONTRACT_ADDRESS` env var for the matcher once we enable on-chain calls.

## Matcher API & Frontend

Run matcher API (serves static UI at http://localhost:8787/):

```powershell
cargo run -p matcher_api
```

Key endpoints:

- POST /orders { trader, side, price, qty, leverage, ttl_secs, is_limit }
- POST /deposit { trader, amount }
- POST /withdraw { trader, amount }
- POST /oracle { price }  (updates demo mark)
- POST /fees { maker_bps, taker_bps }
- WS /ws  (stream match + liquidation events)

Open http://localhost:8787/ in a browser to place orders and view events.

