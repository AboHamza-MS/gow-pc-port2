//! A small FLAC frame decoder, for the CHD codecs that store a hunk as 16-bit stereo audio (`flac` and `cdfl`).
//!
//! A CHD keeps only the FLAC frames of a hunk, with no `fLaC` stream header: the format is fixed (44.1 kHz, two channels,
//! 16 bits) and the decoder is told the block size, as MAME's `flac_decoder` is. Every frame and subframe type of the
//! FLAC format is read (constant, verbatim, fixed and LPC predictors, both Rice codings, the stereo decorrelations);
//! the frame CRCs are not checked, because the CHD's own CRC of the hunk covers the result.

/// Reads bits most significant first, as FLAC writes them.
struct Bits<'a> {
    data: &'a [u8],
    /// Position in bits.
    position: usize,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, position: 0 }
    }

    fn bit(&mut self) -> Result<u32, String> {
        let byte = *self.data.get(self.position / 8).ok_or("the FLAC data ends inside a frame")?;
        let value = (byte >> (7 - self.position % 8)) & 1;
        self.position += 1;
        Ok(u32::from(value))
    }

    fn read(&mut self, count: u32) -> Result<u32, String> {
        let mut value = 0u32;
        for _ in 0..count {
            value = (value << 1) | self.bit()?;
        }
        Ok(value)
    }

    fn signed(&mut self, count: u32) -> Result<i32, String> {
        if count == 0 {
            return Ok(0);
        }
        let value = self.read(count)?;
        let shift = 32 - count;
        Ok(((value << shift) as i32) >> shift)
    }

    /// Zeros before the next one bit.
    fn unary(&mut self) -> Result<u32, String> {
        let mut count = 0;
        while self.bit()? == 0 {
            count += 1;
            if count > 1 << 20 {
                return Err("a FLAC unary code never ends".into());
            }
        }
        Ok(count)
    }

    fn align(&mut self) {
        self.position = self.position.div_ceil(8) * 8;
    }
}

fn residual(bits: &mut Bits, block: usize, order: usize, out: &mut [i64]) -> Result<(), String> {
    let method = bits.read(2)?;
    let (parameter_bits, escape) = match method {
        0 => (4, 15),
        1 => (5, 31),
        _ => return Err("a FLAC residual uses a reserved coding".into()),
    };
    let partition_order = bits.read(4)?;
    let partitions = 1usize << partition_order;
    if block % partitions != 0 || block >> partition_order < order {
        return Err("a FLAC residual partition does not fit its block".into());
    }
    let mut index = order;
    for partition in 0..partitions {
        let count = (block >> partition_order) - if partition == 0 { order } else { 0 };
        let parameter = bits.read(parameter_bits)?;
        if parameter == escape {
            let raw = bits.read(5)?;
            for _ in 0..count {
                out[index] = i64::from(bits.signed(raw)?);
                index += 1;
            }
        } else {
            for _ in 0..count {
                let high = u64::from(bits.unary()?);
                let low = u64::from(bits.read(parameter)?);
                let folded = (high << parameter) | low;
                out[index] = ((folded >> 1) as i64) ^ -((folded & 1) as i64);
                index += 1;
            }
        }
    }
    Ok(())
}

fn subframe(bits: &mut Bits, block: usize, sample_bits: u32, out: &mut [i64]) -> Result<(), String> {
    if bits.bit()? != 0 {
        return Err("a FLAC subframe starts with a set padding bit".into());
    }
    let kind = bits.read(6)?;
    let wasted = if bits.bit()? == 1 { bits.unary()? + 1 } else { 0 };
    if wasted >= sample_bits {
        return Err("a FLAC subframe wastes every bit".into());
    }
    let size = sample_bits - wasted;
    match kind {
        0 => {
            let value = i64::from(bits.signed(size)?);
            out[..block].fill(value);
        }
        1 => {
            for sample in out[..block].iter_mut() {
                *sample = i64::from(bits.signed(size)?);
            }
        }
        8..=12 => {
            let order = (kind - 8) as usize;
            if order > block {
                return Err("a FLAC predictor is longer than its block".into());
            }
            for sample in out[..order].iter_mut() {
                *sample = i64::from(bits.signed(size)?);
            }
            residual(bits, block, order, out)?;
            for i in order..block {
                let predicted = match order {
                    0 => 0,
                    1 => out[i - 1],
                    2 => 2 * out[i - 1] - out[i - 2],
                    3 => 3 * out[i - 1] - 3 * out[i - 2] + out[i - 3],
                    _ => 4 * out[i - 1] - 6 * out[i - 2] + 4 * out[i - 3] - out[i - 4],
                };
                out[i] += predicted;
            }
        }
        32..=63 => {
            let order = (kind - 31) as usize;
            if order > block {
                return Err("a FLAC predictor is longer than its block".into());
            }
            for sample in out[..order].iter_mut() {
                *sample = i64::from(bits.signed(size)?);
            }
            let precision = bits.read(4)? + 1;
            if precision == 16 {
                return Err("a FLAC predictor has an invalid precision".into());
            }
            let shift = bits.signed(5)?;
            if shift < 0 {
                return Err("a FLAC predictor has a negative shift".into());
            }
            let mut coefficients = [0i64; 32];
            for coefficient in coefficients[..order].iter_mut() {
                *coefficient = i64::from(bits.signed(precision)?);
            }
            residual(bits, block, order, out)?;
            for i in order..block {
                let sum: i64 = (0..order).map(|j| coefficients[j] * out[i - 1 - j]).sum();
                out[i] += sum >> shift;
            }
        }
        _ => return Err(format!("a FLAC subframe has the reserved type {kind}")),
    }
    if wasted > 0 {
        for sample in out[..block].iter_mut() {
            *sample <<= wasted;
        }
    }
    Ok(())
}

