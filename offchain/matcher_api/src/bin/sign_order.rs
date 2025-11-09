use clap::Parser;
#[cfg(feature = "signing")]
use ethers::core::types::Address;
#[cfg(feature = "signing")]
use ethers::types::transaction::eip712::{TypedData, Eip712};
use anyhow::{Result, anyhow};
use serde::{Serialize, Deserialize};

#[derive(Parser, Debug)]
#[command(name="sign-order", about="Generate EIP-712 signed order JSON for matcher_api /orders/signed endpoint")]
struct Args {
    
    #[arg(long)]
    privkey: String,
    #[arg(long)]
    side: String,
    #[arg(long)]
    price: i128,
    #[arg(long)]
    qty: i128,
    #[arg(long, default_value_t = 10)]
    leverage: u32,
    #[arg(long, default_value_t = 86400)]
    ttl_secs: u64,
    #[arg(long, default_value_t = true)]
    is_limit: bool,
    #[arg(long, default_value = "http://127.0.0.1:8787")]
    api: String,

    #[arg(long)]
    nonce: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(not(feature = "signing"), allow(dead_code))]
struct SignedOrderData {
    trader: Address,
    side: String,
    price: i128,
    qty: i128,
    leverage: u32,
    ttl_secs: u64,
    is_limit: bool,
    nonce: u64,
}

#[derive(Serialize)]
struct OutputPayload { order: SignedOrderData, signature: String }

#[cfg(feature = "signing")]
#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    if args.side.to_lowercase() != "buy" && args.side.to_lowercase() != "sell" {
        return Err(anyhow!("side must be buy or sell"));
    }
    let pk_bytes = hex::decode(args.privkey.trim_start_matches("0x"))?;
    if pk_bytes.len() != 32 { return Err(anyhow!("private key must be 32 bytes")); }
    use ethers::signers::{LocalWallet, Signer};
    let wallet = LocalWallet::from_bytes(&pk_bytes)?;
    let trader_addr = wallet.address();

    let nonce = match args.nonce {
        Some(n) => n,
        None => fetch_nonce(&args.api, trader_addr).await.unwrap_or(0)
    };

    let data = SignedOrderData {
        trader: trader_addr,
        side: args.side.to_lowercase(),
        price: args.price,
        qty: args.qty,
        leverage: args.leverage,
        ttl_secs: args.ttl_secs,
        is_limit: args.is_limit,
        nonce,
    };
    //  EIP-712 digest
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
            "trader": format!("{:?}", data.trader),
            "side": data.side,
            "price": data.price,
            "qty": data.qty,
            "leverage": data.leverage,
            "ttl_secs": data.ttl_secs,
            "is_limit": data.is_limit,
            "nonce": data.nonce
        }
    });
    let typed: TypedData = serde_json::from_value(td_json)?;
    let digest = typed.encode_eip712()?; // [u8;32]
    let signature = wallet.sign_hash(ethers::core::types::H256::from(digest))?;
    let signature_hex = format!("0x{}", hex::encode(signature.to_vec()));

    let out = OutputPayload { order: data, signature: signature_hex };
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

#[cfg(feature = "signing")]
async fn fetch_nonce(api: &str, trader: Address) -> Result<u64> {
    let url = format!("{}/state", api.trim_end_matches('/'));
    let resp: serde_json::Value = reqwest::get(url).await?.json().await?;
    if let Some(arr) = resp.get("traders").and_then(|v| v.as_array()) {
        for t in arr {
            if t.get("trader").and_then(|x| x.as_str()) == Some(&format!("{:?}", trader)) {
                return Ok(t.get("nonce").and_then(|n| n.as_u64()).unwrap_or(0));
            }
        }
    }
    Ok(0)
}

#[cfg(not(feature = "signing"))]
fn main() {
    eprintln!("sign_order requires building with --features signing");
}
