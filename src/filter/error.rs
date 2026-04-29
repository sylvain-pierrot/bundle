use std::fmt;

/// Reason a filter rejected a bundle.
#[derive(Debug, Clone)]
pub struct FilterRejection {
    pub filter_name: &'static str,
    pub reason: String,
}

impl fmt::Display for FilterRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.filter_name, self.reason)
    }
}

impl std::error::Error for FilterRejection {}
