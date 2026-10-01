//! FLAC decoding, as used by the `flac` and `cdfl` CHD codecs.
//!
//! Unlike a `.flac` file, a CHD chunk carries no FLAC container: no `fLaC`
//! magic and no STREAMINFO block. Like MAME, we synthesize the fixed header
//! the compressors wrote their stream against, feed header and payload to
//! libFLAC as one stream, and interleave the decoded frames ourselves.

use std::os::raw::c_void;

use libflac_sys::FLAC__Frame;
use libflac_sys::FLAC__StreamDecoder;
use libflac_sys::FLAC__StreamDecoderErrorStatus;
use libflac_sys::FLAC__StreamEncoder;
use libflac_sys::FLAC__StreamEncoderWriteStatus;
use libflac_sys::FLAC__byte;
use libflac_sys::FLAC__int32;
use libflac_sys::FLAC__uint64;

use crate::error::Error;
use crate::error::Result;

/// The size of the header we synthesize, in bytes. libFLAC counts the bytes
/// of the stream it consumed against it, so callers subtract it from the
/// decode position to learn where the FLAC stream in a chunk ends.
const HEADER_SIZE: usize = 42;

/// The FLAC stream sample rate both CHD compressors encode at.
const SAMPLE_RATE: u64 = 44_100;

/// Builds the 42-byte header (magic and STREAMINFO block) MAME's FLAC
/// decoder synthesizes for a headerless CHD chunk.
fn custom_header(block_size: u16) -> [u8; HEADER_SIZE] {
    let mut header = [0u8; HEADER_SIZE];
    header[..4].copy_from_slice(b"fLaC");
    // The one and only metadata block: the last one, of type STREAMINFO, 34
    // bytes long.
    header[4] = 0x80;
    header[5..8].copy_from_slice(&[0x00, 0x00, 0x22]);
    // The minimum and maximum block sizes.
    header[8..10].copy_from_slice(&block_size.to_be_bytes());
    header[10..12].copy_from_slice(&block_size.to_be_bytes());
    // The minimum and maximum frame sizes, at 12 and 15, are left at 0.
    // The sample rate, the channel count minus one, the sample width minus
    // one and the total sample count, packed big-endian over 8 bytes.
    let info = (SAMPLE_RATE << 44) | ((2 - 1) << 41) | ((16 - 1) << 36);
    header[18..26].copy_from_slice(&info.to_be_bytes());
    // The md5 sum of the decoded stream, at 26, is left at 0.
    header
}

/// The client data libFLAC hands back to the callbacks: the header and the
/// payload, read as one stream, and the output buffer the frames are
/// interleaved into.
struct Client<'a> {
    header: &'a [u8],
    payload: &'a [u8],
    /// Bytes of header and payload fed to the decoder so far.
    position: usize,
    out: *mut u8,
    /// Samples per channel expected, `out.len()` over 4.
    target: usize,
    /// Samples per channel written so far.
    written: usize,
    /// Whether to write the 16-bit samples big-endian.
    big_endian: bool,
    /// Set by the callbacks to abort the decode.
    failed: bool,
}

unsafe extern "C" fn read_callback(
    _decoder: *const FLAC__StreamDecoder,
    buffer: *mut FLAC__byte,
    bytes: *mut usize,
    client_data: *mut c_void,
) -> libflac_sys::FLAC__StreamDecoderReadStatus {
    let client = unsafe { &mut *client_data.cast::<Client>() };
    let stream_size = client.header.len() + client.payload.len();
    if client.position >= stream_size {
        unsafe { *bytes = 0 };
        return libflac_sys::FLAC__STREAM_DECODER_READ_STATUS_END_OF_STREAM;
    }
    let provided = unsafe { *bytes }.min(stream_size - client.position);
    let mut buffer = buffer;
    let mut remaining = provided;
    if client.position < client.header.len() {
        let len = remaining.min(client.header.len() - client.position);
        unsafe {
            std::ptr::copy_nonoverlapping(client.header.as_ptr().add(client.position), buffer, len)
        };
        client.position += len;
        remaining -= len;
        buffer = unsafe { buffer.add(len) };
    }
    if remaining > 0 {
        let offset = client.position - client.header.len();
        unsafe {
            std::ptr::copy_nonoverlapping(client.payload.as_ptr().add(offset), buffer, remaining)
        };
        client.position += remaining;
    }
    unsafe { *bytes = provided };
    libflac_sys::FLAC__STREAM_DECODER_READ_STATUS_CONTINUE
}

