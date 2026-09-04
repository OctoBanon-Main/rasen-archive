//! C ABI implementation. Pointer ownership and lifetime contracts are defined in
//! `include/rasen_archive.h`, the public interface consumed by FFI callers.
#![allow(clippy::missing_safety_doc)]

mod archive;
mod error;
mod io;
mod pack;
mod types;

pub use archive::*;
pub use error::*;
pub use io::rasen_buffer_free;
pub use pack::*;
pub use types::*;

#[cfg(test)]
mod tests;
