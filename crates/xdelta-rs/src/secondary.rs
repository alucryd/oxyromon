//! xdelta3's secondary compression of a window's sections.
//!
//! Only LZMA, xdelta3's default since 3.0. Its sections are not independent:
//! each section kind is one xz stream that xdelta3 flushes at the end of every
//! section and continues into the next window's, so the decoders persist too.

use crate::error::{Error, Result};
use crate::vcdiff::{VCD_ADDRCOMP, VCD_DATACOMP, VCD_INSTCOMP, Window, corrupt, read_integer};
use lzma_rust2::{Action, XzStream};

const LZMA_ID: u8 = 2;

pub struct Secondary {
    id: u8,
    /// Data, instructions and addresses, created as each is first needed.
    streams: [Option<XzStream>; 3],
}

impl Secondary {
    pub fn new(id: u8) -> Self {
        Secondary {
            id,
            streams: [None, None, None],
        }
    }

    /// Decompress whichever of `window`'s sections its delta indicator flags.
    pub fn decompress(&mut self, window: &mut Window) -> Result<()> {
        // The decoded sizes a target of this size could legitimately need.
        let target = window.target_len as u64;
        let limits = [target, 4 * target + 64, 4 * target + 64];
        let sections = [&mut window.data, &mut window.inst, &mut window.addr];
        for (kind, (flag, section)) in [VCD_DATACOMP, VCD_INSTCOMP, VCD_ADDRCOMP]
            .into_iter()
            .zip(sections)
            .enumerate()
        {
            if window.delta_indicator & flag != 0 {
                *section = self.decompress_section(kind, section, limits[kind])?;
            }
        }
        Ok(())
    }

    fn decompress_section(&mut self, kind: usize, section: &[u8], limit: u64) -> Result<Vec<u8>> {
        if self.id != LZMA_ID {
            return Err(Error::Unsupported(match self.id {
                1 => "DJW secondary compression".into(),
                16 => "FGK secondary compression".into(),
                id => format!("secondary compressor {id}"),
            }));
        }
        let mut input = section;
        let size = read_integer(&mut input)?;
        if size == 0 || size > limit {
            return Err(corrupt("a compressed section of an impossible size"));
        }
        let stream = self.streams[kind].get_or_insert_with(|| XzStream::new(false));
        let mut output = vec![0; size as usize];
        let mut produced = 0;
        while produced < output.len() {
            let result = stream
                .process(input, &mut output[produced..], Action::Run)
                .map_err(|error| corrupt(format!("lzma: {error}")))?;
            input = &input[result.bytes_consumed..];
            produced += result.bytes_produced;
            if result.bytes_consumed == 0 && result.bytes_produced == 0 {
                return Err(corrupt("a compressed section is short"));
            }
        }
        if !input.is_empty() {
            return Err(corrupt("a compressed section is longer than it decodes to"));
        }
        Ok(output)
    }
}
