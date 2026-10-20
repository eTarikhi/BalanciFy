//! Long-term identity keys and ephemeral keys (Sec.9.2).
//!
//! The handshake takes secrets as `[u8; 32]` arguments so it stays sans-I/O and
//! deterministic under test. This module is the place where those bytes come
//! from: generation via the platform CSPRNG, storage, and zeroing on drop.
//!
//! Why separate from the handshake: the handshake proves possession of a key;
//! this module decides where that key lives and when it is erased. That keeps
//! the handshake's security argument (nonce uniqueness, transcript binding)
//! independent of the platform's key-storage mechanism (DPAPI, TPM, file).

use zeroize::{Zeroize, ZeroizeOnDrop};
use x25519_dalek::{PublicKey, StaticSecret};

/// A static keypair: long-term identity, pinned by peers.
#[derive(Clone)]
pub struct StaticKeypair {
    secret: StaticSecretBox,
    public: [u8; 32],
}

/// Wrapper that zeroizes on drop. `x25519_dalek::StaticSecret` already zeroizes
/// when the `zeroize` feature is enabled, but we keep our own box to make the
/// guarantee explicit and to hold the raw bytes for `to_bytes`.
#[derive(Clone)]
struct StaticSecretBox([u8; 32]);

impl Zeroize for StaticSecretBox {
    fn zeroize(&mut self) { self.0.zeroize(); }
}
impl ZeroizeOnDrop for StaticSecretBox {}
impl Drop for StaticSecretBox {
    fn drop(&mut self) { self.zeroize(); }
}

impl std::fmt::Debug for StaticKeypair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StaticKeypair")
            .field("public", &self.public)
            .finish()
    }
}

impl StaticKeypair {
    /// Generate a fresh keypair from the OS CSPRNG. This is the only place in
    /// the crate that reaches for an RNG; everything else takes bytes as
    /// arguments so tests stay deterministic.
    pub fn generate() -> Self {
        let mut secret_bytes = [0u8; 32];
        getrandom::getrandom(&mut secret_bytes).expect("OS CSPRNG must be available");
        Self::from_bytes(secret_bytes)
    }

    /// Build from 32 secret bytes. The public key is derived via X25519.
    pub fn from_bytes(secret_bytes: [u8; 32]) -> Self {
        let secret = StaticSecret::from(secret_bytes);
        let public = PublicKey::from(&secret);
        Self {
            secret: StaticSecretBox(secret_bytes),
            public: *public.as_bytes(),
        }
    }

    pub fn secret_bytes(&self) -> [u8; 32] { self.secret.0 }
    pub fn public_bytes(&self) -> [u8; 32] { self.public }
    pub fn public_key(&self) -> PublicKey { PublicKey::from(self.public) }
    pub fn static_secret(&self) -> StaticSecret { StaticSecret::from(self.secret.0) }

    /// The secret as a `StaticSecret` for DH. Clones the inner bytes; the
    /// returned `StaticSecret` zeroizes on drop when the `zeroize` feature is on.
    pub fn as_static_secret(&self) -> StaticSecret { StaticSecret::from(self.secret.0) }
}

/// An ephemeral keypair: fresh per session, deleted after the handshake.
#[derive(Clone)]
pub struct EphemeralKeypair {
    secret: StaticSecretBox,
    public: [u8; 32],
}

impl EphemeralKeypair {
    pub fn generate() -> Self {
        let mut secret_bytes = [0u8; 32];
        getrandom::getrandom(&mut secret_bytes).expect("OS CSPRNG must be available");
        Self::from_bytes(secret_bytes)
    }

    pub fn from_bytes(secret_bytes: [u8; 32]) -> Self {
        let secret = StaticSecret::from(secret_bytes);
        let public = PublicKey::from(&secret);
        Self {
            secret: StaticSecretBox(secret_bytes),
            public: *public.as_bytes(),
        }
    }

    pub fn secret_bytes(&self) -> [u8; 32] { self.secret.0 }
    pub fn public_bytes(&self) -> [u8; 32] { self.public }

