use crate::{ResonanceSignatureScheme, ResonanceSignatureWithPublic, ResonanceSigner};

use super::types::{ResonancePair, ResonancePublic};
use sp_core::{
    crypto::{DeriveError, DeriveJunction, SecretStringError},
    ByteArray, Pair,
};
use sp_runtime::{
    traits::{IdentifyAccount, Verify},
    AccountId32,
};
use sp_std::vec::Vec;

pub fn crystal_alice() -> ResonancePair {
    let seed = [0u8; 32];
    ResonancePair::from_seed_slice(&seed).expect("Always succeeds")
}
pub fn dilithium_bob() -> ResonancePair {
    let seed = [1u8; 32];
    ResonancePair::from_seed_slice(&seed).expect("Always succeeds")
}
pub fn crystal_charlie() -> ResonancePair {
    let seed = [2u8; 32];
    ResonancePair::from_seed_slice(&seed).expect("Always succeeds")
}

impl IdentifyAccount for ResonancePair {
    type AccountId = AccountId32;
    fn into_account(self) -> AccountId32 {
        self.public().into_account()
    }
}

impl Pair for ResonancePair {
    type Public = ResonancePublic;
    type Seed = Vec<u8>;
    type Signature = ResonanceSignatureWithPublic;

    fn derive<Iter: Iterator<Item = DeriveJunction>>(
        &self,
        _path_iter: Iter,
        _seed: Option<<ResonancePair as Pair>::Seed>,
    ) -> Result<(Self, Option<<ResonancePair as Pair>::Seed>), DeriveError> {
        // Collect the path_iter into a Vec to avoid consuming it prematurely in checks
        unimplemented!("derive not implemented");
    }

    fn from_seed_slice(seed: &[u8]) -> Result<Self, SecretStringError> {
        ResonancePair::from_seed(seed).map_err(|_| SecretStringError::InvalidSeed)
    }

    #[cfg(any(feature = "default", feature = "full_crypto"))]
    fn sign(&self, message: &[u8]) -> ResonanceSignatureWithPublic {
        // Create keypair struct

        use crate::types::ResonanceSignature;
        let keypair =
            hdwallet::create_keypair(&self.public, &self.secret).expect("Failed to create keypair");

        // Sign the message
        let signature = keypair
            .sign(message, None, false)
            .expect("Signing should succeed");

        let signature =
            ResonanceSignature::try_from(signature.as_ref()).expect("Wrap doesn't fail");

        ResonanceSignatureWithPublic::new(signature, self.public())
    }

    fn verify<M: AsRef<[u8]>>(
        sig: &ResonanceSignatureWithPublic,
        message: M,
        pubkey: &ResonancePublic,
    ) -> bool {
        let sig_scheme = ResonanceSignatureScheme::Resonance(sig.clone());
        let signer = ResonanceSigner::Resonance(pubkey.clone());
        sig_scheme.verify(message.as_ref(), &signer.into_account())
    }

    fn public(&self) -> Self::Public {
        ResonancePublic::from_slice(&self.public).expect("Failed to create ResonancePublic")
    }

    fn to_raw_vec(&self) -> Vec<u8> {
        // this is modeled after sr25519 which returns the private key for this method
        self.secret.to_vec()
    }

    #[cfg(feature = "std")]
    fn from_string(s: &str, password_override: Option<&str>) -> Result<Self, SecretStringError> {
        Self::from_string_with_seed(s, password_override).map(|x| x.0)
    }
}

#[cfg(test)]
mod tests {
    use sp_std::vec;

    use super::*;

    fn setup() {
        // Initialize the logger once per test run
        // Using try_init to avoid panics if called multiple times
        let _ = env_logger::try_init();
    }

    #[test]
    fn test_sign_and_verify() {
        setup();

        let seed = vec![0u8; 32];

        let pair = ResonancePair::from_seed_slice(&seed).expect("Failed to create pair");
        let message: Vec<u8> = b"Hello, world!".to_vec();

        log::info!("Signing message: {:?}", &message[..10]);

        let signature = pair.sign(&message);

        log::info!("Signature: {:?}", &message[..10]);

        // sanity check
        // This should go in a separate unit test where we check the hdwallet crate.
        // this is keypair as hdwallet::generate vs keypair as hdwallet::create_keypair (in pair.sign)
        // TODO: fix this
        // let keypair = hdwallet::generate(Some(&seed)).expect("Failed to generate keypair");
        // let sig_bytes = keypair.sign(&message, None, false).expect("Signing failed");
        // assert_eq!(signature.as_ref(), sig_bytes, "Signatures should match");

        let public = pair.public();

        let result = ResonancePair::verify(&signature, message, &public);

        assert!(result, "Signature should verify");
    }

    #[test]
    fn test_sign_different_message_fails() {
        let seed = [0u8; 32];
        let pair = ResonancePair::from_seed(&seed).expect("Failed to create pair");
        let message = b"Hello, world!";
        let wrong_message = b"Goodbye, world!";

        let signature = pair.sign(message);
        let public = pair.public();

        assert!(
            !ResonancePair::verify(&signature, wrong_message, &public),
            "Signature should not verify with wrong message"
        );
    }

    #[test]
    fn test_wrong_signature_fails() {
        let seed = [0u8; 32];
        let pair = ResonancePair::from_seed(&seed).expect("Failed to create pair");
        let message = b"Hello, world!";

        let mut signature = pair.sign(message);
        let signature_bytes = signature.as_mut();
        // Corrupt the signature by flipping a bit
        if let Some(byte) = signature_bytes.get_mut(0) {
            *byte ^= 1;
        }
        let false_signature = ResonanceSignatureWithPublic::from_slice(signature_bytes)
            .expect("Failed to create signature");
        let public = pair.public();

        assert!(
            !ResonancePair::verify(&false_signature, message, &public),
            "Corrupted signature should not verify"
        );
    }

    #[test]
    fn test_different_seed_different_public() {
        let seed1 = vec![0u8; 32];
        let seed2 = vec![1u8; 32];
        let pair1 = ResonancePair::from_seed(&seed1).expect("Failed to create pair");
        let pair2 = ResonancePair::from_seed(&seed2).expect("Failed to create pair");

        let pub1 = pair1.public();
        let pub2 = pair2.public();

        assert_ne!(
            pub1.as_ref(),
            pub2.as_ref(),
            "Different seeds should produce different public keys"
        );
    }
}
