//! LaserDisc CHDs: created from an AVI and extracted back to one, the way
//! chdman 0.289's `createld` and `extractld` do.
//!
//! A hunk holds one field, or one frame of progressive video, with its
//! audio, in the raw form of the `avhu` codec. Interlaced video — at most
//! 30 frames a second, even height above 288 lines — is split in fields,
//! so the CHD runs at twice the AVI's frame rate and half its height.

use std::path::Path;

use crate::avhuff;
use crate::avi::AviReader;
use crate::codec::CODEC_NONE;
use crate::container::{Chd, MDFLAGS_CHECKSUM, MTAG_LD_DISC, MTAG_LD_VIDEO};
use crate::error::{Error, Result};
use crate::vbi::{self, VBI_PACKED_BYTES};
use crate::writer::{Source, write_part};

/// The most audio channels chdman assembles into a frame.
const MAX_CHANNELS: u32 = 8;

/// What a LaserDisc CHD's `AVAV` metadata says, and what follows from it.
#[derive(Clone, Copy, Debug)]
struct AvInfo {
    fps_times_1million: u32,
    width: u32,
    height: u32,
    interlaced: bool,
    channels: u32,
    rate: u32,
    max_samples_per_frame: u32,
    bytes_per_frame: u32,
}

impl AvInfo {
    fn new(
        fps_times_1million: u32,
        width: u32,
        height: u32,
        interlaced: bool,
        channels: u32,
        rate: u32,
    ) -> Self {
        let max_samples_per_frame =
            (u64::from(rate) * 1_000_000).div_ceil(u64::from(fps_times_1million)) as u32;
        Self {
            fps_times_1million,
            width,
            height,
            interlaced,
            channels,
            rate,
            max_samples_per_frame,
            bytes_per_frame: avhuff::raw_size(width, height, channels, max_samples_per_frame),
        }
    }

    fn interlace_factor(&self) -> u32 {
        if self.interlaced { 2 } else { 1 }
    }

    /// The `AVAV` metadata text.
    fn metadata(&self) -> String {
        format!(
            "FPS:{}.{:06} WIDTH:{} HEIGHT:{} INTERLACED:{} CHANNELS:{} SAMPLERATE:{}",
            self.fps_times_1million / 1_000_000,
            self.fps_times_1million % 1_000_000,
            self.width,
            self.height,
            u32::from(self.interlaced),
            self.channels,
            self.rate
        )
    }

    /// The first sample of field `frame` and how many it has.
    fn samples(&self, frame: u32) -> (u32, u32) {
        let at = |frame: u64| {
            (u64::from(self.rate) * frame * 1_000_000).div_ceil(u64::from(self.fps_times_1million))
                as u32
        };
        let first = at(u64::from(frame));
        (first, at(u64::from(frame) + 1).wrapping_sub(first))
    }

    /// Whether the fields are an NTSC or PAL LaserDisc's, whose VBI codes
    /// chdman records.
    fn has_vbi(&self) -> bool {
        self.height == 524 / 2 || self.height == 624 / 2
    }
}

/// The frames of an AVI, assembled the way `chd_avi_compressor::read_data`
/// does.
struct LdSource {
    avi: AviReader,
    info: AvInfo,
    frame_count: u32,
    /// The whole video frame, kept from one read to the next as chdman's
    /// bitmap is.
    bitmap: Vec<u16>,
    audio: Vec<Vec<i16>>,
    ldframedata: Vec<u8>,
    raw: Vec<u8>,
    input_size: u64,
    logical_size: u64,
    reported: u64,
}

