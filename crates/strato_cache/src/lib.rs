//! Incremental cache scaffolding for Strato.

use sha2::{Digest, Sha256};

/// Returns the lowercase SHA-256 digest for `bytes`.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::sha256_hex;

    #[test]
    fn hashes_bytes_deterministically() {
        assert_eq!(
            sha256_hex(b"strato"),
            "79fbe4ba398c29cb7ceff4dbf63c3658e7589386d4be8e587686fbd738d038ba"
        );
    }
}
