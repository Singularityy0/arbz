# Postman Collection Guide for arbz Demo

This document lists all available HTTP endpoints and the WebSocket flow for the `matcher_api` service so you can easily recreate them in Postman. It includes example requests and typical responses. Adjust host/port if you changed defaults.

Base URL:
```
http://localhost:8787
```

## Environment Setup (Postman Environment Variables)
Create a Postman environment named `arbz-local` with variables:
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

## Suggested Postman Workflow
1. Create environment & variables.
2. Deposit for two traders.
3. Place a buy order for alice and a sell order for bob.
4. Open WS connection and wait for match event.
5. Update oracle to force liquidation scenario if desired (lower price).
6. Observe liquidation event.

## Error Handling Notes
- All endpoints currently return 200 with {"ok":false} on business failure (e.g., insufficient collateral). For production you’d want proper HTTP status codes.
- On-chain failures (when enabled) simply keep orders in the book; you may not see a tx hash. Re-try logic can be added later.

## Authentication / Security
None implemented in demo. A real system would require wallet signature per order and authenticated deposit tracking.

## Extending the Collection
You can export these requests as a Postman Collection JSON. Recommended folder grouping:
- Collateral: /deposit, /withdraw
- Trading: /orders, /ws
- Admin: /oracle, /fees, /status

## Troubleshooting
- No matches appearing: ensure at least one buy and one sell with overlapping price & positive qty.
- Liquidations not firing: set oracle price far from entry (e.g., drop from 100 to 50) after position opens.
- On-chain tx missing: verify env vars and that matcher started with `--features onchain`.

---
Generated automatically. Update as endpoints evolve.
