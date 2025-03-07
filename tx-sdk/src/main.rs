use tx_sdk::TxSdk;
use sp_core::H256;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize the SDK with your node's RPC URL
    let sdk = TxSdk::new("http://localhost:9944");

    // Example unsigned extrinsic (replace with your actual extrinsic bytes)
    let unsigned_extrinsic = vec![0x41, 0x02, 0x00]; // Dummy data for testing

    // Send the transaction
    match sdk.send_tx(unsigned_extrinsic).await {
        Ok(hash) => println!("Transaction submitted successfully with hash: {:?}", hash),
        Err(e) => eprintln!("Failed to submit transaction: {:?}", e),
    }

    Ok(())
}