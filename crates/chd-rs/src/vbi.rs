//! The LaserDisc VBI codes chdman records per frame in `AVLD` metadata: a
//! port of MAME's `src/lib/util/vbiparse.cpp`.
//!
//! The parser reads the luma of a field's lines 11 (the white flag) and 16
//! to 18 (Manchester-coded Philips codes). Lines are slices of the whole
//! frame buffer, so where MAME's averaging windows run off either end of a
//! line they read the neighbouring pixels in memory, as here.

/// The size of one frame's packed VBI data.
pub(crate) const VBI_PACKED_BYTES: usize = 16;

const MAX_SOURCE_WIDTH: usize = 1024;
const MAX_CLOCK_DIFF: i32 = 3;
const MASK_CAV_PICTURE: u32 = 0xf00000;
const CODE_CAV_PICTURE: u32 = 0xf00000;

/// The VBI data of a field.
#[derive(Default)]
struct Vbi {
    white: bool,
    line16: u32,
    line17: u32,
    line18: u32,
    line1718: u32,
}

/// A line of pixels: `pixels[start + x]`, `x` possibly off the line.
struct Line<'a> {
    pixels: &'a [u16],
    start: usize,
    width: usize,
    shift: u32,
}

impl Line<'_> {
    fn raw(&self, x: i64) -> u16 {
        usize::try_from(self.start as i64 + x)
            .ok()
            .and_then(|index| self.pixels.get(index))
            .copied()
            .unwrap_or(0)
    }

    fn value(&self, x: i64) -> u8 {
        (self.raw(x) >> self.shift) as u8
    }
}

/// `vbi_parse_manchester_code`: the 24 bits of a line, each with its
/// confidence above the bit, or `None`.
fn parse_manchester_code(line: &Line<'_>, expectedbits: usize) -> Option<[u32; 24]> {
    let width = line.width;
    if width > MAX_SOURCE_WIDTH {
        return None;
    }
    let (mut min, mut max) = (0xffu8, 0x00u8);
    for x in 0..width {
        let raw = line.value(x as i64);
        min = min.min(raw);
        max = max.max(raw);
    }
    if max < 0x80 || min > 0x80 {
        return None;
    }
    let mid = ((u32::from(min) + u32::from(max)) / 2) as u8;
    let min = mid - (mid - min) / 2;
    let max = mid + (max - mid) / 2;

    // MAME's buffer is uninitialised past the line, and reading before it
    // is undefined; both read as low here
    let mut srcabs = [0u8; MAX_SOURCE_WIDTH];
    // the first comparison is against the raw pixel, not its luma
    let mut srcabsval = u8::from(line.raw(0) > u16::from(mid));
    for (x, abs) in srcabs.iter_mut().enumerate().take(width) {
        let raw = line.value(x as i64);
        if raw >= max {
            srcabsval = 1;
        } else if raw <= min {
            srcabsval = 0;
        }
        *abs = srcabsval;
    }
    let abs = |index: i64| -> u8 {
        usize::try_from(index)
            .ok()
            .and_then(|index| srcabs.get(index))
            .copied()
            .unwrap_or(0)
    };

    let firstedge = (0..width.saturating_sub(1)).find(|&x| srcabs[x] != srcabs[x + 1])?;
    let firstedge = firstedge as i64;

    // the clock with a transition nearest each beat
    let mut bestclock = 0.0f64;
    let mut besterr = 1000;
    let mut clock = width as f64 / expectedbits as f64;
    while clock >= 2.0 {
        let mut error = 0;
        let mut x = 1;
        while x < expectedbits {
            let curbit = (firstedge as f64 + x as f64 * clock) as i64;
            let mut offby = 0;
            while offby <= MAX_CLOCK_DIFF {
                let off = i64::from(offby);
                if abs(curbit + off) != abs(curbit + off + 1)
                    || abs(curbit - off) != abs(curbit - off + 1)
                {
                    break;
                }
                offby += 1;
            }
            if offby > MAX_CLOCK_DIFF {
                break;
            }
            error += offby;
            if error >= besterr {
                break;
            }
            x += 1;
        }
        if x == expectedbits {
            besterr = error;
            bestclock = clock;
        }
        clock -= 1.0 / expectedbits as f64;
    }
    if bestclock == 0.0 {
        return None;
    }

    let mut result = [0u32; 24];
    for (x, bit) in result.iter_mut().enumerate().take(expectedbits) {
        let x = x as f64;
        let leftstart = firstedge + ((x - 0.5) * bestclock).ceil() as i64;
        let leftend = firstedge + (x * bestclock).floor() as i64;
        let rightstart = firstedge + (x * bestclock).ceil() as i64;
        let rightend = firstedge + ((x + 0.5) * bestclock).floor() as i64;
        let average = |start: i64, end: i64| -> (bool, i32) {
            let mut sum = 0i32;
            for tx in start..=end {
                sum += i32::from(line.value(tx)) - i32::from(mid);
            }
            (sum >= 0, sum.abs())
        };
        let (leftabs, leftavg) = average(leftstart, leftend);
        let (rightabs, rightavg) = average(rightstart, rightend);
        if leftabs == rightabs {
            return None;
        }
        *bit = u32::from(!leftabs && rightabs) | ((leftavg + rightavg) as u32) << 1;
    }
    Some(result)
}

