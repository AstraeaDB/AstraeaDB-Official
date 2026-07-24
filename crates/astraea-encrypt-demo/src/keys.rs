use rand::Rng;
use serde::{Deserialize, Serialize};

/// A symmetric key for the demonstration encryption scheme.
/// In production, this would be replaced by an FHE key pair (e.g., tfhe-rs).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretKey {
    key_bytes: Vec<u8>,
}

/// Public key (placeholder for FHE schemes where the server needs it).
/// In a real FHE setup, the public key allows the server to perform
/// computations on encrypted data without seeing plaintext.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicKey {
    key_bytes: Vec<u8>,
}

/// A key pair consisting of a public and secret key.
#[derive(Debug, Clone)]
pub struct KeyPair {
    pub public_key: PublicKey,
    pub secret_key: SecretKey,
}

impl KeyPair {
    /// Generate a new random key pair with 32 bytes of entropy.
    pub fn generate() -> Self {
        let mut rng = rand::thread_rng();
        let mut secret_bytes = vec![0u8; 32];
        rng.fill(&mut secret_bytes[..]);

        let mut public_bytes = vec![0u8; 32];
        rng.fill(&mut public_bytes[..]);

        KeyPair {
            public_key: PublicKey {
                key_bytes: public_bytes,
            },
            secret_key: SecretKey {
                key_bytes: secret_bytes,
            },
        }
    }
}

impl SecretKey {
    /// Create a secret key from raw bytes.
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self { key_bytes: bytes }
    }

    /// Get the raw key bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.key_bytes
    }

    /// Encrypt plaintext bytes using XOR-based encryption for demonstration.
    ///
    /// This is NOT cryptographically secure. In production, this would be
    /// replaced by FHE encryption (e.g., Microsoft SEAL or tfhe-rs).
    /// The key is expanded via a simple repeating-key XOR.
    pub fn encrypt_bytes(&self, plaintext: &[u8]) -> Vec<u8> {
        plaintext
            .iter()
            .enumerate()
            .map(|(i, &byte)| byte ^ self.key_bytes[i % self.key_bytes.len()])
            .collect()
    }

    /// Decrypt ciphertext bytes using XOR-based decryption (symmetric).
    ///
    /// Since XOR is its own inverse, decryption is the same operation
    /// as encryption with the same key.
    pub fn decrypt_bytes(&self, ciphertext: &[u8]) -> Vec<u8> {
        // XOR is symmetric: encrypt == decrypt
        self.encrypt_bytes(ciphertext)
    }

    /// Produce a deterministic tag for a given plaintext.
    ///
    /// This is an HMAC-like construction for demonstration purposes.
    /// The tag is deterministic so that the same plaintext always produces
    /// the same tag, enabling equality checks on encrypted data without
    /// revealing the plaintext to the server.
    ///
    /// In production, this would use a proper HMAC (e.g., HMAC-SHA256).
    pub fn deterministic_tag(&self, plaintext: &[u8]) -> Vec<u8> {
        // Simple hash-like construction: mix plaintext with key and fold
        // into a 32-byte tag regardless of input length.
        let mut tag = vec![0u8; 32];
        for (i, &byte) in plaintext.iter().enumerate() {
            let key_byte = self.key_bytes[i % self.key_bytes.len()];
            // Mix the input position into the tag to avoid collisions
            // for inputs that differ only by length or reordering.
            let mixed = byte.wrapping_add(key_byte).wrapping_add((i & 0xFF) as u8);
            tag[i % 32] ^= mixed;
        }
        // Second pass: cascade each byte into the next for diffusion.
        for i in 1..32 {
            tag[i] = tag[i].wrapping_add(tag[i - 1]);
        }
        tag
    }
}

impl PublicKey {
    /// Create a public key from raw bytes.
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self { key_bytes: bytes }
    }

    /// Get the raw key bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.key_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_key_pair() {
        let kp = KeyPair::generate();
        assert_eq!(kp.secret_key.key_bytes.len(), 32);
        assert_eq!(kp.public_key.key_bytes.len(), 32);
        // Keys should not be all zeros (astronomically unlikely).
        assert!(kp.secret_key.key_bytes.iter().any(|&b| b != 0));
        assert!(kp.public_key.key_bytes.iter().any(|&b| b != 0));
    }

    #[test]
    fn test_generate_produces_different_keys() {
        let kp1 = KeyPair::generate();
        let kp2 = KeyPair::generate();
        // Two generated key pairs should differ (astronomically unlikely to collide).
        assert_ne!(kp1.secret_key.key_bytes, kp2.secret_key.key_bytes);
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let kp = KeyPair::generate();
        let plaintext = b"Hello, AstraeaDB!";
        let ciphertext = kp.secret_key.encrypt_bytes(plaintext);
        // Ciphertext should differ from plaintext.
        assert_ne!(&ciphertext, plaintext);
        let decrypted = kp.secret_key.decrypt_bytes(&ciphertext);
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_encrypt_decrypt_empty() {
        let kp = KeyPair::generate();
        let plaintext = b"";
        let ciphertext = kp.secret_key.encrypt_bytes(plaintext);
        assert!(ciphertext.is_empty());
        let decrypted = kp.secret_key.decrypt_bytes(&ciphertext);
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_encrypt_decrypt_long_data() {
        let kp = KeyPair::generate();
        // Data longer than the key (32 bytes).
        let plaintext: Vec<u8> = (0..256).map(|i| (i % 256) as u8).collect();
        let ciphertext = kp.secret_key.encrypt_bytes(&plaintext);
        assert_ne!(ciphertext, plaintext);
        let decrypted = kp.secret_key.decrypt_bytes(&ciphertext);
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_deterministic_tag_consistency() {
        let kp = KeyPair::generate();
        let tag1 = kp.secret_key.deterministic_tag(b"Person");
        let tag2 = kp.secret_key.deterministic_tag(b"Person");
        assert_eq!(tag1, tag2);
    }

    #[test]
    fn test_deterministic_tag_differs_for_different_input() {
        let kp = KeyPair::generate();
        let tag1 = kp.secret_key.deterministic_tag(b"Person");
        let tag2 = kp.secret_key.deterministic_tag(b"Company");
        assert_ne!(tag1, tag2);
    }

    #[test]
    fn test_deterministic_tag_differs_for_different_keys() {
        let kp1 = KeyPair::generate();
        let kp2 = KeyPair::generate();
        let tag1 = kp1.secret_key.deterministic_tag(b"Person");
        let tag2 = kp2.secret_key.deterministic_tag(b"Person");
        assert_ne!(tag1, tag2);
    }

    #[test]
    fn test_secret_key_from_bytes() {
        let bytes = vec![42u8; 32];
        let sk = SecretKey::from_bytes(bytes.clone());
        assert_eq!(sk.as_bytes(), &bytes[..]);
    }

    #[test]
    fn test_public_key_from_bytes() {
        let bytes = vec![99u8; 32];
        let pk = PublicKey::from_bytes(bytes.clone());
        assert_eq!(pk.as_bytes(), &bytes[..]);
    }
}
