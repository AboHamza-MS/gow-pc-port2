//! GOW-Port (decision D4, discs of level 1): which disc a game executable comes from.
//!
//! The port recompiles the user's own ELF, so any disc whose code is the USA code produces the same generated C++: fan
//! translations change text and textures but keep `.text` byte for byte. They are told apart from the original by the
//! ELF CRC (PCSX2's XOR of all 32-bit words) and recognised as compatible by the SHA-1 of `.text`.

use crate::ElfFile;

/// PCSX2's game CRC of the original USA executable (SCUS_973.99).
pub const USA_ELF_CRC: u32 = 0xD638_5328;
/// SHA-1 of the `.text` section of the original USA executable: the code the function map and the overrides describe.
pub const USA_TEXT_SHA1: &str = "8c6e61a95e84e88b01a0cea25b3824cf6b2cbe00";

/// How an executable relates to the supported game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeIdentity {
    /// The original USA executable.
    Original,
    /// Another executable with the USA code (a translation, or an edition that only changed data).
    SameCode,
    /// Different code: not the supported game.
    Other,
}

/// PCSX2's game CRC: the XOR of all the 32-bit little-endian words of the file.
pub fn pcsx2_crc(elf: &[u8]) -> u32 {
    elf.chunks_exact(4).fold(0u32, |crc, word| crc ^ u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
}

/// SHA-1 (hex) of the ELF's `.text` section, `None` when the file does not parse or has no `.text` data.
pub fn text_sha1(elf: &[u8]) -> Option<String> {
    let parsed = ElfFile::parse(elf).ok()?;
    let text = parsed.sections().iter().find(|section| section.name == ".text")?.data?;
    Some(sha1_hex(text))
}

/// Original, same code, or something else.
pub fn identify(elf: &[u8]) -> CodeIdentity {
    if pcsx2_crc(elf) == USA_ELF_CRC {
        CodeIdentity::Original
    } else if text_sha1(elf).as_deref() == Some(USA_TEXT_SHA1) {
        CodeIdentity::SameCode
    } else {
        CodeIdentity::Other
    }
}

/// SHA-1 (FIPS 180-4) of `data`, lowercase hex. Small and dependency-free: it only identifies executables.
pub fn sha1_hex(data: &[u8]) -> String {
    let mut h: [u32; 5] = [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476, 0xC3D2_E1F0];
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&((data.len() as u64).wrapping_mul(8)).to_be_bytes());
    for block in message.chunks_exact(64) {
        let mut w = [0u32; 80];
        for (i, word) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, &word) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let next = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = next;
        }
        for (slot, value) in h.iter_mut().zip([a, b, c, d, e]) {
            *slot = slot.wrapping_add(value);
        }
    }
    h.iter().map(|word| format!("{word:08x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha1_matches_the_standard_vectors() {
        assert_eq!(sha1_hex(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            sha1_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
        assert_eq!(sha1_hex(&vec![b'a'; 1_000_000]), "34aa973cd4c4daa4f61eeb2bdbad27316534016f");
    }

    #[test]
    fn the_crc_is_the_xor_of_the_words() {
        let mut bytes = USA_ELF_CRC.to_le_bytes().to_vec();
        assert_eq!(pcsx2_crc(&bytes), USA_ELF_CRC);
        bytes.extend_from_slice(&[1, 0, 0, 0, 1, 0, 0, 0]);
        assert_eq!(pcsx2_crc(&bytes), USA_ELF_CRC, "two equal words cancel out");
        // An unparsable file with another CRC is not the game.
        assert_eq!(identify(b"not an elf at all"), CodeIdentity::Other);
    }
}
