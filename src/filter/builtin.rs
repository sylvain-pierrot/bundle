use crate::bundle::canonical::{BlockFlags, CanonicalBlock};
use crate::bundle::crc::Crc;
use crate::bundle::primary::PrimaryBlock;
use crate::eid::Eid;
use crate::extension::{HopCount, PreviousNode};

use super::error::FilterRejection;
use super::traits::{BundleFilter, BundleMetadata, BundleMutator};

/// Reject bundles whose hop count has been exceeded.
pub struct HopCountFilter;

impl BundleFilter for HopCountFilter {
    fn name(&self) -> &'static str {
        "hop_count"
    }

    fn check(&self, meta: &BundleMetadata<'_>) -> Result<(), FilterRejection> {
        for block in meta.extensions {
            if let Ok(hc) = block.parse_ext::<HopCount>()
                && hc.exceeded()
            {
                return Err(FilterRejection {
                    filter_name: self.name(),
                    reason: format!("count {} exceeds limit {}", hc.count, hc.limit),
                });
            }
        }
        Ok(())
    }
}

/// Reject bundles whose payload exceeds a size limit.
pub struct MaxPayloadSizeFilter {
    max_bytes: u64,
}

impl MaxPayloadSizeFilter {
    pub fn new(max_bytes: u64) -> Self {
        Self { max_bytes }
    }
}

impl BundleFilter for MaxPayloadSizeFilter {
    fn name(&self) -> &'static str {
        "max_payload_size"
    }

    fn check(&self, meta: &BundleMetadata<'_>) -> Result<(), FilterRejection> {
        if meta.payload_len > self.max_bytes {
            return Err(FilterRejection {
                filter_name: self.name(),
                reason: format!(
                    "payload {} bytes exceeds limit {} bytes",
                    meta.payload_len, self.max_bytes
                ),
            });
        }
        Ok(())
    }
}

/// Reject bundles not destined for any of the allowed EIDs.
pub struct DestinationFilter {
    allowed: Vec<Eid>,
}

impl DestinationFilter {
    pub fn new(allowed: Vec<Eid>) -> Self {
        Self { allowed }
    }
}

impl BundleFilter for DestinationFilter {
    fn name(&self) -> &'static str {
        "destination"
    }

    fn check(&self, meta: &BundleMetadata<'_>) -> Result<(), FilterRejection> {
        if !self.allowed.contains(&meta.primary.dest_eid) {
            return Err(FilterRejection {
                filter_name: self.name(),
                reason: format!("destination {:?} not allowed", meta.primary.dest_eid),
            });
        }
        Ok(())
    }
}

/// Increment the hop count by 1. Adds a hop count block if none exists.
pub struct HopCountIncrementMutator {
    pub default_limit: u8,
}

impl HopCountIncrementMutator {
    pub fn new(default_limit: u8) -> Self {
        Self { default_limit }
    }
}

impl BundleMutator for HopCountIncrementMutator {
    fn name(&self) -> &'static str {
        "hop_count_increment"
    }

    fn mutate(&self, _primary: &mut PrimaryBlock, extensions: &mut Vec<CanonicalBlock>) {
        for block in extensions.iter_mut() {
            if let Ok(mut hc) = block.parse_ext::<HopCount>() {
                hc.count = hc.count.saturating_add(1);
                *block = CanonicalBlock::from_ext(block.block_number, block.flags, block.crc, &hc);
                return;
            }
        }
        // No hop count block — add one
        let next_num = extensions.iter().map(|b| b.block_number).max().unwrap_or(1) + 1;
        let hc = HopCount {
            limit: self.default_limit,
            count: 1,
        };
        extensions.push(CanonicalBlock::from_ext(
            next_num,
            BlockFlags::from_bits(0),
            Crc::None,
            &hc,
        ));
    }
}

/// Set or update the Previous Node extension block.
pub struct PreviousNodeMutator {
    node_id: Eid,
}

impl PreviousNodeMutator {
    pub fn new(node_id: Eid) -> Self {
        Self { node_id }
    }
}

impl BundleMutator for PreviousNodeMutator {
    fn name(&self) -> &'static str {
        "previous_node"
    }

    fn mutate(&self, _primary: &mut PrimaryBlock, extensions: &mut Vec<CanonicalBlock>) {
        let pn = PreviousNode {
            node_id: self.node_id.clone(),
        };
        for block in extensions.iter_mut() {
            if block.parse_ext::<PreviousNode>().is_ok() {
                *block = CanonicalBlock::from_ext(block.block_number, block.flags, block.crc, &pn);
                return;
            }
        }
        // No previous node block — add one
        let next_num = extensions.iter().map(|b| b.block_number).max().unwrap_or(1) + 1;
        extensions.push(CanonicalBlock::from_ext(
            next_num,
            BlockFlags::from_bits(0),
            Crc::None,
            &pn,
        ));
    }
}
