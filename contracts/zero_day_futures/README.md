# Zero Day Futures Stylus Contract

This crate contains the Stylus (Rust/WASM) smart contract for the demo.

## Quickstart (per Stylus quickstart)

Prereqs:
- Rust nightly + wasm32-unknown-unknown target
- Stylus CLI: `cargo install --locked stylus`
- RPC + deployer key: set `ARBITRUM_RPC` and `PRIVATE_KEY` env vars

Build (WASM):
```powershell
rustup target add wasm32-unknown-unknown
cargo stylus build -p zero_day_futures --release
```

Deploy (PowerShell):
```powershell
# set your RPC and key
$env:ARBITRUM_RPC = "https://sepolia-rollup.arbitrum.io/rpc"
$env:PRIVATE_KEY = "0x..."

./scripts/deploy_stylus.ps1 -ContractCrate contracts/zero_day_futures
```

Post-deploy, export the address to use with the matcher API:
```powershell
$env:CONTRACT_ADDRESS = "0x..."
```

Public externs (selected):
- `ext_init()`
- `ext_deposit()` (payable)
- `ext_withdraw(amount)`
- `ext_place_order(side, price, qty, leverage)`
- `ext_match(buy_id, sell_id, price)`
- `ext_update_oracle(product_id, price)`
- `ext_liquidate(trader, mark_price)` / `ext_batch_liquidate(traders, mark_price)`
- `ext_set_fees(maker_bps, taker_bps)` / `ext_withdraw_fees(to, amount)`
