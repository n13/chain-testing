use jsonrpsee::core::client::{ClientT, Error as JsonRpcError};
use jsonrpsee::http_client::{HttpClient, HttpClientBuilder};
use jsonrpsee::rpc_params;
use sp_core::H256;

pub struct TxSdk {
    rpc: HttpClient,
}

impl TxSdk {
    /// Creates a new TxSdk instance with the given RPC URL.
    pub fn new(url: &str) -> Self {
        let rpc = HttpClientBuilder::default()
            .build(url)
            .expect("Valid RPC URL required");
        Self { rpc }
    }

    /// Sends an unsigned extrinsic to the Substrate node via RPC.
    pub async fn send_tx(&self, unsigned_extrinsic: Vec<u8>) -> Result<H256, JsonRpcError> {
        let encoded = hex::encode(unsigned_extrinsic);
        self.rpc
            .request("author_submitExtrinsic", rpc_params![encoded])
            .await
    }
}