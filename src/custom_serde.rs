/// Custom serde serialization module for Age Recipient
///
/// Provides safe serialization/deserialization for age::x25519::Recipient using
/// the built-in to_string() and from_str() methods.
pub mod age_recipient_serde {
    use age::x25519::Recipient as AgeRecipient;
    use serde::{Deserialize, Serialize};
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(value: &Option<AgeRecipient>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(recipient) => recipient.to_string().serialize(serializer),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<AgeRecipient>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(Some(
            s.parse::<AgeRecipient>()
                .map_err(serde::de::Error::custom)?,
        ))
    }

    #[cfg(test)]
    #[allow(clippy::unwrap_used, clippy::expect_used)]
    mod tests {
        use super::*;
        use age::x25519::{Identity as AgeIdentity, Recipient as AgeRecipient};

        #[test]
        fn test_age_recipient_serialize() {
            let identity = AgeIdentity::generate();
            let recipient = identity.to_public();
            let serialized = serde_json::to_string(&recipient.to_string()).unwrap();

            // Verify it starts with the expected prefix in the serialized string
            assert!(serialized.starts_with("\"age"));
        }

        #[test]
        fn test_age_recipient_deserialize() {
            let identity = AgeIdentity::generate();
            let recipient = identity.to_public();
            let recipient_string = recipient.to_string();

            // Test that we can parse it back
            let parsed = recipient_string.parse::<AgeRecipient>().unwrap();

            // Verify they produce the same string representation
            assert_eq!(recipient.to_string(), parsed.to_string());
        }

        #[test]
        fn test_age_recipient_serde_roundtrip() {
            let identity = AgeIdentity::generate();
            let recipient = identity.to_public();

            // Serialize
            let mut serializer = serde_json::Serializer::new(Vec::new());
            serialize(&Some(recipient.clone()), &mut serializer).unwrap();
            let serialized_bytes = serializer.into_inner();
            let serialized_str = String::from_utf8(serialized_bytes).unwrap();

            // Deserialize
            let mut deserializer = serde_json::Deserializer::from_str(&serialized_str);
            let deserialized = deserialize(&mut deserializer).unwrap();

            // Verify round-trip equality
            assert_eq!(recipient.to_string(), deserialized.unwrap().to_string(),);
        }
    }
}
