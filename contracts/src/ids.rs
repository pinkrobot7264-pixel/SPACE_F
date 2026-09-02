//! Shared identity types (M0.3).
//!
//! Two shapes:
//!  * UUIDv7 identities (`FileId`, `DirectoryId`, ...): time-ordered, prefixed on
//!    the wire so a stray `v_...` cannot be parsed as an `f_...`.
//!  * [`ChunkId`]: *content* address, not identity -- `b3:<64 lowercase hex>`.
//!
//! The type system stops a `VersionId` being passed where a `FileId` is
//! expected; there is a documented `trybuild`-style note in the tests.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::errors::SpaceError;

macro_rules! uuid_id {
    ($name:ident, $prefix:literal) => {
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// A fresh, time-ordered id.
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            /// Parse the prefixed string form produced by `Display`.
            pub fn parse(s: &str) -> Result<Self, SpaceError> {
                let rest = s.strip_prefix($prefix).ok_or_else(|| {
                    SpaceError::invalid_param(concat!("expected ", $prefix, " prefix"))
                })?;
                Uuid::parse_str(rest)
                    .map(Self)
                    .map_err(|_| SpaceError::invalid_param("malformed uuid"))
            }

            /// The raw UUID, for storage layers that need it.
            pub fn as_uuid(&self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}{}", $prefix, self.0)
            }
        }

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self)
            }
        }

        impl std::str::FromStr for $name {
            type Err = SpaceError;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Self::parse(s)
            }
        }
    };
}

uuid_id!(FileId, "f_");
uuid_id!(DirectoryId, "d_");
uuid_id!(VersionId, "v_");
uuid_id!(ManifestId, "m_");
uuid_id!(RequestId, "r_");
uuid_id!(OperationId, "o_");

/// A content address: the BLAKE3 hash of the bytes it names.
///
/// `ChunkId` is intentionally *not* an identity type -- two identical byte
/// ranges have the same `ChunkId`, and that is the whole point.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChunkId(String); // "b3:<64 lowercase hex>"

impl ChunkId {
    /// The content address of `bytes`.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self(format!("b3:{}", blake3::hash(bytes).to_hex()))
    }

    /// Parse and validate the `b3:` string form.
    pub fn parse(s: &str) -> Result<Self, SpaceError> {
        let hex = s
            .strip_prefix("b3:")
            .ok_or_else(|| SpaceError::invalid_param("chunk id must start with b3:"))?;
        if hex.len() != 64 {
            return Err(SpaceError::invalid_param(
                "chunk id hash must be 64 hex chars",
            ));
        }
        if !hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(SpaceError::invalid_param("chunk id must be lowercase hex"));
        }
        Ok(Self(s.to_string()))
    }

    /// Does this id actually address `bytes`?
    pub fn verify(&self, bytes: &[u8]) -> bool {
        *self == Self::from_bytes(bytes)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ChunkId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::fmt::Debug for ChunkId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::str::FromStr for ChunkId {
    type Err = SpaceError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_id_round_trips() {
        let id = FileId::new();
        let s = id.to_string();
        assert!(s.starts_with("f_"));
        assert_eq!(FileId::parse(&s).unwrap(), id);
    }

    #[test]
    fn file_id_rejects_bad_input() {
        assert!(FileId::parse("").is_err());
        assert!(FileId::parse(&VersionId::new().to_string()).is_err()); // wrong prefix
        assert!(FileId::parse("01890a5d-ac96-774b-bcce-b302099a8057").is_err()); // no prefix
        assert!(FileId::parse("f_not-a-uuid").is_err());
        assert!(FileId::parse(&format!("f_{} ", uuid::Uuid::nil())).is_err()); // trailing space
        assert!(FileId::parse("f_01890a5d\0ac96").is_err()); // embedded null
    }

    #[test]
    fn chunk_id_from_empty_is_stable() {
        assert_eq!(ChunkId::from_bytes(b""), ChunkId::from_bytes(b""));
        // BLAKE3 of the empty input, well-known vector.
        assert_eq!(
            ChunkId::from_bytes(b"").as_str(),
            "b3:af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
        );
    }

    #[test]
    fn chunk_id_matches_known_blake3_vector_for_abc() {
        assert_eq!(
            ChunkId::from_bytes(b"abc").as_str(),
            "b3:6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85"
        );
    }

    #[test]
    fn chunk_id_parse_rejects_malformed() {
        let good = ChunkId::from_bytes(b"abc").to_string();
        assert!(ChunkId::parse(&good).is_ok());
        assert!(ChunkId::parse(&good.replace("b3:", "")).is_err()); // no prefix
        assert!(ChunkId::parse(&good.to_uppercase()).is_err()); // uppercase hex
        assert!(ChunkId::parse("b3:6437b3ac").is_err()); // 8 chars
        assert!(ChunkId::parse(&format!("{good}0")).is_err()); // 65 chars
        assert!(ChunkId::parse(&format!("b3:{}", "g".repeat(64))).is_err()); // non-hex
    }

    #[test]
    fn chunk_id_verify_detects_a_single_flipped_byte() {
        let bytes = b"the quick brown fox".to_vec();
        let id = ChunkId::from_bytes(&bytes);
        let mut bad = bytes.clone();
        bad[0] ^= 0x01;
        assert!(id.verify(&bytes));
        assert!(!id.verify(&bad));
    }
}
