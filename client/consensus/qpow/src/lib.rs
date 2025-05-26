mod chain_management;
mod miner;

pub use chain_management::ChainManagement;
pub use chain_management::HeaviestChain;
use codec::{Decode, Encode};
pub use miner::QPoWMiner;
use primitive_types::{H256, U512};
use sc_client_api::BlockBackend;
use sc_consensus_pow::{Error, PowAlgorithm};
use sp_api::ProvideRuntimeApi;
use sp_api::__private::BlockT;
use sp_consensus_pow::Seal as RawSeal;
use sp_consensus_qpow::QPoWApi;
use sp_runtime::generic::BlockId;
use std::marker::PhantomData;
use std::sync::Arc;

#[derive(Clone, Debug, Encode, Decode, PartialEq)]
pub struct QPoWSeal {
    pub nonce: [u8; 64],
}

pub struct QPowAlgorithm<B, C>
where
    B: BlockT<Hash = H256>,
    C: ProvideRuntimeApi<B>,
{
    pub client: Arc<C>,
    pub _phantom: PhantomData<B>,
}

impl<B, C> Clone for QPowAlgorithm<B, C>
where
    B: BlockT<Hash = H256>,
    C: ProvideRuntimeApi<B>,
{
    fn clone(&self) -> Self {
        Self {
            client: Arc::clone(&self.client),
            _phantom: PhantomData,
        }
    }
}

// Here we implement the general PowAlgorithm trait for our concrete Sha3Algorithm
impl<B, C> PowAlgorithm<B> for QPowAlgorithm<B, C>
where
    B: BlockT<Hash = H256>,
    C: ProvideRuntimeApi<B> + BlockBackend<B> + Send + Sync + 'static,
    C::Api: QPoWApi<B>,
{
    type Difficulty = U512;

    fn difficulty(&self, parent: B::Hash) -> Result<Self::Difficulty, Error<B>> {
        self.client
            .runtime_api()
            .get_difficulty(parent)
            .map(U512::from)
            .map_err(|_| Error::Runtime("Failed to fetch difficulty".into()))
    }

    fn verify(
        &self,
        parent: &BlockId<B>,
        pre_hash: &H256,
        _pre_digest: Option<&[u8]>,
        seal: &RawSeal,
        _difficulty: Self::Difficulty,
    ) -> Result<bool, Error<B>> {
        //Executed for mined and imported blocks

        /*

        For now, we will turn this off temporally.
        In this way, node can mine/import blocks without rewarding anyone.

        // Block miner should exist

        let mut extracted_author: Option<AccountId32> = None;
        if let Some(pre_digest_bytes) = pre_digest {
            if let Ok(account) = <AccountId32 as Decode>::decode(&mut &pre_digest_bytes[..]) {
                extracted_author = Some(account);
            }
        }

        let _author = match extracted_author {
            Some(acc) => acc,
            None => return Err(Error::Runtime("Failed to extract AccountId32 from pre_digest".into())),
        };

        */

        // Convert seal to nonce [u8; 64]
        let nonce: [u8; 64] = match seal.as_slice().try_into() {
            Ok(arr) => arr,
            Err(_) => panic!("Vec<u8> does not have exactly 64 elements"),
        };
        let parent_hash = match extract_block_hash(parent) {
            Ok(hash) => hash,
            Err(_) => return Ok(false),
        };

        let pre_hash = pre_hash.as_ref().try_into().unwrap_or([0u8; 32]);

        // Verify the nonce using QPoW
        if !self
            .client
            .runtime_api()
            .verify_for_import(parent_hash, pre_hash, nonce)
            .map_err(|e| Error::Runtime(format!("API error in verify_nonce: {:?}", e)))?
        {
            return Ok(false);
        }

        Ok(true)
    }
}

pub fn extract_block_hash<B: BlockT<Hash = H256>>(parent: &BlockId<B>) -> Result<H256, Error<B>> {
    match parent {
        BlockId::Hash(hash) => Ok(*hash),
        BlockId::Number(_) => Err(Error::Runtime(
            "Expected BlockId::Hash, but got BlockId::Number".into(),
        )),
    }
}
