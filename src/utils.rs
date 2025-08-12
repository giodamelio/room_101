/// Creates a TopicId from a string literal, padding with zeros to 32 bytes at compile time
/// Fails at compile time if the string is longer than 32 bytes
macro_rules! topic_id {
    ($s:literal) => {{
        const BYTES: &[u8] = $s.as_bytes();
        const LEN: usize = BYTES.len();
        const _: () = assert!(LEN <= 32, "Topic string is too long (max 32 bytes)");
        const PADDED: [u8; 32] = {
            let mut arr = [0u8; 32];
            let mut i = 0;
            while i < LEN {
                arr[i] = BYTES[i];
                i += 1;
            }
            arr
        };
        TopicId::from_bytes(PADDED)
    }};
}

pub(crate) use topic_id;