unsafe extern "C" fn tell_callback(
    _decoder: *const FLAC__StreamDecoder,
    absolute_byte_offset: *mut FLAC__uint64,
    client_data: *mut c_void,
) -> libflac_sys::FLAC__StreamDecoderTellStatus {
    let client = unsafe { &*client_data.cast::<Client>() };
    unsafe { *absolute_byte_offset = client.position as FLAC__uint64 };
    libflac_sys::FLAC__STREAM_DECODER_TELL_STATUS_OK
}

unsafe extern "C" fn write_callback(
    _decoder: *const FLAC__StreamDecoder,
    frame: *const FLAC__Frame,
    buffer: *const *const FLAC__int32,
    client_data: *mut c_void,
) -> libflac_sys::FLAC__StreamDecoderWriteStatus {
    let client = unsafe { &mut *client_data.cast::<Client>() };
    // CHD FLAC streams are 44.1 kHz stereo and the header we feed the
    // decoder declares 16-bit samples, so frames arrive at the target width
    // and, like MAME's SCALE_SAME path, samples are merely interleaved.
    let header = unsafe { (*frame).header };
    if header.channels != 2 || header.bits_per_sample != 16 || client.written >= client.target {
        client.failed = true;
        return libflac_sys::FLAC__STREAM_DECODER_WRITE_STATUS_ABORT;
    }
    let block_size = usize::try_from(header.blocksize).unwrap_or(usize::MAX);
    let samples = block_size.min(client.target - client.written);
    let channels = [unsafe { *buffer }, unsafe { *buffer.add(1) }];
    let out =
        unsafe { std::slice::from_raw_parts_mut(client.out.add(client.written * 4), samples * 4) };
    for sample in 0..samples {
        for (channel, samples) in channels.iter().enumerate() {
            let value = unsafe { *samples.add(sample) } as u16;
            let value = if client.big_endian {
                value.to_be_bytes()
            } else {
                value.to_le_bytes()
            };
            out[sample * 4 + channel * 2..][..2].copy_from_slice(&value);
        }
    }
    client.written += samples;
    libflac_sys::FLAC__STREAM_DECODER_WRITE_STATUS_CONTINUE
}

unsafe extern "C" fn metadata_callback(
    _decoder: *const FLAC__StreamDecoder,
    _metadata: *const libflac_sys::FLAC__StreamMetadata,
    _client_data: *mut c_void,
) {
}

unsafe extern "C" fn error_callback(
    _decoder: *const FLAC__StreamDecoder,
    _status: FLAC__StreamDecoderErrorStatus,
    client_data: *mut c_void,
) {
    let client = unsafe { &mut *client_data.cast::<Client>() };
    client.failed = true;
}