/// `vbi_parse_white_flag`: whether the line's luma peaks near its top.
fn parse_white_flag(line: &Line<'_>) -> bool {
    let mut histo = [0i32; 256];
    for x in 0..line.width {
        histo[usize::from(line.value(x as i64))] += 1;
    }
    let mut subtract = (line.width / 100) as i32;
    let mut minval = 0;
    while minval < 255 {
        subtract -= histo[minval];
        if subtract < 0 {
            break;
        }
        minval += 1;
    }
    let mut subtract = (line.width / 100) as i32;
    let mut maxval = 255;
    while maxval > 0 {
        subtract -= histo[maxval];
        if subtract < 0 {
            break;
        }
        maxval -= 1;
    }
    let (minval, maxval) = (minval as i32, maxval as i32);
    if maxval - minval < 10 {
        return false;
    }
    let mut peakval = 0;
    for x in 1..256 {
        if histo[x] > histo[peakval] {
            peakval = x;
        }
    }
    peakval as i32 > minval + 9 * (maxval - minval) / 10
}

/// `vbi_parse_all` then `vbi_metadata_pack`: the packed VBI data of field
/// `framenum`, whose rows start every `rowpixels` pixels from `start`.
pub(crate) fn parse_and_pack(
    pixels: &[u16],
    start: usize,
    rowpixels: usize,
    width: usize,
    framenum: u32,
) -> [u8; VBI_PACKED_BYTES] {
    let line = |row: usize| Line {
        pixels,
        start: start + row * rowpixels,
        width,
        shift: 8,
    };
    let code = |bits: &[u32; 24]| bits.iter().fold(0u32, |code, bit| (code << 1) | (bit & 1));
    let mut vbi = Vbi {
        white: parse_white_flag(&line(11)),
        ..Vbi::default()
    };
    if let Some(bits) = parse_manchester_code(&line(16), 24) {
        vbi.line16 = code(&bits);
    }
    let bits17 = parse_manchester_code(&line(17), 24);
    if let Some(bits) = &bits17 {
        vbi.line17 = code(bits);
    }
    let bits18 = parse_manchester_code(&line(18), 24);
    if let Some(bits) = &bits18 {
        vbi.line18 = code(bits);
    }
    // only consulted when both lines decoded
    let first = bits17.unwrap_or([0; 24]);
    let second = bits18.unwrap_or([0; 24]);

    // the most plausible of lines 17 and 18
    if vbi.line17 == 0 {
        vbi.line1718 = vbi.line18;
    } else if vbi.line18 == 0 || vbi.line17 == vbi.line18 {
        vbi.line1718 = vbi.line17;
    } else {
        let bad_bcd = |code: u32| {
            (code & 0xf000) > 0x9000
                || (code & 0xf00) > 0x900
                || (code & 0xf0) > 0x90
                || (code & 0xf) > 0x9
        };
        if (vbi.line17 & MASK_CAV_PICTURE) == CODE_CAV_PICTURE
            && (vbi.line18 & MASK_CAV_PICTURE) == CODE_CAV_PICTURE
        {
            if bad_bcd(vbi.line17) {
                vbi.line1718 = vbi.line18;
            } else if bad_bcd(vbi.line18) {
                vbi.line1718 = vbi.line17;
            }
        }
        if vbi.line1718 == 0 {
            for bitnum in 0..24 {
                let bit = if first[bitnum] > second[bitnum] {
                    first[bitnum] & 1
                } else {
                    second[bitnum] & 1
                };
                vbi.line1718 = (vbi.line1718 << 1) | bit;
            }
        }
    }

    let mut packed = [0u8; VBI_PACKED_BYTES];
    packed[..3].copy_from_slice(&framenum.to_be_bytes()[1..]);
    packed[3] = u8::from(vbi.white);
    packed[4..7].copy_from_slice(&vbi.line16.to_be_bytes()[1..]);
    packed[7..10].copy_from_slice(&vbi.line17.to_be_bytes()[1..]);
    packed[10..13].copy_from_slice(&vbi.line18.to_be_bytes()[1..]);
    packed[13..16].copy_from_slice(&vbi.line1718.to_be_bytes()[1..]);
    packed
}
