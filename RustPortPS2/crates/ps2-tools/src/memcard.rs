//! GOW-Port (R5): `mc-format` and `mc-check`, the 8 MB memory card image the port uses (PCSX2 `.ps2`: 16384 pages of
//! 512 bytes, each followed by 16 spare bytes with its ECC).
//!
//! The game can format a blank card itself, but that path was never verified in the port, so the installer creates the
//! card already formatted, laid out like a card the PS2 BIOS formats: superblock, one indirect FAT cluster, 32 FAT
//! clusters and a root directory holding `.` and `..`. Every field matches a BIOS-formatted card, and the ECC is the
//! one the card's controller computes (three bytes per 128-byte quarter of a page).

use std::path::Path;

pub const PAGE_SIZE: usize = 512;
pub const SPARE_SIZE: usize = 16;
pub const RAW_PAGE_SIZE: usize = PAGE_SIZE + SPARE_SIZE;
pub const PAGE_COUNT: usize = 0x4000;
pub const CARD_SIZE: usize = RAW_PAGE_SIZE * PAGE_COUNT;

const PAGES_PER_CLUSTER: usize = 2;
const CLUSTER_SIZE: usize = PAGE_SIZE * PAGES_PER_CLUSTER;
const CLUSTERS_PER_CARD: u32 = 8192;
const INDIRECT_FAT_CLUSTER: u32 = 8;
const FAT_CLUSTERS: u32 = 32;
const ALLOC_OFFSET: u32 = INDIRECT_FAT_CLUSTER + 1 + FAT_CLUSTERS; // 41
const ALLOC_END: u32 = CLUSTERS_PER_CARD - ALLOC_OFFSET - 16; // the two backup blocks
const FAT_ENTRIES_PER_CLUSTER: u32 = (CLUSTER_SIZE / 4) as u32;
/// Directory entry modes: readable, writable, executable, directory, exists (`.` also carries the "0x400" bit).
const MODE_DOT: u16 = 0x8427;
const MODE_DOTDOT: u16 = 0xA426;
const MAGIC: &[u8] = b"Sony PS2 Memory Card Format 1.2.0.0";

/// A card date: what the console stores (the game's own clock is Japan time, UTC+9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CardTime {
    pub second: u8,
    pub minute: u8,
    pub hour: u8,
    pub day: u8,
    pub month: u8,
    pub year: u16,
}

impl CardTime {
    fn bytes(self) -> [u8; 8] {
        let year = self.year.to_le_bytes();
        [0, self.second, self.minute, self.hour, self.day, self.month, year[0], year[1]]
    }

    /// The current time in Japan time, as a console would stamp it.
    pub fn now_japan() -> Self {
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or(0)
            + 9 * 3600;
        let days = (seconds / 86_400) as i64;
        let in_day = seconds % 86_400;
        // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let day_of_era = z - era * 146_097;
        let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let month_index = (5 * day_of_year + 2) / 153;
        let day = day_of_year - (153 * month_index + 2) / 5 + 1;
        let month = if month_index < 10 { month_index + 3 } else { month_index - 9 };
        let year = year_of_era + era * 400 + i64::from(month <= 2);
        Self {
            second: (in_day % 60) as u8,
            minute: (in_day / 60 % 60) as u8,
            hour: (in_day / 3600) as u8,
            day: day as u8,
            month: month as u8,
            year: year as u16,
        }
    }
}

fn parity(byte: u8) -> u8 {
    (byte.count_ones() & 1) as u8
}

/// The three ECC bytes of one 128-byte quarter of a page: column parity, then the line parity and its complement.
pub fn ecc(chunk: &[u8]) -> [u8; 3] {
    const MASKS: [u8; 7] = [0x55, 0x33, 0x0F, 0x00, 0xAA, 0xCC, 0xF0];
    let (mut column, mut line0, mut line1) = (0x77u8, 0x7Fu8, 0x7Fu8);
    for (index, &byte) in chunk.iter().enumerate() {
        for (bit, mask) in MASKS.iter().enumerate() {
            column ^= parity(byte & mask) << bit;
        }
        if parity(byte) != 0 {
            line0 ^= !(index as u8);
            line1 ^= index as u8;
        }
    }
    [column, line0 & 0x7F, line1]
}

/// The spare area of a page: four ECCs, then four erased bytes.
pub fn spare(page: &[u8]) -> [u8; SPARE_SIZE] {
    let mut spare = [0xFFu8; SPARE_SIZE];
    for (quarter, chunk) in page.chunks_exact(128).enumerate() {
        spare[quarter * 3..quarter * 3 + 3].copy_from_slice(&ecc(chunk));
    }
    spare
}