impl LdSource {
    /// Assembles field `framenum` into `self.raw`, padded to a frame.
    fn assemble(&mut self, framenum: u32) -> Result<()> {
        let info = self.info;
        let factor = info.interlace_factor();
        let (first_sample, samples) = info.samples(framenum);
        let channels = info.channels.min(MAX_CHANNELS);
        for channel in 0..channels as usize {
            // like chdman's, the buffer keeps whatever a short read leaves
            self.audio[channel].resize(samples as usize, 0);
            let (avi, audio) = (&mut self.avi, &mut self.audio[channel]);
            avi.read_sound_samples(channel as u32, first_sample, audio).map_err(|error| {
                Error::Corrupt(format!(
                    "cannot read audio samples {first_sample}-{samples} of channel {channel}: {error}"
                ))
            })?;
        }
        let width = info.width as usize;
        self.avi
            .read_video_frame(framenum / factor, &mut self.bitmap, width)
            .map_err(|error| {
                Error::Corrupt(format!(
                    "cannot read AVI frame {}: {error}",
                    framenum / factor
                ))
            })?;
        let field = (framenum % factor) as usize * width;
        let stride = width * factor as usize;
        let rows = (self.bitmap.len() / width) / factor as usize;

        if info.has_vbi() {
            let packed = vbi::parse_and_pack(&self.bitmap, field, stride, width, framenum);
            self.ldframedata[framenum as usize * VBI_PACKED_BYTES..][..VBI_PACKED_BYTES]
                .copy_from_slice(&packed);
        }

        // avhuff_encoder::assemble_data
        if samples > 65535 || width > 65535 || rows > 65535 {
            return Err(Error::Corrupt(format!("frame {framenum} is too large")));
        }
        self.raw.clear();
        self.raw.extend_from_slice(b"chav");
        self.raw.push(0);
        self.raw.push(channels as u8);
        self.raw.extend_from_slice(&(samples as u16).to_be_bytes());
        self.raw.extend_from_slice(&(width as u16).to_be_bytes());
        self.raw.extend_from_slice(&(rows as u16).to_be_bytes());
        for audio in &self.audio[..channels as usize] {
            for sample in audio {
                self.raw.extend_from_slice(&sample.to_be_bytes());
            }
        }
        for row in 0..rows {
            let start = field + row * stride;
            for pixel in &self.bitmap[start..start + width] {
                self.raw.extend_from_slice(&pixel.to_be_bytes());
            }
        }
        if self.raw.len() < info.bytes_per_frame as usize {
            self.raw.resize(info.bytes_per_frame as usize, 0);
        }
        Ok(())
    }
}

impl Source for LdSource {
    fn read(&mut self, mut offset: u64, dest: &mut [u8]) -> Result<u64> {
        let bytes_per_frame = u64::from(self.info.bytes_per_frame);
        let length = dest.len() as u64;
        let end = offset + length;
        let mut remaining = length;
        let mut d = 0usize;
        let start_frame = offset / bytes_per_frame;
        let end_frame = (offset + length - 1) / bytes_per_frame;
        for framenum in start_frame..=end_frame {
            if framenum >= u64::from(self.frame_count) {
                continue;
            }
            self.assemble(framenum as u32)?;
            let start_offset = framenum * bytes_per_frame;
            let copy = remaining.min(start_offset + bytes_per_frame - offset) as usize;
            let from = (offset - start_offset) as usize;
            dest[d..d + copy].copy_from_slice(&self.raw[from..from + copy]);
            offset += copy as u64;
            d += copy;
            remaining -= copy as u64;
        }
        let target = (u128::from(self.input_size) * u128::from(end.min(self.logical_size))
            / u128::from(self.logical_size.max(1))) as u64;
        let delta = target.saturating_sub(self.reported);
        self.reported += delta;
        Ok(delta)
    }

    fn late_metadata(&self) -> Vec<(u32, u8, Vec<u8>)> {
        if self.info.has_vbi() {
            vec![(MTAG_LD_DISC, 0, self.ldframedata.clone())]
        } else {
            Vec::new()
        }
    }
}

