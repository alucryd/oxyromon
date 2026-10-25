//! Wii U disc images, part of oxyROMon.
//!
//! A WUD is a raw dump of a Wii U disc. A WUX stores the same bytes losslessly,
//! each distinct 32 KiB sector once, as [wudd] writes it and [Cemu] reads it.
//! A WUA is Cemu's archive of a title's decrypted files: converting to it
//! drops the disc's encryption and layout, so there is no way back.
//!
//! ```no_run
//! use std::path::Path;
//!
//! let wud = Path::new("game.wud");
//! let wux = Path::new("game.wux");
//! wux_rs::compress(wud, wux, &mut |_| {}).unwrap();
//! wux_rs::decompress(wux, Path::new("out.wud"), &mut |_| {}).unwrap();
//!
//! // Cemu's keys.txt must hold the disc key and the Wii U common key.
//! let keys = wux_rs::load_keys(&wux_rs::cemu_keys_path().unwrap()).unwrap();
//! // A game and its update, from a NUS folder, make one WUA.
//! let update = Path::new("update/");
//! wux_rs::to_wua(&[wux, update], Path::new("game.wua"), &keys, &mut |_| {}).unwrap();
//! ```
//!
//! [wudd]: https://github.com/wiiu-env/wudd
//! [Cemu]: https://github.com/cemu-project/Cemu

mod crypto;
mod disc;
mod error;
mod image;
mod install;
mod io;
mod keys;
mod nus;
mod revert;
mod wua;
mod wux;
mod zarchive;

pub use crypto::Key;
pub use error::{Error, Result};
pub use image::Image;
pub use install::to_nus;
pub use keys::{cemu_keys_path, load_keys, parse_keys};
pub use nus::{ticket_path, tmd_path};
pub use revert::wua_to_nus;
pub use wua::{title_id, to_wua};
pub use wux::{HEADER_SIZE, Header, MAGIC, SECTOR_SIZE, compress, decompress};

/// Sizes of the two files a run touched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    pub input_size: u64,
    pub output_size: u64,
}

impl Stats {
    /// Output size as a percentage of the input's.
    pub fn ratio_percent(&self) -> f64 {
        if self.input_size == 0 {
            0.0
        } else {
            self.output_size as f64 * 100.0 / self.input_size as f64
        }
    }
}