/// Decodes a headerless FLAC `payload` into `out`, where `block_size` is the
/// block size the encoder used and the samples are written big-endian when
/// `big_endian` says so. Returns the number of payload bytes the FLAC stream
/// consumed; the rest of the payload, if any, is another codec's.
pub(crate) fn flac_decode(
    payload: &[u8],
    out: &mut [u8],
    block_size: u32,
    big_endian: bool,
) -> Result<usize> {
    if !out.len().is_multiple_of(4) {
        return Err(Error::Corrupt(
            "FLAC chunk output is not a whole number of stereo samples".to_string(),
        ));
    }
    let header = custom_header(block_size.try_into().unwrap_or_default());
    let mut client = Client {
        header: &header,
        payload,
        position: 0,
        out: out.as_mut_ptr(),
        target: out.len() / 4,
        written: 0,
        big_endian,
        failed: false,
    };
    unsafe {
        let decoder = libflac_sys::FLAC__stream_decoder_new();
        if decoder.is_null() {
            return Err(Error::Corrupt("cannot allocate a FLAC decoder".to_string()));
        }
        libflac_sys::FLAC__stream_decoder_set_md5_checking(decoder, 0);
        let status = libflac_sys::FLAC__stream_decoder_init_stream(
            decoder,
            Some(read_callback),
            None,
            Some(tell_callback),
            None,
            None,
            Some(write_callback),
            Some(metadata_callback),
            Some(error_callback),
            (&raw mut client).cast(),
        );
        if status != libflac_sys::FLAC__STREAM_DECODER_INIT_STATUS_OK {
            libflac_sys::FLAC__stream_decoder_delete(decoder);
            return Err(Error::Corrupt(format!(
                "corrupt FLAC stream: init status {status}"
            )));
        }
        let mut decoded =
            libflac_sys::FLAC__stream_decoder_process_until_end_of_metadata(decoder) != 0;
        while decoded && !client.failed && client.written < client.target {
            decoded = libflac_sys::FLAC__stream_decoder_process_single(decoder) != 0;
        }
        let decoded = decoded && !client.failed;
        let mut position: FLAC__uint64 = 0;
        let located =
            libflac_sys::FLAC__stream_decoder_get_decode_position(decoder, &raw mut position) != 0;
        libflac_sys::FLAC__stream_decoder_finish(decoder);
        libflac_sys::FLAC__stream_decoder_delete(decoder);
        if !decoded {
            return Err(Error::Corrupt("corrupt FLAC stream".to_string()));
        }
        if !located {
            return Err(Error::Corrupt(
                "cannot locate the end of the FLAC stream".to_string(),
            ));
        }
        Ok((usize::try_from(position).unwrap_or(usize::MAX)).saturating_sub(HEADER_SIZE))
    }
}

/// The client data of the encoder: what MAME's FLAC buffer keeps while it
/// throws the container away — how many bytes of it are still to be
/// skipped, and whether the last metadata block has been passed — plus
/// the raw FLAC frames that survive.
struct EncodeClient {
    out: Vec<u8>,
    /// Bytes still to be skipped: the `fLaC` magic at first, then the
    /// payload of each metadata block.
    ignore_bytes: usize,
    /// Set once a metadata block flagged as the last one has been read;
    /// everything after it is audio.
    found_audio: bool,
}

unsafe extern "C" fn encode_write_callback(
    _encoder: *const FLAC__StreamEncoder,
    buffer: *const FLAC__byte,
    bytes: usize,
    _samples: u32,
    _current_frame: u32,
    client_data: *mut c_void,
) -> FLAC__StreamEncoderWriteStatus {
    let client = unsafe { &mut *client_data.cast::<EncodeClient>() };
    let mut offset = 0;
    while offset < bytes {
        if client.ignore_bytes != 0 {
            let ignored = (bytes - offset).min(client.ignore_bytes);
            offset += ignored;
            client.ignore_bytes -= ignored;
        } else if !client.found_audio {
            // A metadata block header: the block is the last one when the
            // top bit of its first byte is set, its length follows in
            // three bytes.
            if bytes - offset < 4 {
                return libflac_sys::FLAC__STREAM_ENCODER_WRITE_STATUS_FATAL_ERROR;
            }
            let header = unsafe { buffer.add(offset) };
            client.found_audio = unsafe { *header } & 0x80 != 0;
            client.ignore_bytes = unsafe {
                (u32::from(*header.add(1)) << 16)
                    | (u32::from(*header.add(2)) << 8)
                    | u32::from(*header.add(3))
            } as usize;
            offset += 4;
        } else {
            let rest = unsafe { std::slice::from_raw_parts(buffer.add(offset), bytes - offset) };
            client.out.extend_from_slice(rest);
            break;
        }
    }
    libflac_sys::FLAC__STREAM_ENCODER_WRITE_STATUS_OK
}