/// Creates a LaserDisc CHD from an AVI of YUY2, UYVY or VYUY video and
/// 8- or 16-bit PCM audio, the way `chdman createld` does.
///
/// Each hunk is a field (or a progressive frame), `hunk_bytes` a multiple
/// of its size; `None` takes one field a hunk, chdman's default. The
/// `avhu` codec is the only one that applies, and an uncompressed CHD is
/// refused, as chdman refuses it. NTSC and PAL LaserDisc captures (524 and
/// 624 lines) also get the `AVLD` VBI codes of every field. Progress adds
/// up to the AVI's size.
pub fn create_ld(
    input: &Path,
    output: &Path,
    hunk_bytes: Option<u32>,
    compression: [u32; 4],
    parent: Option<&mut Chd>,
    progress: &mut dyn FnMut(u64),
) -> Result<()> {
    let avi = AviReader::open(input)?;
    let movie = avi.info;
    if movie.video_sampletime == 0 {
        return Err(Error::Corrupt("AVI has no frame rate".to_owned()));
    }
    let fps =
        (u64::from(movie.video_timescale) * 1_000_000 / u64::from(movie.video_sampletime)) as u32;
    let interlaced =
        fps / 1_000_000 <= 30 && movie.video_height.is_multiple_of(2) && movie.video_height > 288;
    let (fps, height, frames) = if interlaced {
        (fps * 2, movie.video_height / 2, movie.video_numsamples * 2)
    } else {
        (fps, movie.video_height, movie.video_numsamples)
    };
    if fps == 0 {
        return Err(Error::Corrupt("AVI has no frame rate".to_owned()));
    }
    let info = AvInfo::new(
        fps,
        movie.video_width,
        height,
        interlaced,
        movie.audio_channels,
        movie.audio_samplerate,
    );
    let hunk_bytes = hunk_bytes
        .or(parent.as_ref().map(|parent| parent.info().hunk_size))
        .unwrap_or(info.bytes_per_frame);
    if !hunk_bytes.is_multiple_of(info.bytes_per_frame) {
        return Err(Error::InvalidOption(format!(
            "hunk size {hunk_bytes} is not a multiple of the frame size {}",
            info.bytes_per_frame
        )));
    }
    if compression[0] == CODEC_NONE {
        return Err(Error::InvalidOption(
            "an uncompressed LaserDisc CHD is not supported".to_owned(),
        ));
    }
    let logical_size = u64::from(frames) * u64::from(hunk_bytes);
    let mut metadata = info.metadata().into_bytes();
    metadata.push(0);
    let metadata = [(MTAG_LD_VIDEO, MDFLAGS_CHECKSUM, metadata)];
    let input_size = avi.size();
    let bitmap = vec![0u16; info.width as usize * (height * info.interlace_factor()) as usize];
    let mut source = LdSource {
        avi,
        info,
        frame_count: frames,
        bitmap,
        audio: vec![Vec::new(); info.channels.min(MAX_CHANNELS) as usize],
        ldframedata: vec![0; frames as usize * VBI_PACKED_BYTES],
        raw: Vec::new(),
        input_size,
        logical_size,
        reported: 0,
    };
    write_part(
        &mut source,
        logical_size,
        output,
        info.bytes_per_frame,
        hunk_bytes,
        compression,
        parent,
        &metadata,
        progress,
    )
}

/// Reads `AVAV` metadata, `sscanf`'s `FPS:%d.%06d WIDTH:%d HEIGHT:%d
/// INTERLACED:%d CHANNELS:%d SAMPLERATE:%d`, the fraction six digits at
/// most.
fn parse_av_metadata(data: &[u8]) -> Option<AvInfo> {
    let text = std::str::from_utf8(data).ok()?.split('\0').next()?;
    fn number<'a>(text: &'a str, prefix: &str, max_digits: usize) -> Option<(i64, &'a str)> {
        let rest = text.strip_prefix(prefix)?.trim_start();
        let (negative, rest) = match rest.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, rest.strip_prefix('+').unwrap_or(rest)),
        };
        let digits = rest
            .bytes()
            .take(max_digits)
            .take_while(u8::is_ascii_digit)
            .count();
        if digits == 0 {
            return None;
        }
        let value: i64 = rest[..digits].parse().ok()?;
        Some((if negative { -value } else { value }, &rest[digits..]))
    }
    let (fps, rest) = number(text, "FPS:", usize::MAX)?;
    let (fpsfrac, rest) = number(rest, ".", 6)?;
    let (width, rest) = number(rest.trim_start(), "WIDTH:", usize::MAX)?;
    let (height, rest) = number(rest.trim_start(), "HEIGHT:", usize::MAX)?;
    let (interlaced, rest) = number(rest.trim_start(), "INTERLACED:", usize::MAX)?;
    let (channels, rest) = number(rest.trim_start(), "CHANNELS:", usize::MAX)?;
    let (rate, _) = number(rest.trim_start(), "SAMPLERATE:", usize::MAX)?;
    let fps_times_1million = (fps * 1_000_000 + fpsfrac) as u32;
    if fps_times_1million == 0 {
        return None;
    }
    Some(AvInfo::new(
        fps_times_1million,
        width as u32,
        height as u32,
        interlaced != 0,
        channels as u32,
        rate as u32,
    ))
}

/// Extracts a LaserDisc CHD to an AVI of YUY2 video and 16-bit PCM audio,
/// the way `chdman extractld` does: fields are woven back into frames, and
/// the sound is interleaved ahead of the video.
///
/// Progress is reported as CHD logical bytes, adding up to its logical
/// size. The AVI lands on `<output>.part` and is renamed into place once
/// complete; a failed run leaves nothing behind.
pub fn extract_ld(chd: &mut Chd, output: &Path, progress: &mut dyn FnMut(u64)) -> Result<()> {
    let part = crate::writer::part_path(output);
    let result = extract_ld_inner(chd, &part, progress);
    match result {
        Ok(()) => Ok(std::fs::rename(&part, output)?),
        Err(error) => {
            let _ = std::fs::remove_file(&part);
            Err(error)
        }
    }
}

