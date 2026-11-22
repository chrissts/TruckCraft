#![forbid(unsafe_code)]

pub mod archive;
pub mod cityhash;
pub mod gdeflate;
pub mod hashfs1;
pub mod hashfs2;
pub mod sii;
pub mod tobj;
pub mod zip;
pub use archive::Archive;

/// Limits apply before allocating or inflating game archive input.
pub const MAX_ARCHIVE_ENTRY: usize = 256 * 1024 * 1024;
pub const MAX_ARCHIVE_INDEX: usize = 64 * 1024 * 1024;
pub const MAX_ARCHIVE_ENTRIES: usize = 1_000_000;
