//! The two hashes the VU1 tools use.
//!
//! `fnv` is the 64-bit FNV-1a that `VU1Interpreter::hashMicrocode` computes, which names the `vu1_<hash>.bin` files.
//! `blake2b_64_hex` is BLAKE2b with an 8-byte digest (Python's `hashlib.blake2b(data, digest_size=8)`), which the manifest
//! uses to fingerprint the pieces of the ELF.

const FNV_BASE: u64 = 1_469_598_103_934_665_603;
const FNV_PRIME: u64 = 1_099_511_628_211;

/// FNV-1a, 64 bits.
pub fn fnv(data: &[u8]) -> u64 {
    data.iter().fold(FNV_BASE, |hash, &byte| (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME))
}

const IV: [u64; 8] = [
    0x6a09_e667_f3bc_c908,
    0xbb67_ae85_84ca_a73b,
    0x3c6e_f372_fe94_f82b,
    0xa54f_f53a_5f1d_36f1,
    0x510e_527f_ade6_82d1,
    0x9b05_688c_2b3e_6c1f,
    0x1f83_d9ab_fb41_bd6b,
    0x5be0_cd19_137e_2179,
];

const SIGMA: [[usize; 16]; 12] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
    [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
    [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
    [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
    [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
    [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
    [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
    [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
    [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
];

fn mix(v: &mut [u64; 16], a: usize, b: usize, c: usize, d: usize, x: u64, y: u64) {
    v[a] = v[a].wrapping_add(v[b]).wrapping_add(x);
    v[d] = (v[d] ^ v[a]).rotate_right(32);
    v[c] = v[c].wrapping_add(v[d]);
    v[b] = (v[b] ^ v[c]).rotate_right(24);
    v[a] = v[a].wrapping_add(v[b]).wrapping_add(y);
    v[d] = (v[d] ^ v[a]).rotate_right(16);
    v[c] = v[c].wrapping_add(v[d]);
    v[b] = (v[b] ^ v[c]).rotate_right(63);
}

fn compress(h: &mut [u64; 8], block: &[u8], counter: u128, last: bool) {
    let mut m = [0u64; 16];
    for (word, chunk) in m.iter_mut().zip(block.chunks_exact(8)) {
        *word = u64::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7]]);
    }

    let mut v = [0u64; 16];
    v[..8].copy_from_slice(h);
    v[8..].copy_from_slice(&IV);
    v[12] ^= counter as u64;
    v[13] ^= (counter >> 64) as u64;
    if last {
        v[14] = !v[14];
    }

    for round in SIGMA {
        mix(&mut v, 0, 4, 8, 12, m[round[0]], m[round[1]]);
        mix(&mut v, 1, 5, 9, 13, m[round[2]], m[round[3]]);
        mix(&mut v, 2, 6, 10, 14, m[round[4]], m[round[5]]);
        mix(&mut v, 3, 7, 11, 15, m[round[6]], m[round[7]]);
        mix(&mut v, 0, 5, 10, 15, m[round[8]], m[round[9]]);
        mix(&mut v, 1, 6, 11, 12, m[round[10]], m[round[11]]);
        mix(&mut v, 2, 7, 8, 13, m[round[12]], m[round[13]]);
        mix(&mut v, 3, 4, 9, 14, m[round[14]], m[round[15]]);
    }

    for i in 0..8 {
        h[i] ^= v[i] ^ v[i + 8];
    }
}

/// BLAKE2b with an 8-byte digest, as 16 lowercase hexadecimal digits.
pub fn blake2b_64_hex(data: &[u8]) -> String {
    const DIGEST_LENGTH: u64 = 8;
    let mut h = IV;
    // Parameter block: digest length, no key, fanout 1, depth 1.
    h[0] ^= 0x0101_0000 ^ DIGEST_LENGTH;

    // Every block but the last is compressed as it is; the last one (possibly empty, for empty input) is padded.
    let full_blocks = if data.is_empty() { 0 } else { (data.len() - 1) / 128 };
    for index in 0..full_blocks {
        compress(&mut h, &data[index * 128..(index + 1) * 128], ((index + 1) * 128) as u128, false);
    }
    let mut tail = [0u8; 128];
    let rest = &data[full_blocks * 128..];
    tail[..rest.len()].copy_from_slice(rest);
    compress(&mut h, &tail, data.len() as u128, true);

    h[0].to_le_bytes().iter().map(|byte| format!("{byte:02x}")).collect()
}

/// SHA-1, fed in pieces: the checksum a CHD keeps of its raw data and of its metadata.
#[derive(Clone)]
pub struct Sha1 {
    state: [u32; 5],
    block: [u8; 64],
    filled: usize,
    length: u64,
}

impl Default for Sha1 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha1 {
    pub fn new() -> Self {
        Self {
            state: [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476, 0xC3D2_E1F0],
            block: [0; 64],
            filled: 0,
            length: 0,
        }
    }

    fn compress(state: &mut [u32; 5], block: &[u8]) {
        let mut w = [0u32; 80];
        for (word, bytes) in w.iter_mut().zip(block.chunks_exact(4)) {
            *word = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = *state;
        for (i, word) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let temp = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        for (value, add) in state.iter_mut().zip([a, b, c, d, e]) {
            *value = value.wrapping_add(add);
        }
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.length += data.len() as u64;
        if self.filled > 0 {
            let take = (64 - self.filled).min(data.len());
            self.block[self.filled..self.filled + take].copy_from_slice(&data[..take]);
            self.filled += take;
            data = &data[take..];
            if self.filled < 64 {
                return;
            }
            let block = self.block;
            Self::compress(&mut self.state, &block);
            self.filled = 0;
        }
        let mut blocks = data.chunks_exact(64);
        for block in &mut blocks {
            Self::compress(&mut self.state, block);
        }
        let rest = blocks.remainder();
        self.block[..rest.len()].copy_from_slice(rest);
        self.filled = rest.len();
    }

    pub fn finish(mut self) -> [u8; 20] {
        let bits = self.length.wrapping_mul(8);
        self.update(&[0x80]);
        while self.filled != 56 {
            self.update(&[0]);
        }
        self.update(&bits.to_be_bytes());
        let mut digest = [0u8; 20];
        for (bytes, value) in digest.chunks_exact_mut(4).zip(self.state) {
            bytes.copy_from_slice(&value.to_be_bytes());
        }
        digest
    }

    /// The SHA-1 of `data` in one call.
    pub fn digest(data: &[u8]) -> [u8; 20] {
        let mut hasher = Self::new();
        hasher.update(data);
        hasher.finish()
    }
}

#[cfg(test)]
mod sha1_tests {
    use super::Sha1;

    fn hex(digest: [u8; 20]) -> String {
        digest.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn sha1_matches_the_reference_vectors() {
        assert_eq!(hex(Sha1::digest(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(hex(Sha1::digest(b"abc")), "a9993e364706816aba3e25717850c26c9cd0d89d");
        let long = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        assert_eq!(hex(Sha1::digest(long)), "84983e441c3bd26ebaae4aa1f95129e5e54670f1");
        // Fed in uneven pieces, the result is the same.
        let mut pieces = Sha1::new();
        for chunk in long.chunks(7) {
            pieces.update(chunk);
        }
        assert_eq!(hex(pieces.finish()), "84983e441c3bd26ebaae4aa1f95129e5e54670f1");
    }
}
