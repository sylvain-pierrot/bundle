//! Bundle filtering and mutation pipeline.
//!
//! Policies live on the reader (ingress) or writer (egress).
//! Configure once, every bundle follows the same rules.
//! Rejected bundles waste zero I/O.
//!
//! Traits and built-in filters are in [`bundle_bpv7::filter`].
//! This module provides [`FilterChain`] which wires them into
//! the streaming I/O pipeline.

use alloc::boxed::Box;
use alloc::vec::Vec;

use bundle_bpv7::{CanonicalBlock, PrimaryBlock};

pub use bundle_bpv7::filter::builtin;
pub use bundle_bpv7::filter::{BundleFilter, BundleMetadata, BundleMutator, FilterRejection};

/// An ordered collection of filters and mutators.
///
/// Filters run first (in order). If any rejects, processing stops.
/// Then mutators run in order on accepted bundles.
pub struct FilterChain {
    filters: Vec<Box<dyn BundleFilter>>,
    mutators: Vec<Box<dyn BundleMutator>>,
}

impl FilterChain {
    pub fn new() -> Self {
        Self {
            filters: Vec::new(),
            mutators: Vec::new(),
        }
    }

    pub fn add_filter(&mut self, f: impl BundleFilter + 'static) {
        self.filters.push(Box::new(f));
    }

    pub fn add_mutator(&mut self, m: impl BundleMutator + 'static) {
        self.mutators.push(Box::new(m));
    }

    pub fn is_empty(&self) -> bool {
        self.filters.is_empty() && self.mutators.is_empty()
    }

    pub(crate) fn run_filters(&self, meta: &BundleMetadata<'_>) -> Result<(), FilterRejection> {
        for f in &self.filters {
            f.check(meta)?;
        }
        Ok(())
    }

    pub(crate) fn run_mutators(
        &self,
        primary: &mut PrimaryBlock,
        extensions: &mut Vec<CanonicalBlock>,
    ) {
        for m in &self.mutators {
            m.mutate(primary, extensions);
        }
    }
}

impl Default for FilterChain {
    fn default() -> Self {
        Self::new()
    }
}
