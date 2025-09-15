use std::marker::PhantomData;

use anyhow::Result;
use ed25519_dalek::Signature;
use iroh::{PublicKey, SecretKey};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

#[derive(Debug, Serialize, Deserialize)]
pub struct SignedMessage<M: Serialize + DeserializeOwned> {
    phantom: PhantomData<M>,
    from: PublicKey,
    data: Vec<u8>,
    signature: Signature,
}

impl<M: Serialize + DeserializeOwned> SignedMessage<M> {
    pub fn verify_and_decode(bytes: &[u8]) -> Result<(PublicKey, M)> {
        let signed_message: Self = serde_json::from_slice(bytes)?;
        signed_message
            .from
            .verify(&signed_message.data, &signed_message.signature)?;
        let message: M = serde_json::from_slice(&signed_message.data)?;
        Ok((signed_message.from, message))
    }

    pub fn sign_and_encode(secret_key: &SecretKey, message: &M) -> Result<Vec<u8>> {
        let data = serde_json::to_vec(&message)?;
        let signature = secret_key.sign(&data);
        let from: PublicKey = secret_key.public();
        let signed_message = Self {
            phantom: PhantomData,
            from,
            data,
            signature,
        };
        let encoded = serde_json::to_vec(&signed_message)?;
        Ok(encoded)
    }
}

pub trait MessageSigner: Serialize + DeserializeOwned {
    fn sign(&self, secret_key: &SecretKey) -> Result<Vec<u8>> {
        SignedMessage::<Self>::sign_and_encode(secret_key, self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::SecretKey;
    use rand::thread_rng;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct TestMessage {
        content: String,
        number: u32,
    }

    impl MessageSigner for TestMessage {}

    #[test]
    fn test_sign_and_encode_produces_valid_bytes() {
        let secret_key = SecretKey::generate(&mut thread_rng());
        let message = TestMessage {
            content: "Hello World".to_string(),
            number: 42,
        };

        let result = SignedMessage::sign_and_encode(&secret_key, &message);
        assert!(result.is_ok());

        let encoded = result.unwrap();
        assert!(!encoded.is_empty());

        // Should be valid JSON
        let parsed: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
        assert!(parsed.is_object());
    }

    #[test]
    fn test_verify_and_decode_valid_message() {
        let secret_key = SecretKey::generate(&mut thread_rng());
        let expected_public_key = secret_key.public();
        let message = TestMessage {
            content: "Test Message".to_string(),
            number: 123,
        };

        let encoded = SignedMessage::sign_and_encode(&secret_key, &message).unwrap();
        let result = SignedMessage::<TestMessage>::verify_and_decode(&encoded);

        assert!(result.is_ok());
        let (public_key, decoded_message) = result.unwrap();
        assert_eq!(public_key, expected_public_key);
        assert_eq!(decoded_message, message);
    }

    #[test]
    fn test_round_trip_sign_then_verify() {
        let secret_key = SecretKey::generate(&mut thread_rng());
        let original_message = TestMessage {
            content: "Round trip test".to_string(),
            number: 999,
        };

        // Sign the message
        let encoded = SignedMessage::sign_and_encode(&secret_key, &original_message).unwrap();

        // Verify and decode
        let (public_key, decoded_message) =
            SignedMessage::<TestMessage>::verify_and_decode(&encoded).unwrap();

        assert_eq!(public_key, secret_key.public());
        assert_eq!(decoded_message, original_message);
    }

    #[test]
    fn test_verify_fails_with_tampered_data() {
        let secret_key = SecretKey::generate(&mut thread_rng());
        let message = TestMessage {
            content: "Original message".to_string(),
            number: 456,
        };

        let mut encoded = SignedMessage::sign_and_encode(&secret_key, &message).unwrap();

        // Tamper with the encoded data
        if let Some(last_byte) = encoded.last_mut() {
            *last_byte = last_byte.wrapping_add(1);
        }

        let result = SignedMessage::<TestMessage>::verify_and_decode(&encoded);
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_fails_with_malformed_json() {
        let malformed_data = b"{ invalid json }";
        let result = SignedMessage::<TestMessage>::verify_and_decode(malformed_data);
        assert!(result.is_err());
    }

    #[test]
    fn test_message_signer_trait() {
        let secret_key = SecretKey::generate(&mut thread_rng());
        let message = TestMessage {
            content: "Trait test".to_string(),
            number: 789,
        };

        let encoded = message.sign(&secret_key).unwrap();
        let (public_key, decoded) =
            SignedMessage::<TestMessage>::verify_and_decode(&encoded).unwrap();

        assert_eq!(public_key, secret_key.public());
        assert_eq!(decoded, message);
    }

    #[test]
    fn test_signature_verification_with_different_key() {
        let secret_key1 = SecretKey::generate(&mut thread_rng());
        let secret_key2 = SecretKey::generate(&mut thread_rng());
        let message = TestMessage {
            content: "Key mismatch test".to_string(),
            number: 111,
        };

        // Sign with key1
        let encoded = SignedMessage::sign_and_encode(&secret_key1, &message).unwrap();

        // Verify should still work (it uses the embedded public key)
        let result = SignedMessage::<TestMessage>::verify_and_decode(&encoded);
        assert!(result.is_ok());

        let (public_key, _) = result.unwrap();
        assert_eq!(public_key, secret_key1.public());
        assert_ne!(public_key, secret_key2.public());
    }
}
