//! Incremental cache scaffolding for Strato.

pub mod invalidation;
pub mod manifest;
pub mod storage;

pub use invalidation::CacheInvalidation;
pub use manifest::{
    CacheArtifact, CacheArtifactKind, CacheManifest, CachedFileKind, CachedFileResult,
    CallSiteSyntax, ClassSyntax, DecoratorSyntax, FileSyntax, FunctionSyntax, ImportSyntax,
    StorageKey, SyntaxLocation, sha256_hex,
};
pub use storage::{CacheStorage, StorageError};

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
