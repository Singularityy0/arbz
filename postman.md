# Postman Collection Guide 

This document lists all available HTTP endpoints and the WebSocket flow for the `matcher_api` service so you can easily recreate them in Postman. It includes example requests and typical responses. Adjust host/port if you changed defaults.

Base URL:
```
http://localhost:8787
```

## Environment Setup
Create a Postman environment  with variables:
- `base_url` = `http://localhost:8787`
- `trader_alice` = `alice`
- `trader_bob` = `bob`
- `price` = `100`
- `qty` = `1`
- `leverage` = `1`

(Optional when on-chain integration is enabled)
- `contract_address` = `<0x...>`
- `onchain_active` = `true` or `false`

Use `{{base_url}}` in requests below.

## 1. Deposit
Add collateral for a trader.
- Method: POST
- URL: `{{base_url}}/deposit`
- Body (raw JSON):
```json
{
  "trader": "{{trader_alice}}",
  "amount": 1000
}
```
- Sample response:
```json
{"ok":true}
```

## 2. Withdraw
Withdraw collateral (succeeds only if free collateral >= amount).
- Method: POST
- URL: `{{base_url}}/withdraw`
- Body:
```json
{
  "trader": "{{trader_alice}}",
  "amount": 200
}
```
- Response:
```json
{"ok":true}
```
(or `{"ok":false}` if insufficient free collateral)

## 3. Place Plain Order
Place a buy or sell order.
- Method: POST
- URL: `{{base_url}}/orders`
- Body:
```json
{
  "trader": "{{trader_alice}}",
  "side": "buy",
  "price": {{price}},
  "qty": {{qty}},
  "leverage": {{leverage}},
  "ttl_secs": 3600,
  "is_limit": true
}
```
- Response off-chain only:
```json
{"id":1,"tx":null}
```
- Response with on-chain active (example):
```json
{"id":42,"tx":"0xabc123..."}
```
`id` is the order id (on-chain if active). `tx` present only when on-chain placement succeeded.

## 4. Place Signed Order (EIP-712)
Requires server started with `--features signing` and using the signer CLI to produce a JSON payload.

- Method: POST
- URL: `{{base_url}}/orders/signed`
- Body (example):
```json
{
  "order": {
    "trader": "0x8ba1f109551bD432803012645Ac136ddd64DBA72",
    "side": "buy",
    "price": 101,
    "qty": 500,
    "leverage": 10,
    "ttl_secs": 600,
    "is_limit": true,
    "nonce": 0
  },
  "signature": "0x...65bytes..."
}
```
- Success Response mirrors plain order: `{ "id": <order_id>, "tx": null }`
- Error responses:
  - Bad nonce: `{ "error": "bad nonce", "expected": <n> }`
  - Signature mismatch: HTTP 401 `{ "error": "signature mismatch" }`
  - Encoding failures: `{ "error": "encode failed" }`

## 5. Update Oracle Price
Set the mark price used for PnL & liquidation (demo single product).
- Method: POST
- URL: `{{base_url}}/oracle`
- Body:
```json
{
  "price": 101
}
```
- Response:
```json
{"ok":true}
```
(On-chain active: triggers a transaction; no tx hash returned directly, WS may show later.)

## 6. Update Fee Configuration
Set maker/taker basis points.
- Method: POST
- URL: `{{base_url}}/fees`
- Body:
```json
{
  "maker_bps": 2,
  "taker_bps": 5
}
```
- Response:
```json
{"ok":true}
```

## 7. Status
Check if on-chain feature is compiled and active.
- Method: GET
- URL: `{{base_url}}/status`
- Response off-chain only:
```json
{"onchain_feature":false}
```
- Response on-chain active:
```json
{"onchain_feature":true,"active":true,"contract_address":"0x..."}
```

## 8. Get State Snapshot
Aggregated risk + mark price.
- Method: GET
- URL: `{{base_url}}/state`
- Sample response:
```json
{
  "mark": 102,
  "traders": [
    {
      "trader": "alice",
      "collateral": 99975,
      "locked_margin": 5050,
      "qty": 500,
      "entry_price": 100,
      "pnl": 1000,
      "health_bps": 190000,
      "nonce": 1
    }
  ]
}
```

Field meanings: see `final.md` (PnL, health, nonce).

## 9. WebSocket Match / Oracle / Liquidation Stream
Stream match and liquidation events.
- URL: `ws://localhost:8787/ws`
- In Postman: New tab -> WebSocket -> enter URL -> Connect.
- Example match event (off-chain only):
```json
{
  "event": "match",
  "price": 100,
  "qty": 1,
  "buy_trader": "alice",
  "sell_trader": "bob",
  "maker_fee": 20,
  "taker_fee": 50,
  "buy_id": 1,
  "sell_id": 2
}
```
- Example match event (on-chain):
```json
{
  "event": "match",
  "price": 100,
  "qty": 1,
  "buy_trader": "alice",
  "sell_trader": "bob",
  "maker_fee": 20,
  "taker_fee": 50,
  "buy_id": 41,
  "sell_id": 42,
  "tx": "0xabc123..."
}
```
- Liquidation event sample:
```json
{
  "event": "liquidation",
  "trader": "alice",
  "mark": 85
}
```

