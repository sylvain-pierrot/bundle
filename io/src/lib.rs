//! Portable I/O traits for bundle.
//!
//! [`Read`] and [`Write`] abstract over `std::io` (with `std` feature)
//! and `embedded-io` (without `std`). All streaming code in the
//! bundle workspace is written against these traits.

#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;

mod error;
mod traits;

pub use error::Error;
pub use traits::{Read, Write};