fn put_page(card: &mut [u8], page: usize, data: &[u8]) {
    let start = page * RAW_PAGE_SIZE;
    card[start..start + PAGE_SIZE].copy_from_slice(&data[..PAGE_SIZE]);
    card[start + PAGE_SIZE..start + RAW_PAGE_SIZE].copy_from_slice(&spare(&data[..PAGE_SIZE]));
}

fn put_cluster(card: &mut [u8], cluster: u32, data: &[u8]) {
    let first = cluster as usize * PAGES_PER_CLUSTER;
    for (index, page) in data.chunks_exact(PAGE_SIZE).enumerate() {
        put_page(card, first + index, page);
    }
}

fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn superblock() -> [u8; PAGE_SIZE] {
    let mut block = [0xFFu8; PAGE_SIZE];
    block[..0x28].fill(0);
    block[..MAGIC.len()].copy_from_slice(MAGIC);
    put_u16(&mut block, 0x28, PAGE_SIZE as u16);
    put_u16(&mut block, 0x2A, PAGES_PER_CLUSTER as u16);
    put_u16(&mut block, 0x2C, 16); // pages per block
    put_u16(&mut block, 0x2E, 0xFF00);
    put_u32(&mut block, 0x30, CLUSTERS_PER_CARD);
    put_u32(&mut block, 0x34, ALLOC_OFFSET);
    put_u32(&mut block, 0x38, ALLOC_END);
    put_u32(&mut block, 0x3C, 0); // root directory cluster
    put_u32(&mut block, 0x40, 1023); // backup blocks
    put_u32(&mut block, 0x44, 1022);
    block[0x48..0xD0].fill(0); // reserved, then the indirect FAT cluster list
    put_u32(&mut block, 0x50, INDIRECT_FAT_CLUSTER);
    block[0x150] = 2; // card type: PS2
    block[0x151] = 0x2B; // card flags, as the BIOS writes them
    put_u32(&mut block, 0x154, CLUSTER_SIZE as u32);
    put_u32(&mut block, 0x158, FAT_ENTRIES_PER_CLUSTER);
    put_u32(&mut block, 0x15C, 8); // clusters per block
    put_u32(&mut block, 0x160, 0xFFFF_FFFF); // "card form"
    block[0x164..0x170].fill(0);
    put_u32(&mut block, 0x170, 0x1F41); // largest allocatable count the BIOS records
    block
}

fn directory_entry(mode: u16, length: u32, time: CardTime, name: &[u8]) -> [u8; PAGE_SIZE] {
    let mut entry = [0u8; PAGE_SIZE];
    put_u16(&mut entry, 0x00, mode);
    put_u32(&mut entry, 0x04, length);
    entry[0x08..0x10].copy_from_slice(&time.bytes()); // created
    entry[0x18..0x20].copy_from_slice(&time.bytes()); // modified
    entry[0x40..0x40 + name.len()].copy_from_slice(name);
    entry
}

/// A formatted, empty 8 MB card.
pub fn format(time: CardTime) -> Vec<u8> {
    let mut card = vec![0xFFu8; CARD_SIZE];
    put_page(&mut card, 0, &superblock());

    let mut indirect = [0xFFu8; CLUSTER_SIZE];
    for index in 0..FAT_CLUSTERS {
        put_u32(&mut indirect, index as usize * 4, INDIRECT_FAT_CLUSTER + 1 + index);
    }
    put_cluster(&mut card, INDIRECT_FAT_CLUSTER, &indirect);

    // FAT: the root directory is one cluster (end of chain), every other allocatable cluster is free, and the entries past
    // the allocatable end are marked used so nothing allocates them.
    for fat_cluster in 0..FAT_CLUSTERS {
        let mut cluster = [0u8; CLUSTER_SIZE];
        for slot in 0..FAT_ENTRIES_PER_CLUSTER {
            let entry = fat_cluster * FAT_ENTRIES_PER_CLUSTER + slot;
            let value = if entry == 0 || entry >= ALLOC_END { 0xFFFF_FFFF } else { 0x7FFF_FFFF };
            put_u32(&mut cluster, slot as usize * 4, value);
        }
        put_cluster(&mut card, INDIRECT_FAT_CLUSTER + 1 + fat_cluster, &cluster);
    }

    let mut root = [0u8; CLUSTER_SIZE];
    root[..PAGE_SIZE].copy_from_slice(&directory_entry(MODE_DOT, 2, time, b"."));
    root[PAGE_SIZE..].copy_from_slice(&directory_entry(MODE_DOTDOT, 0, time, b".."));
    put_cluster(&mut card, ALLOC_OFFSET, &root);
    card
}

