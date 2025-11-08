//! Chain interaction implementation (optional, behind `onchain` feature).
//! Provides minimal calls: place_order, match, update_oracle, deposit.

#[cfg(feature = "onchain")]
use ethers::{ prelude::*, types::{I256, U256, Address} };

#[cfg(feature = "onchain")]
abigen!(
    ZeroDayFutures,
    r#"[
        function ext_place_order(uint8 side, int256 price, int256 qty, uint32 leverage) external returns (uint64)
        function ext_match(uint64 buy_id, uint64 sell_id, int256 price) external
        function ext_update_oracle(uint64 product_id, int256 price) external
        function ext_deposit() external payable
    ]"#
);

#[derive(Clone)]
pub struct ChainClient {
    #[cfg(feature = "onchain")]
    active: bool,
    #[cfg(feature = "onchain")]
    contract: Option<ZeroDayFutures<SignerMiddleware<Provider<Http>, WalletSigner>>>,
    // store contract address string for logs
    pub contract_address: Option<String>,
}

#[cfg(feature = "onchain")]
type WalletSigner = LocalWallet;

impl ChainClient {
    pub fn new(contract_address: Option<String>) -> Self {
        #[cfg(feature = "onchain")]
        {
            let rpc = std::env::var("ARBITRUM_RPC").ok();
            let pk = std::env::var("PRIVATE_KEY").ok();
            let addr = contract_address.clone();
            let chain_id = std::env::var("CHAIN_ID").ok().and_then(|v| v.parse().ok()).unwrap_or(421614u64); // Arbitrum Sepolia default
            if let (Some(rpc), Some(pk), Some(ca)) = (rpc, pk, addr.clone(),) {
                if let Ok(provider) = Provider::<Http>::try_from(rpc) {
                    if let Ok(wallet) = pk.parse::<LocalWallet>() {
                        let signer = SignerMiddleware::new(provider, wallet.with_chain_id(chain_id));
                        if let Ok(address) = ca.parse::<Address>() {
                            let arc_signer = std::sync::Arc::new(signer);
                            let client = ZeroDayFutures::new(address, arc_signer); 
                            return Self { active: true, contract: Some(client), contract_address: Some(ca) };
                        }
                    }
                }
            }
            return Self { active: false, contract: None, contract_address };        
        }
        #[cfg(not(feature = "onchain"))]
        {
            Self { contract_address, }
        }
    }

    pub fn is_active(&self) -> bool {
        #[cfg(feature = "onchain")]
        { self.active && self.contract.is_some() }
        #[cfg(not(feature = "onchain"))]
        { false }
    }

    pub async fn place_order(&self, _side: u8, _price: i128, _qty: i128, _leverage: u32) -> anyhow::Result<Option<(u64, String)>> {
        #[cfg(feature = "onchain")]
        {
            if let Some(c) = &self.contract {
                // dry-run call() to get the order id (view) then send transaction
                let preview: u64 = c.ext_place_order(_side, I256::from(_price), I256::from(_qty), _leverage).call().await?;
                let call = c.ext_place_order(_side, I256::from(_price), I256::from(_qty), _leverage);
                let tx = call.send().await?;
                let txh = tx.tx_hash();
                return Ok(Some((preview, format!("0x{}", hex::encode(txh.as_bytes())))));
            }
        }
        Ok(None)
    }

    pub async fn match_orders(&self, _buy_id: u64, _sell_id: u64, _price: i128) -> anyhow::Result<Option<String>> {
        #[cfg(feature = "onchain")]
        {
            if let Some(c) = &self.contract {
                let call = c.ext_match(_buy_id, _sell_id, I256::from(_price));
                let tx = call.send().await?;
                let txh = tx.tx_hash();
                return Ok(Some(format!("0x{}", hex::encode(txh.as_bytes()))));
            }
        }
        Ok(None)
    }

    pub async fn update_oracle(&self, _product_id: u64, _price: i128) -> anyhow::Result<Option<String>> {
        #[cfg(feature = "onchain")]
        {
            if let Some(c) = &self.contract {
                let call = c.ext_update_oracle(_product_id, I256::from(_price));
                let tx = call.send().await?;
                let txh = tx.tx_hash();
                return Ok(Some(format!("0x{}", hex::encode(txh.as_bytes()))));
            }
        }
        Ok(None)
    }

    pub async fn deposit(&self, _amount_wei: u128) -> anyhow::Result<Option<String>> {
        #[cfg(feature = "onchain")]
        {
            if let Some(c) = &self.contract {
                let call = c.ext_deposit().value(U256::from(_amount_wei));
                let tx = call.send().await?;
                let txh = tx.tx_hash();
                return Ok(Some(format!("0x{}", hex::encode(txh.as_bytes()))));
            }
        }
        Ok(None)
    }
}
