//! CIA (CTR Importable Archive) reading and unpacking, part of oxyROMon,
//! behaving for its parts the way [ctrtool] does in `--plain` mode, which
//! this began as a port of.
//!
//! A CIA is a Nintendo 3DS installable container: a header, a certificate
//! chain, a ticket, a title metadata record, the title's contents packed
//! together, and an optional footer. This crate locates every part and
//! copies them out as they are — an encrypted content stays encrypted —
//! which is what makes an extracted content hash match what a DAT recorded.
//!
//! ```no_run
//! use std::path::Path;
//!
//! let cia = cia_rs::Cia::parse(Path::new("game.cia"), &mut |_| {}).unwrap();
//! println!(
//!     "title {:016x} v{}, {} contents",
//!     cia.title_id(),
//!     cia.title_version(),
//!     cia.contents().len(),
//! );
//!
//! let written = cia_rs::unpack(
//!     Path::new("game.cia"),
//!     Path::new("out"),
//!     &cia_rs::UnpackOptions::default(),
//!     &mut |_| {},
//! )
//! .unwrap();
//! for file in &written {
//!     println!("{} from {:?}", file.path.display(), file.part);
//! }
//! ```
//!
//! [ctrtool]: https://github.com/3DSGuy/Project_CTR

mod cia;
mod error;
mod tmd;
mod unpack;

pub use cia::{Cia, CiaFormat, Content, Section};
pub use error::{Error, Result};
pub use unpack::{Extraction, Part, UnpackOptions, unpack};
