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

/// Custom serde serialization module for AgeIdentity
///
/// Provides safe serialization/deserialization for age::x25519::Identity using
/// the built-in to_string() and from_str() methods. The age identity is serialized
/// as a string in Bech32 format with "AGE-SECRET-KEY-1" prefix.
pub mod age_identity_serde {
    use age::secrecy::ExposeSecret;
    use age::x25519::Identity as AgeIdentity;
    use serde::{Deserialize, Serialize};
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(age_key: &AgeIdentity, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        age_key.to_string().expose_secret().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<AgeIdentity, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse::<AgeIdentity>().map_err(serde::de::Error::custom)
    }

    #[cfg(test)]
    #[allow(clippy::unwrap_used, clippy::expect_used)]
    mod tests {
        use super::*;

        #[test]
        fn test_age_identity_serialize() {
            let age_identity = AgeIdentity::generate();
            let serialized =
                serde_json::to_string(&age_identity.to_string().expose_secret()).unwrap();

            // Verify it starts with the expected prefix in the serialized string
            assert!(serialized.contains("AGE-SECRET-KEY-1"));
        }

        #[test]
        fn test_age_identity_deserialize() {
            let age_identity = AgeIdentity::generate();
            let identity_string = age_identity.to_string().expose_secret().to_string();

            // Test that we can parse it back
            let parsed = identity_string.parse::<AgeIdentity>().unwrap();

            // Verify they produce the same string representation
            assert_eq!(
                age_identity.to_string().expose_secret(),
                parsed.to_string().expose_secret()
            );
        }

        #[test]
        fn test_age_identity_serde_roundtrip() {
            let original = AgeIdentity::generate();

            // Serialize
            let mut serializer = serde_json::Serializer::new(Vec::new());
            serialize(&original, &mut serializer).unwrap();
            let serialized_bytes = serializer.into_inner();
            let serialized_str = String::from_utf8(serialized_bytes).unwrap();

            // Deserialize
            let mut deserializer = serde_json::Deserializer::from_str(&serialized_str);
            let deserialized = deserialize(&mut deserializer).unwrap();

            // Verify round-trip equality
            assert_eq!(
                original.to_string().expose_secret(),
                deserialized.to_string().expose_secret()
            );
        }
    }
}
