//! UUID generation strategy and collision retry policy for inserts.

use serde::{Deserialize, Serialize};

/// How to generate the UUID for a primary key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum UuidStrategy {
    /// UUIDv4: random, collision probability ≈ 2^−122 (negligible).
    V4,
    /// UUIDv5: deterministic (namespace + name), collisions ARE possible.
    V5 {
        /// The namespace UUID for deterministic generation.
        namespace: String,
    },
    /// UUIDv7: timestamp-ordered, collision probability ≈ 2^−122 per millisecond.
    V7,
    /// User provides the ID; framework does not generate.
    UserProvided,
}

/// What to do when a UUID collision is detected.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CollisionPolicy {
    /// Retry up to N times with a newly generated UUID.
    Retry(u32),
    /// Fail immediately with `Error::IdCollision`.
    Fail,
}

impl Default for CollisionPolicy {
    fn default() -> Self {
        CollisionPolicy::Retry(3)
    }
}

/// Policy for insert operations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InsertPolicy {
    /// UUID generation strategy.
    pub uuid: UuidStrategy,
    /// Collision handling.
    pub collision: CollisionPolicy,
}

impl Default for InsertPolicy {
    fn default() -> Self {
        InsertPolicy {
            uuid: UuidStrategy::V4,
            collision: CollisionPolicy::Retry(3),
        }
    }
}

impl InsertPolicy {
    /// Create a policy with UUIDv4 (random) and default retry.
    pub fn v4() -> Self {
        Self::default()
    }

    /// Create a policy with UUIDv5 (deterministic) and explicit retry.
    pub fn v5(namespace: impl Into<String>, max_retries: u32) -> Self {
        InsertPolicy {
            uuid: UuidStrategy::V5 {
                namespace: namespace.into(),
            },
            collision: CollisionPolicy::Retry(max_retries),
        }
    }

    /// Create a policy with UUIDv7 (timestamp-ordered) and default retry.
    pub fn v7() -> Self {
        InsertPolicy {
            uuid: UuidStrategy::V7,
            collision: CollisionPolicy::Retry(3),
        }
    }

    /// Create a fail-fast policy (no retry).
    pub fn fail_fast(mut self) -> Self {
        self.collision = CollisionPolicy::Fail;
        self
    }

    /// Generate a UUID according to the strategy.
    pub fn generate_uuid(&self, name: Option<&str>) -> Result<uuid::Uuid, crate::Error> {
        match &self.uuid {
            UuidStrategy::V4 => Ok(uuid::Uuid::new_v4()),
            UuidStrategy::V7 => {
                // UUIDv7: timestamp (48 bits) + random (74 bits)
                // Simplified implementation using v4 as base with timestamp prefix
                // (full v7 implementation can use the uuid crate's v7 feature when available)
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                let mut bytes = *uuid::Uuid::new_v4().as_bytes();
                // Set version to 7 and variant to RFC4122
                bytes[6] = (bytes[6] & 0x0F) | 0x70;
                bytes[8] = (bytes[8] & 0x3F) | 0x80;
                // Embed timestamp in the first 6 bytes
                bytes[0] = (now >> 40) as u8;
                bytes[1] = (now >> 32) as u8;
                bytes[2] = (now >> 24) as u8;
                bytes[3] = (now >> 16) as u8;
                bytes[4] = (now >> 8) as u8;
                bytes[5] = now as u8;
                Ok(uuid::Uuid::from_bytes(bytes))
            }
            UuidStrategy::V5 { namespace } => {
                let name = name.ok_or_else(|| {
                    crate::Error::Config("UUIDv5 requires a name parameter".into())
                })?;
                let ns = uuid::Uuid::parse_str(namespace)
                    .map_err(|e| crate::Error::Config(format!("invalid namespace: {e}")))?;
                Ok(uuid::Uuid::new_v5(&ns, name.as_bytes()))
            }
            UuidStrategy::UserProvided => Err(crate::Error::Config(
                "UserProvided strategy does not generate UUIDs".into(),
            )),
        }
    }
}
