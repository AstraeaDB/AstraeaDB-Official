//! # ⚠️ DEMO-ONLY CRYPTO — NOT FOR PRODUCTION
//!
//! This crate illustrates the shape of a searchable-encryption API over
//! the AstraeaDB graph. **The primitives used here are not
//! cryptographically secure.** Specifically:
//!
//! - `encrypt` / `decrypt` use byte-wise XOR with the secret key. This
//!   is a one-time-pad only when the key is as long as the data and
//!   never reused — neither condition holds here.
//! - `deterministic_tag` is a hand-rolled deterministic hash, not a
//!   real MAC/HMAC. It has no keyed-collision-resistance guarantees.
//! - [`PublicKey`] and [`SecretKey`] are cryptographically unrelated:
//!   [`KeyPair::generate`] derives the public key by hashing the
//!   secret, which is fine as an identifier but gives no asymmetric
//!   security property.
//!
//! Real transport security (TLS termination, mTLS, token-based auth)
//! is handled by [`astraea-server`] via `rustls`, not here.
//!
//! If you want to experiment with the API shape, this crate is fine.
//! If you need actual security, do not ship anything downstream of it
//! until the primitives are replaced with vetted alternatives.
//!
//! astraeadb-issues.md #13.

pub mod keys;
pub mod encrypted;
pub mod engine;

pub use keys::{KeyPair, PublicKey, SecretKey};
pub use encrypted::{EncryptedValue, EncryptedLabel, EncryptedNode};
pub use engine::EncryptedQueryEngine;