fn extract_ld_inner(chd: &mut Chd, part: &Path, progress: &mut dyn FnMut(u64)) -> Result<()> {
    let metadata = chd
        .metadata()
        .iter()
        .find(|entry| entry.tag == MTAG_LD_VIDEO && entry.index == 0)
        .ok_or_else(|| Error::Corrupt("no A/V metadata in the CHD".to_owned()))?;
    let info = parse_av_metadata(&metadata.data)
        .ok_or_else(|| Error::Corrupt("improperly formatted A/V metadata".to_owned()))?;
    let chd_info = chd.info();
    if info.bytes_per_frame != chd_info.hunk_size {
        return Err(Error::Corrupt(
            "frame size does not match hunk size for this CHD".to_owned(),
        ));
    }
    let factor = info.interlace_factor();
    let end = (chd_info.hunk_count / u64::from(factor)) as u32 * factor;
    let width = info.width as usize;
    let mut avi = crate::avi::AviWriter::create(
        part,
        crate::avi::CreateInfo {
            video_timescale: info.fps_times_1million / factor,
            video_sampletime: 1_000_000,
            video_width: info.width,
            video_height: info.height * factor,
            audio_channels: info.channels,
            audio_samplerate: info.rate,
        },
    )?;
    let mut fullbitmap = vec![0u16; width * (info.height * factor) as usize];
    let mut audio = vec![vec![0i16; info.max_samples_per_frame.max(1) as usize]; 16];
    let mut hunk = vec![0u8; chd_info.hunk_size as usize];
    let mut reported = 0u64;
    for framenum in 0..end {
        chd.read_hunk(framenum, &mut hunk)
            .map_err(|error| Error::Corrupt(format!("cannot read hunk {framenum}: {error}")))?;
        if raw_frame_size(&hunk).is_none() {
            return Err(Error::Corrupt(format!(
                "hunk {framenum} is not an A/V frame"
            )));
        }
        let channels = usize::from(hunk[5]);
        let samples = usize::from(u16::from_be_bytes([hunk[6], hunk[7]]));
        let frame_width = usize::from(u16::from_be_bytes([hunk[8], hunk[9]]));
        let frame_height = usize::from(u16::from_be_bytes([hunk[10], hunk[11]]));
        if frame_width > width || frame_height > info.height as usize {
            return Err(Error::Corrupt(format!(
                "hunk {framenum} holds too large a frame"
            )));
        }
        if channels > audio.len() || (channels > 0 && samples > audio[0].len()) {
            return Err(Error::Corrupt(format!(
                "hunk {framenum} holds too many samples"
            )));
        }
        let mut offset = 12 + usize::from(hunk[4]);
        for channel in audio.iter_mut().take(channels) {
            for (sample, raw) in channel
                .iter_mut()
                .zip(hunk[offset..offset + samples * 2].as_chunks::<2>().0)
            {
                *sample = i16::from_be_bytes(*raw);
            }
            offset += samples * 2;
        }
        // the field's rows, every other row of the frame when interlaced
        let field = (framenum % factor) as usize;
        for row in 0..frame_height {
            let start = (row * factor as usize + field) * width;
            for (pixel, raw) in fullbitmap[start..start + frame_width]
                .iter_mut()
                .zip(hunk[offset..offset + frame_width * 2].as_chunks::<2>().0)
            {
                *pixel = u16::from_be_bytes(*raw);
            }
            offset += frame_width * 2;
        }
        for (channel, samples_of) in audio.iter().enumerate().take(info.channels as usize) {
            avi.append_sound_samples(channel, &samples_of[..samples])?;
        }
        if (framenum + 1).is_multiple_of(factor) {
            avi.append_video_frame(&fullbitmap, width)?;
        }
        let step = u64::from(chd_info.hunk_size);
        reported += step;
        progress(step);
    }
    avi.finish()?;
    progress(chd_info.logical_size.saturating_sub(reported));
    Ok(())
}

/// The size of the raw frame a hunk holds, if it holds one.
fn raw_frame_size(hunk: &[u8]) -> Option<usize> {
    let size = avhuff::raw_data_size(hunk);
    (size != 0 && size <= hunk.len()).then_some(size)
}
