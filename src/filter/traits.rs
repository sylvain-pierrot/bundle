use crate::bundle::canonical::CanonicalBlock;
use crate::bundle::primary::PrimaryBlock;

use super::error::FilterRejection;

/// Read-only view of bundle metadata available before the payload.
pub struct BundleMetadata<'a> {
    pub primary: &'a PrimaryBlock,
    pub extensions: &'a [CanonicalBlock],
    pub payload_len: u64,
}

/// Read-only check on bundle metadata. Can accept or reject.
pub trait BundleFilter: Send + Sync {
    fn name(&self) -> &'static str;
    fn check(&self, meta: &BundleMetadata<'_>) -> Result<(), FilterRejection>;
}

/// Mutation applied to bundle metadata after all filters pass,
/// before the payload is streamed to retention (ingress) or wire (egress).
pub trait BundleMutator: Send + Sync {
    fn name(&self) -> &'static str;
    fn mutate(&self, primary: &mut PrimaryBlock, extensions: &mut Vec<CanonicalBlock>);
}
