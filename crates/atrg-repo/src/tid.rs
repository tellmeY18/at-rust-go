//! TID (Timestamp Identifier) generation and parsing.
//!
//! TIDs are base32-sortable timestamps used as record keys in the AT Protocol.
//! Format: 13 characters from the charset `234567abcdefghijklmnopqrstuvwxyz`.
//! Encodes microseconds since Unix epoch in the upper bits, random clock ID in the lower 10 bits.

use crate::error::RepoError;

/// The base32-sortable charset used for TID encoding.
const TID_CHARSET: &[u8; 32] = b"234567abcdefghijklmnopqrstuvwxyz";

/// Expected length of a TID string.
const TID_LEN: usize = 13;

/// A TID (Timestamp Identifier) — base32-sortable, used as record keys.
///
/// TIDs encode a microsecond timestamp and a random clock ID into a
/// 13-character base32-sortable string. They are lexicographically ordered
/// by creation time, making them ideal for record keys in AT Protocol
/// repositories.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Tid(String);

impl Tid {
    /// Generate a new TID from the current timestamp and a random clock ID.
    ///
    /// Uses `std::time::SystemTime` for the timestamp and `rand` for the
    /// 10-bit clock ID.
    pub fn now() -> Self {
        use rand::Rng;
        use std::time::{SystemTime, UNIX_EPOCH};

        let micros = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before Unix epoch")
            .as_micros() as u64;

        let clock_id: u64 = rand::thread_rng().gen_range(0..1024);

        // TID is a 64-bit value: upper 54 bits are microseconds, lower 10 bits are clock ID.
        let tid_value = (micros << 10) | clock_id;

        Self(encode_base32_sortable(tid_value))
    }

    /// Parse a TID string, validating format.
    ///
    /// A valid TID is exactly 13 characters long, using only characters from
    /// the base32-sortable charset (`234567abcdefghijklmnopqrstuvwxyz`).
    ///
    /// # Errors
    ///
    /// Returns [`RepoError::InvalidTid`] if the string is not a valid TID.
    pub fn parse(s: &str) -> Result<Self, RepoError> {
        if s.len() != TID_LEN {
            return Err(RepoError::InvalidTid(format!(
                "TID must be {TID_LEN} characters, got {}",
                s.len()
            )));
        }

        for (i, ch) in s.chars().enumerate() {
            if !TID_CHARSET.contains(&(ch as u8)) {
                return Err(RepoError::InvalidTid(format!(
                    "invalid character '{}' at position {} in TID",
                    ch, i
                )));
            }
        }

        Ok(Self(s.to_string()))
    }

    /// Return the inner string representation.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the TID and return the inner string.
    pub fn into_string(self) -> String {
        self.0
    }
}

impl std::fmt::Display for Tid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl serde::Serialize for Tid {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for Tid {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Tid::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// Encode a 64-bit value as a 13-character base32-sortable string (big-endian).
fn encode_base32_sortable(mut value: u64) -> String {
    let mut buf = [b'2'; TID_LEN]; // '2' is the zero character in our charset

    for i in (0..TID_LEN).rev() {
        let idx = (value & 0x1F) as usize;
        buf[i] = TID_CHARSET[idx];
        value >>= 5;
    }

    // SAFETY: all bytes come from TID_CHARSET which is valid ASCII/UTF-8.
    String::from_utf8(buf.to_vec()).expect("TID charset is valid UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_produces_13_chars() {
        let tid = Tid::now();
        assert_eq!(tid.as_str().len(), 13);
    }

    #[test]
    fn now_uses_valid_charset() {
        let tid = Tid::now();
        for ch in tid.as_str().chars() {
            assert!(
                TID_CHARSET.contains(&(ch as u8)),
                "unexpected character '{ch}' in TID"
            );
        }
    }

    #[test]
    fn now_is_monotonically_increasing() {
        let a = Tid::now();
        // Small sleep to ensure different timestamp.
        std::thread::sleep(std::time::Duration::from_millis(2));
        let b = Tid::now();
        assert!(b.as_str() >= a.as_str(), "TIDs should be sortable by time");
    }

    #[test]
    fn parse_valid_tid() {
        // 13 chars from the valid charset
        let tid = Tid::parse("2222222222222").unwrap();
        assert_eq!(tid.as_str(), "2222222222222");
    }

    #[test]
    fn parse_rejects_too_short() {
        let result = Tid::parse("222222");
        assert!(result.is_err());
        match result.unwrap_err() {
            RepoError::InvalidTid(msg) => assert!(msg.contains("13 characters")),
            other => panic!("expected InvalidTid, got: {other}"),
        }
    }

    #[test]
    fn parse_rejects_too_long() {
        let result = Tid::parse("22222222222222");
        assert!(result.is_err());
    }

    #[test]
    fn parse_rejects_invalid_chars() {
        let result = Tid::parse("222222222222A");
        assert!(result.is_err());
        match result.unwrap_err() {
            RepoError::InvalidTid(msg) => assert!(msg.contains("invalid character")),
            other => panic!("expected InvalidTid, got: {other}"),
        }
    }

    #[test]
    fn parse_rejects_uppercase() {
        let result = Tid::parse("222222222222Z");
        assert!(result.is_err());
    }

    #[test]
    fn parse_rejects_0_and_1() {
        // '0' and '1' are not in the base32-sortable charset
        assert!(Tid::parse("0222222222222").is_err());
        assert!(Tid::parse("1222222222222").is_err());
    }

    #[test]
    fn display_matches_as_str() {
        let tid = Tid::now();
        assert_eq!(format!("{tid}"), tid.as_str());
    }

    #[test]
    fn roundtrip_through_generated() {
        let tid = Tid::now();
        let parsed = Tid::parse(tid.as_str()).unwrap();
        assert_eq!(tid, parsed);
    }

    #[test]
    fn encode_zero_value() {
        let encoded = encode_base32_sortable(0);
        assert_eq!(encoded, "2222222222222");
    }

    #[test]
    fn test_into_string() {
        let tid = Tid::now();
        let s = tid.clone().into_string();
        assert_eq!(s.len(), 13);
        assert_eq!(s, tid.as_str());
    }

    #[test]
    fn test_serde_roundtrip() {
        let tid = Tid::now();
        let json = serde_json::to_string(&tid).unwrap();
        let deserialized: Tid = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.as_str(), tid.as_str());
    }

    #[test]
    fn test_deserialize_invalid_tid_wrong_length() {
        let result = serde_json::from_str::<Tid>(r#""abc""#);
        assert!(result.is_err());
    }

    #[test]
    fn test_deserialize_invalid_tid_bad_chars() {
        // 13 chars but with invalid uppercase
        let result = serde_json::from_str::<Tid>(r#""AAAAAAAAAAAAA""#);
        assert!(result.is_err());
    }

    #[test]
    fn test_deserialize_valid_tid() {
        let tid: Tid = serde_json::from_str(r#""2222222222222""#).unwrap();
        assert_eq!(tid.as_str(), "2222222222222");
    }
}