/// Skips the UTF-8-like frame or sample number of a frame header.
fn coded_number(bits: &mut Bits) -> Result<(), String> {
    let first = bits.read(8)? as u8;
    let ones = first.leading_ones();
    if ones == 1 || ones > 7 {
        return Err("a FLAC frame number is malformed".into());
    }
    let extra = ones.saturating_sub(1);
    for _ in 0..extra {
        if bits.read(8)? & 0xC0 != 0x80 {
            return Err("a FLAC frame number is malformed".into());
        }
    }
    Ok(())
}

/// Decodes `samples` stereo samples from the frames in `data` into `out` (16 bits each, channel 0 first, big- or
/// little-endian) and returns how many bytes of `data` the frames took: the CD codec's subcode follows them.
pub(crate) fn decode_stereo16(
    data: &[u8],
    block_size: u32,
    samples: usize,
    big_endian: bool,
    out: &mut [u8],
) -> Result<usize, String> {
    if out.len() < samples * 4 {
        return Err("the FLAC output buffer is too small".into());
    }
    let mut bits = Bits::new(data);
    let mut channels = [vec![0i64; 65536], vec![0i64; 65536]];
    let mut written = 0usize;
    while written < samples {
        if bits.read(14)? != 0x3FFE {
            return Err("a FLAC frame does not start with its sync code".into());
        }
        bits.read(2)?;
        let size_code = bits.read(4)?;
        let rate_code = bits.read(4)?;
        let assignment = bits.read(4)?;
        let sample_bits = match bits.read(3)? {
            0 | 4 => 16,
            1 => 8,
            2 => 12,
            5 => 20,
            6 => 24,
            7 => 32,
            _ => return Err("a FLAC frame uses a reserved sample size".into()),
        };
        bits.read(1)?;
        coded_number(&mut bits)?;
        let block = match size_code {
            0 => block_size as usize,
            1 => 192,
            2..=5 => 576 << (size_code - 2),
            6 => bits.read(8)? as usize + 1,
            7 => bits.read(16)? as usize + 1,
            _ => 256 << (size_code - 8),
        };
        match rate_code {
            12 => {
                bits.read(8)?;
            }
            13 | 14 => {
                bits.read(16)?;
            }
            15 => return Err("a FLAC frame uses an invalid sample rate".into()),
            _ => {}
        }
        bits.read(8)?; // CRC-8 of the header
        if !(assignment == 1 || (8..=10).contains(&assignment)) {
            return Err("a CHD FLAC frame must be stereo".into());
        }
        if block > 65536 {
            return Err("a FLAC block is too big".into());
        }
        let [left, right] = &mut channels;
        // The side channel carries one more bit: first in side/right, second in left/side and mid/side.
        let side_first = u32::from(assignment == 9);
        let side_second = u32::from(assignment == 8 || assignment == 10);
        subframe(&mut bits, block, sample_bits + side_first, left)?;
        subframe(&mut bits, block, sample_bits + side_second, right)?;
        for i in 0..block {
            let (l, r) = match assignment {
                8 => (left[i], left[i] - right[i]),
                9 => (left[i] + right[i], right[i]),
                10 => {
                    let mid = (left[i] << 1) | (right[i] & 1);
                    ((mid + right[i]) >> 1, (mid - right[i]) >> 1)
                }
                _ => (left[i], right[i]),
            };
            if written < samples {
                for (channel, value) in [l, r].into_iter().enumerate() {
                    let value = value as i16;
                    let bytes = if big_endian { value.to_be_bytes() } else { value.to_le_bytes() };
                    out[written * 4 + channel * 2..written * 4 + channel * 2 + 2].copy_from_slice(&bytes);
                }
                written += 1;
            }
        }
        bits.align();
        bits.read(16)?; // CRC-16 of the frame
    }
    Ok(bits.position / 8)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Writes bits most significant first, to build test frames.
    #[derive(Default)]
    pub(crate) struct BitWriter {
        pub bytes: Vec<u8>,
        used: u32,
    }

    impl BitWriter {
        pub fn put(&mut self, value: u64, count: u32) {
            for shift in (0..count).rev() {
                if self.used == 0 {
                    self.bytes.push(0);
                }
                let last = self.bytes.last_mut().unwrap();
                *last |= (((value >> shift) & 1) as u8) << (7 - self.used);
                self.used = (self.used + 1) % 8;
            }
        }

        pub fn align(&mut self) {
            self.used = 0;
        }
    }

    /// One stereo frame of `left`/`right` 16-bit samples: left verbatim, right as a fixed order-1 predictor with a
    /// Rice-coded residual, independent channels.
    pub(crate) fn frame(left: &[i16], right: &[i16]) -> Vec<u8> {
        let mut w = BitWriter::default();
        w.put(0x3FFE, 14);
        w.put(0, 2);
        w.put(7, 4); // 16-bit block size follows
        w.put(0, 4); // sample rate from the stream
        w.put(1, 4); // two independent channels
        w.put(4, 3); // 16 bits
        w.put(0, 1);
        w.put(0, 8); // frame number 0
        w.put(left.len() as u64 - 1, 16);
        w.put(0, 8); // CRC-8, not checked
                     // Left: verbatim.
        w.put(0, 1);
        w.put(1, 6);
        w.put(0, 1);
        for &sample in left {
            w.put(u64::from(sample as u16), 16);
        }
        // Right: fixed order 1, one partition, Rice parameter 3.
        w.put(0, 1);
        w.put(9, 6);
        w.put(0, 1);
        w.put(u64::from(right[0] as u16), 16);
        w.put(0, 2);
        w.put(0, 4);
        w.put(3, 4);
        for i in 1..right.len() {
            let delta = i64::from(right[i]) - i64::from(right[i - 1]);
            let folded = ((delta << 1) ^ (delta >> 63)) as u64;
            for _ in 0..folded >> 3 {
                w.put(0, 1);
            }
            w.put(1, 1);
            w.put(folded & 7, 3);
        }
        w.align();
        w.put(0, 16); // CRC-16, not checked
        w.bytes
    }

    #[test]
    fn verbatim_and_fixed_subframes_decode() {
        let left: Vec<i16> = (0..32).map(|i| (i * 1000 - 9000) as i16).collect();
        let right: Vec<i16> = (0..32).map(|i| (i * 3 - 40) as i16).collect();
        let mut data = frame(&left, &right);
        let frame_length = data.len();
        data.extend_from_slice(b"tail");
        let mut out = vec![0u8; 32 * 4];
        let used = decode_stereo16(&data, 32, 32, true, &mut out).unwrap();
        assert_eq!(used, frame_length, "the subcode starts right after the frames");
        for i in 0..32 {
            assert_eq!(i16::from_be_bytes([out[i * 4], out[i * 4 + 1]]), left[i]);
            assert_eq!(i16::from_be_bytes([out[i * 4 + 2], out[i * 4 + 3]]), right[i]);
        }
    }

    #[test]
    fn a_constant_mid_side_frame_decodes() {
        let mut w = BitWriter::default();
        w.put(0x3FFE, 14);
        w.put(0, 2);
        w.put(6, 4); // 8-bit block size follows
        w.put(0, 4);
        w.put(10, 4); // mid/side
        w.put(0, 3);
        w.put(0, 1);
        w.put(0, 8);
        w.put(3, 8); // 4 samples
        w.put(0, 8);
        // mid = 100 (16 bits), side = -2 (17 bits): left 99, right 101.
        w.put(0, 1);
        w.put(0, 6);
        w.put(0, 1);
        w.put(100, 16);
        w.put(0, 1);
        w.put(0, 6);
        w.put(0, 1);
        w.put((-2i64 as u64) & 0x1FFFF, 17);
        w.align();
        w.put(0, 16);
        let mut out = vec![0u8; 16];
        decode_stereo16(&w.bytes, 4, 4, false, &mut out).unwrap();
        assert_eq!(i16::from_le_bytes([out[0], out[1]]), 99);
        assert_eq!(i16::from_le_bytes([out[2], out[3]]), 101);
    }
}