/// What `check` found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckReport {
    pub formatted: bool,
    /// Pages whose ECC does not match their data (erased pages and pages with an erased spare area are skipped).
    pub ecc_errors: Vec<usize>,
}

/// Checks an image: its size, whether it is formatted (a blank card is valid: the game can format it) and the ECC of
/// every written page.
pub fn check(card: &[u8]) -> Result<CheckReport, String> {
    if card.len() != CARD_SIZE {
        return Err(format!("the card has {} bytes; an 8 MB PS2 card image with ECC has {CARD_SIZE}", card.len()));
    }
    let blank = card.iter().all(|&byte| byte == 0xFF);
    let formatted = card.starts_with(MAGIC);
    if !blank && !formatted {
        return Err("the card is not formatted and not blank: the superblock is damaged".to_owned());
    }
    let mut ecc_errors = Vec::new();
    for (index, page) in card.chunks_exact(RAW_PAGE_SIZE).enumerate() {
        let (data, stored) = page.split_at(PAGE_SIZE);
        if stored[..12].iter().all(|&byte| byte == 0xFF) {
            continue; // erased, or written without ECC
        }
        if stored[..12] != spare(data)[..12] {
            ecc_errors.push(index);
        }
    }
    Ok(CheckReport { formatted, ecc_errors })
}

/// `mc-format`: writes a formatted card to `path`, refusing to replace an existing file.
pub fn format_file(path: &Path) -> Result<(), String> {
    if path.exists() {
        return Err(format!("{} already exists: the card is never replaced", path.display()));
    }
    let temporary = path.with_extension("ps2.tmp");
    std::fs::write(&temporary, format(CardTime::now_japan())).and_then(|()| std::fs::rename(&temporary, path)).map_err(
        |error| {
            let _ = std::fs::remove_file(&temporary);
            format!("{}: {error}", path.display())
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ecc_of_an_erased_quarter_is_the_controller_s() {
        assert_eq!(ecc(&[0u8; 128]), [0x77, 0x7F, 0x7F]);
        assert_eq!(ecc(&[0xFFu8; 128]), [0x77, 0x7F, 0x7F], "an even count of each bit cancels out");
        let mut one = [0u8; 128];
        one[5] = 1;
        let [column, line0, line1] = ecc(&one);
        assert_eq!((line0, line1), (0x7F ^ (!5u8 & 0x7F), 0x7F ^ 5));
        assert_ne!(column, 0x77);
    }

    #[test]
    fn the_layout_matches_a_bios_formatted_card() {
        assert_eq!((ALLOC_OFFSET, ALLOC_END), (41, 8135));
        let card = format(CardTime { second: 0, minute: 0, hour: 0, day: 1, month: 1, year: 2026 });
        let report = check(&card).unwrap();
        assert!(report.formatted && report.ecc_errors.is_empty());
        // Root directory: absolute cluster 41, pages 82 and 83.
        assert_eq!(&card[82 * RAW_PAGE_SIZE..82 * RAW_PAGE_SIZE + 2], &MODE_DOT.to_le_bytes());
        assert_eq!(card[83 * RAW_PAGE_SIZE + 0x40..83 * RAW_PAGE_SIZE + 0x42], *b"..");
        // FAT entry 0 ends the root's chain, entry 1 is free.
        let fat = 18 * RAW_PAGE_SIZE;
        assert_eq!(card[fat..fat + 8], [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x7F]);
    }

    #[test]
    fn a_card_of_the_wrong_size_or_with_a_bad_page_is_reported() {
        assert!(check(&[0xFF; 100]).is_err());
        assert_eq!(check(&vec![0xFF; CARD_SIZE]).unwrap(), CheckReport { formatted: false, ecc_errors: vec![] });
        let mut card = format(CardTime::now_japan());
        card[18 * RAW_PAGE_SIZE + 7] ^= 0x01;
        assert_eq!(check(&card).unwrap().ecc_errors, vec![18]);
        card[0] = 0;
        assert!(check(&card).is_err(), "a damaged superblock");
    }

    #[test]
    fn japan_time_is_a_valid_date() {
        let time = CardTime::now_japan();
        assert!(time.year >= 2024 && (1..=12).contains(&time.month) && (1..=31).contains(&time.day) && time.hour < 24);
    }
}
