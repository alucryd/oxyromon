// SPDX-License-Identifier: MIT OR Apache-2.0
//! The PlayStation 3 disc as Redump dumps it, shared by ps3-rs and ore-rs:
//! 2048-byte sectors, the regions sector 0 lists left plain and the ones
//! between them encrypted with the disc key, and an ISO 9660 file system
//! with Joliet names.
//!
//! Errors are `std::io`'s, `InvalidData` for a disc that is not one, so
//! that each host keeps its own error type.

pub mod disc;
