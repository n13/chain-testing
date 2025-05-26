#![cfg_attr(not(feature = "std"), no_std)]
use codec::{Decode, Encode};
use scale_info::TypeInfo;
extern crate alloc;
use alloc::vec::Vec;
use primitive_types::U512;

/// Engine ID for QPoW consensus.
pub const QPOW_ENGINE_ID: [u8; 4] = *b"QPoW";

sp_api::decl_runtime_apis! {
    pub trait QPoWApi {
        /// Verify a nonce for a block being imported from the network
        fn verify_for_import(
            header: [u8; 32],
            nonce: [u8; 64],
        ) -> bool;

        /// Verify a nonce for a historical block that's already in the chain
        fn verify_historical_block(
            header: [u8; 32],
            nonce: [u8; 64],
            block_number: u32,
        ) -> bool;

        /// Submit a locally mined nonce
        fn submit_nonce(
            header: [u8; 32],
            nonce: [u8; 64],
        ) -> bool;

        /// calculate distance header with nonce to with nonce
        fn get_nonce_distance(
            header: [u8; 32],  // 256-bit header
            nonce: [u8; 64], // 512-bit nonce
        ) -> U512;

        /// Get the max possible reorg depth
        fn get_max_reorg_depth() -> u32;

        /// Get the max possible distance_threshold for work calculation
        fn get_max_distance() -> U512;

        /// Get the current difficulty (max_distance / distance_threshold)
        fn get_difficulty() -> U512;

        /// Get the current distance_threshold target for proof generation
        fn get_distance_threshold() -> U512;

        /// Get distance_threshold at block
        fn get_distance_threshold_at_block(block_number: u32) -> U512;

        /// Get total work
        fn get_total_work() -> U512;

        /// Get sum of block times in rolling history
        fn get_block_time_sum() -> u64;

        /// Get median block time for preconfigured list of elements
        fn get_median_block_time() -> u64;

        /// Get last block timestamp
        fn get_last_block_time() -> u64;

        // Get last block mining time
        fn get_last_block_duration() -> u64;

        /// Retrieve latest submitted proof
        fn get_latest_nonce() -> Option<[u8; 64]>;

        fn get_chain_height() -> u32;

        fn get_random_rsa(header: &[u8; 32]) -> (U512, U512);
        fn hash_to_group_bigint(h: &U512, m: &U512, n: &U512, solution: &U512) -> U512;
    }
}

#[derive(Debug, Encode, Decode, TypeInfo)]
pub enum Error {
    /// Invalid proof submitted
    InvalidProof,
    /// Arithmetic calculation error
    ArithmeticError,
    /// Other error occurred
    Other(Vec<u8>),
}