/// Encodes `plane` as 16-bit stereo samples in a headerless FLAC stream at
/// `block_size`, the mirror of MAME's FLAC compressor: the frames the
/// encoder produces, with the magic and every metadata block stripped.
/// `big_endian` says how the sample pairs in the plane are byte ordered,
/// which is how the plain FLAC codec tries both readings of a hunk.
pub(crate) fn flac_encode(plane: &[u8], block_size: u32, big_endian: bool) -> Result<Vec<u8>> {
    if !plane.len().is_multiple_of(4) {
        return Err(Error::Compression(
            "FLAC chunk input is not a whole number of stereo samples".to_string(),
        ));
    }
    let mut client = EncodeClient {
        out: Vec::new(),
        ignore_bytes: 4,
        found_audio: false,
    };
    unsafe {
        let encoder = libflac_sys::FLAC__stream_encoder_new();
        if encoder.is_null() {
            return Err(Error::Compression(
                "cannot allocate a FLAC encoder".to_string(),
            ));
        }
        libflac_sys::FLAC__stream_encoder_set_verify(encoder, 0);
        libflac_sys::FLAC__stream_encoder_set_streamable_subset(encoder, 0);
        libflac_sys::FLAC__stream_encoder_set_channels(encoder, 2);
        libflac_sys::FLAC__stream_encoder_set_bits_per_sample(encoder, 16);
        libflac_sys::FLAC__stream_encoder_set_sample_rate(encoder, 44_100);
        libflac_sys::FLAC__stream_encoder_set_compression_level(encoder, 8);
        libflac_sys::FLAC__stream_encoder_set_blocksize(encoder, block_size);
        libflac_sys::FLAC__stream_encoder_set_total_samples_estimate(encoder, 0);
        let status = libflac_sys::FLAC__stream_encoder_init_stream(
            encoder,
            Some(encode_write_callback),
            None,
            None,
            None,
            (&raw mut client).cast(),
        );
        if status != libflac_sys::FLAC__STREAM_ENCODER_INIT_STATUS_OK {
            libflac_sys::FLAC__stream_encoder_delete(encoder);
            return Err(Error::Compression(format!(
                "cannot initialize the FLAC encoder: init status {status}"
            )));
        }
        let mut encoded = true;
        let mut offset = 0;
        let mut remaining = plane.len() / 4;
        let mut samples = [0 as FLAC__int32; 2048];
        while remaining > 0 && encoded {
            // MAME converts and submits at most 1024 samples per channel.
            let batch = remaining.min(1024);
            for sample in 0..batch {
                let frame = &plane[offset + sample * 4..][..4];
                let (left, right) = if big_endian {
                    (
                        i16::from_be_bytes([frame[0], frame[1]]),
                        i16::from_be_bytes([frame[2], frame[3]]),
                    )
                } else {
                    (
                        i16::from_le_bytes([frame[0], frame[1]]),
                        i16::from_le_bytes([frame[2], frame[3]]),
                    )
                };
                samples[sample * 2] = left as FLAC__int32;
                samples[sample * 2 + 1] = right as FLAC__int32;
            }
            encoded = libflac_sys::FLAC__stream_encoder_process_interleaved(
                encoder,
                samples.as_ptr(),
                batch as u32,
            ) != 0;
            offset += batch * 4;
            remaining -= batch;
        }
        let finished = libflac_sys::FLAC__stream_encoder_finish(encoder) != 0;
        libflac_sys::FLAC__stream_encoder_delete(encoder);
        if !encoded {
            return Err(Error::Compression(
                "cannot compress the FLAC stream".to_string(),
            ));
        }
        if !finished {
            return Err(Error::Compression(
                "cannot finish the FLAC stream".to_string(),
            ));
        }
        Ok(client.out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flac_encode_roundtrips_through_flac_decode() {
        // A sector plane of eight CD frames of noise, and the block size
        // the cdfl codec derives from a 19584-byte hunk.
        let mut plane = Vec::new();
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        while plane.len() < 8 * 2352 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            plane.extend_from_slice(&state.to_be_bytes());
        }
        plane.truncate(8 * 2352);
        let block_size = 2352;
        let encoded = flac_encode(&plane, block_size, true).unwrap();
        let mut decoded = vec![0; plane.len()];
        let consumed = flac_decode(&encoded, &mut decoded, block_size, true).unwrap();
        assert_eq!(consumed, encoded.len());
        assert_eq!(decoded, plane);
    }
}
