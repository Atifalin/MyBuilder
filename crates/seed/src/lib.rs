//! Deterministic seeding, hashing and counter-based random streams.
//!
//! All generation code derives entropy through this crate so that output
//! depends only on the project seed and stable entity paths, never on
//! generation order or platform.

use std::fmt;
use std::str::FromStr;

/// BLAKE3 hash of `data`.
pub fn hash_bytes(data: &[u8]) -> [u8; 32] {
    *blake3::hash(data).as_bytes()
}

/// Derive a child seed from a parent seed, a domain `kind` (e.g. "region",
/// "stage") and a `key` within that domain.
///
/// The preimage is `parent || len64(kind) || kind || len64(key) || key`
/// with little-endian length prefixes so that no two (kind, key) pairs can
/// collide on framing. The first 8 bytes of the BLAKE3 digest are taken
/// little-endian.
pub fn derive(parent: u64, kind: &str, key: &str) -> u64 {
    let mut h = blake3::Hasher::new();
    h.update(&parent.to_le_bytes());
    h.update(&(kind.len() as u64).to_le_bytes());
    h.update(kind.as_bytes());
    h.update(&(key.len() as u64).to_le_bytes());
    h.update(key.as_bytes());
    u64::from_le_bytes(h.finalize().as_bytes()[..8].try_into().unwrap())
}

/// Fold [`derive`] over a hierarchy path such as
/// `[("region", "north"), ("building", "b-3")]`.
pub fn derive_path(root: u64, path: &[(&str, &str)]) -> u64 {
    path.iter()
        .fold(root, |seed, (kind, key)| derive(seed, kind, key))
}

/// SplitMix64-style finalizer. Bijective in `z`, platform-stable.
fn mix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

/// Counter-based deterministic RNG (SplitMix64 over `seed` + counter).
///
/// `at(seed, i)` gives the i-th word directly, so parallel generation is
/// reproducible regardless of scheduling order.
#[derive(Clone, Debug)]
pub struct Rng {
    seed: u64,
    counter: u64,
}

impl Rng {
    /// The i-th word of the stream for `seed` without constructing an `Rng`.
    pub fn at(seed: u64, i: u64) -> u64 {
        mix64(seed.wrapping_add((i.wrapping_add(1)).wrapping_mul(0x9E3779B97F4A7C15)))
    }

    pub fn new(seed: u64) -> Self {
        Rng { seed, counter: 0 }
    }

    /// Rebind this stream to a different seed, restarting the counter.
    pub fn reseed(&mut self, seed: u64) {
        self.seed = seed;
        self.counter = 0;
    }

    pub fn next_u64(&mut self) -> u64 {
        let v = Self::at(self.seed, self.counter);
        self.counter += 1;
        v
    }

    /// Uniform float in [0, 1): 53 high bits scaled by 2^-53.
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / 9007199254740992.0)
    }

    /// Uniform integer in [0, n) via Lemire's widening multiply.
    ///
    /// There is a tiny modulo bias of at most n/2^64, which is negligible
    /// for generation and accepted in exchange for a rejection-free,
    /// fixed-cost function. `n == 0` yields 0.
    pub fn range_u64(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        ((self.next_u64() as u128 * n as u128) >> 64) as u64
    }

    /// Uniform float in [lo, hi).
    pub fn range_f64(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next_f64()
    }

    /// An independent stream for a sub-entity.
    pub fn fork(&self, kind: &str, key: &str) -> Rng {
        Rng::new(derive(self.seed, kind, key))
    }
}

/// Stable entity identifier derived from a seed path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntityId(pub u64);

impl EntityId {
    pub fn from_path(root: u64, path: &[(&str, &str)]) -> Self {
        EntityId(derive_path(root, path))
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

impl FromStr for EntityId {
    type Err = std::num::ParseIntError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        u64::from_str_radix(s.trim(), 16).map(EntityId)
    }
}

/// Lowercase hex encoding of raw bytes.
pub fn hex_bytes(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Serde helpers that serialize a `u64` seed as a 16-character lowercase
/// hex string, because JavaScript cannot hold u64 exactly.
#[cfg(feature = "serde")]
pub mod serde_hex {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &u64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("{v:016x}"))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
        let s = String::deserialize(d)?;
        u64::from_str_radix(s.trim(), 16)
            .map_err(|e| serde::de::Error::custom(format!("bad hex seed {s:?}: {e}")))
    }
}

/// Golden-test helper shared by all crates.
///
/// Compares a computed hex digest (or the BLAKE3 of raw bytes) against a
/// file at `<crate>/tests/golden/<name>.blake3`. Set `MB_BLESS=1` to write
/// or overwrite the file instead. A missing file without `MB_BLESS` fails
/// with instructions.
#[cfg(any(test, feature = "golden"))]
pub mod golden {
    use std::path::{Path, PathBuf};

    fn golden_path(manifest_dir: &str, name: &str) -> PathBuf {
        Path::new(manifest_dir)
            .join("tests")
            .join("golden")
            .join(format!("{name}.blake3"))
    }

    /// Compare `actual` hex against the golden file `name`.
    pub fn check_hex(manifest_dir: &str, name: &str, actual: &str) {
        let path = golden_path(manifest_dir, name);
        if std::env::var("MB_BLESS").as_deref() == Ok("1") {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, format!("{actual}\n")).unwrap();
            return;
        }
        match std::fs::read_to_string(&path) {
            Ok(expected) => assert_eq!(
                expected.trim(),
                actual.trim(),
                "golden mismatch for {name}: run with MB_BLESS=1 to update {}",
                path.display()
            ),
            Err(_) => panic!(
                "missing golden file {}; run with MB_BLESS=1 to create it",
                path.display()
            ),
        }
    }

    /// BLAKE3-hash `data` and compare the hex against the golden file.
    pub fn check_hash(manifest_dir: &str, name: &str, data: &[u8]) {
        let hex = crate::hex_bytes(&crate::hash_bytes(data));
        check_hex(manifest_dir, name, &hex);
    }
}
