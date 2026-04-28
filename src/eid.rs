use std::borrow::Cow;

/// Endpoint identifier (RFC 9171 §4.2.5).
///
/// Uses `Cow<str>` for the DTN scheme: zero-copy when borrowed from an input
/// buffer, owned when constructed or parsed from a stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Eid<'a> {
    Null,
    Dtn(Cow<'a, str>),
    Ipn { node: u64, service: u64 },
}

impl Eid<'_> {
    #[inline]
    pub fn is_null(&self) -> bool {
        matches!(self, Eid::Null)
    }

    /// Convert to an owned `Eid<'static>` by cloning any borrowed data.
    pub fn into_owned(self) -> Eid<'static> {
        match self {
            Eid::Null => Eid::Null,
            Eid::Dtn(s) => Eid::Dtn(Cow::Owned(s.into_owned())),
            Eid::Ipn { node, service } => Eid::Ipn { node, service },
        }
    }
}