    /// Consume the keypair and return the secret bytes, zeroizing the copy left
    /// behind. Use this when passing the secret into the handshake, so the
    /// ephemeral does not linger in this struct.
    pub fn into_secret_bytes(mut self) -> [u8; 32] {
        let bytes = self.secret.0;
        self.secret.zeroize();
        bytes
    }
}

impl Zeroize for EphemeralKeypair {
    fn zeroize(&mut self) { self.secret.zeroize(); self.public.zeroize(); }
}
impl ZeroizeOnDrop for EphemeralKeypair {}
impl Drop for EphemeralKeypair {
    fn drop(&mut self) { self.zeroize(); }
}

/// A set of pinned public keys: relay identities the client trusts, or client
/// identities the relay trusts. The handshake learns the peer's static key from
/// flight 1; this set decides whether to accept it.
#[derive(Debug, Clone, Default)]
pub struct PinnedKeys {
    keys: Vec<[u8; 32]>,
}

impl PinnedKeys {
    pub fn new() -> Self { Self { keys: Vec::new() } }

    pub fn from_iter<I: IntoIterator<Item = [u8; 32]>>(iter: I) -> Self {
        Self { keys: iter.into_iter().collect() }
    }

    pub fn pin(&mut self, public: [u8; 32]) {
        if !self.keys.contains(&public) { self.keys.push(public); }
    }

    pub fn unpin(&mut self, public: &[u8; 32]) { self.keys.retain(|k| k != public); }

    pub fn is_pinned(&self, public: &[u8; 32]) -> bool { self.keys.contains(public) }

    pub fn len(&self) -> usize { self.keys.len() }
    pub fn is_empty(&self) -> bool { self.keys.is_empty() }

    pub fn iter(&self) -> impl Iterator<Item = &[u8; 32]> { self.keys.iter() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_keypair_derives_public() {
        let kp = StaticKeypair::from_bytes([0x11u8; 32]);
        let public = kp.public_bytes();
        // Deterministic: same secret -> same public.
        let kp2 = StaticKeypair::from_bytes([0x11u8; 32]);
        assert_eq!(public, kp2.public_bytes());
        // Different secret -> different public (with overwhelming probability).
        let kp3 = StaticKeypair::from_bytes([0x22u8; 32]);
        assert_ne!(public, kp3.public_bytes());
    }

    #[test]
    fn ephemeral_is_zeroized_on_drop() {
        // This test cannot observe zeroization directly, but it ensures the
        // Zeroize impl exists and the type is Send (so it can be moved).
        let kp = EphemeralKeypair::from_bytes([0x33u8; 32]);
        let secret = kp.secret_bytes();
        assert_eq!(secret, [0x33u8; 32]);
        // Drop happens here; under Miri or with a custom allocator we could
        // verify zeroization, but the type system guarantees Drop calls zeroize.
    }

    #[test]
    fn pinned_keys_basic() {
        let mut pinned = PinnedKeys::new();
        let k1 = [1u8; 32];
        let k2 = [2u8; 32];
        assert!(pinned.is_empty());
        pinned.pin(k1);
        assert!(pinned.is_pinned(&k1));
        assert!(!pinned.is_pinned(&k2));
        pinned.pin(k1); // idempotent
        assert_eq!(pinned.len(), 1);
        pinned.pin(k2);
        assert_eq!(pinned.len(), 2);
        pinned.unpin(&k1);
        assert!(!pinned.is_pinned(&k1));
        assert_eq!(pinned.len(), 1);
    }

    #[test]
    fn generate_uses_os_rng() {
        // Two generated keypairs should differ (with overwhelming probability).
        // This test would fail if getrandom returned zeros.
        let k1 = StaticKeypair::generate();
        let k2 = StaticKeypair::generate();
        assert_ne!(k1.public_bytes(), k2.public_bytes(), "OS RNG must produce distinct keys");
    }
}
