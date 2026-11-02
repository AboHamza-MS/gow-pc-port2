//! Tests of the installer tools. No game files: every image is built in memory, and the generator goldens come from the
//! Python tools these replaced (run on the same synthetic inputs).

use std::fs;
use std::path::{Path, PathBuf};

use ps2_tools::elfcheck::{inspect_bytes, EXPECTED_CRC};
use ps2_tools::hash::{blake2b_64_hex, fnv};
use ps2_tools::{iop_gen, iso, vu1, vu1n_gen};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ps2-tools-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn golden(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(path)
}

fn normalized(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display())).replace("\r\n", "\n")
}

// ---- hashes -----------------------------------------------------------------------------------------------------------

#[test]
fn fnv_matches_the_microcode_hash() {
    assert_eq!(fnv(b""), 0x1465_0fb0_739d_0383);
    assert_eq!(fnv(b"abc"), 0xe168_0151_0db8_9efd);
    let all: Vec<u8> = (0..=255).collect();
    assert_eq!(fnv(&all), 0x16d1_73bd_fcda_e583);
}

#[test]
fn blake2b_with_an_eight_byte_digest_matches_python() {
    let long: Vec<u8> = (0..=255u8).cycle().take(768).collect();
    assert_eq!(blake2b_64_hex(b""), "e4a6a0577479b2b4");
    assert_eq!(blake2b_64_hex(b"abc"), "d8bb14d833d59559");
    assert_eq!(blake2b_64_hex(&long), "6cdb98d18e1d1970");
    // The block boundary: a full block is the last one, one byte more adds a block.
    assert_eq!(blake2b_64_hex(&[b'x'; 128]), "901ce374e8e12402");
    assert_eq!(blake2b_64_hex(&[b'y'; 129]), "7c617a149f5301e6");
}

// ---- verify-elf -------------------------------------------------------------------------------------------------------

fn tiny_elf() -> Vec<u8> {
    let mut data = vec![0u8; 128];
    data[..7].copy_from_slice(b"\x7fELF\x01\x01\x01");
    let put16 = |data: &mut Vec<u8>, at: usize, value: u16| data[at..at + 2].copy_from_slice(&value.to_le_bytes());
    let put32 = |data: &mut Vec<u8>, at: usize, value: u32| data[at..at + 4].copy_from_slice(&value.to_le_bytes());
    put16(&mut data, 16, 2); // ET_EXEC
    put16(&mut data, 18, 8); // EM_MIPS
    put32(&mut data, 20, 1);
    put32(&mut data, 24, 0x0010_0008); // entry
    put32(&mut data, 28, 52); // phoff
    put16(&mut data, 42, 32); // phentsize
    put16(&mut data, 44, 1); // phnum
    for (index, value) in [1u32, 96, 0x0010_0000, 0, 32, 32, 5, 16].into_iter().enumerate() {
        put32(&mut data, 52 + index * 4, value); // PT_LOAD, offset, vaddr, paddr, filesz, memsz, flags, align
    }
    data
}

fn one_function(start: u32, end: u32) -> Vec<(String, u32, u32)> {
    vec![("f".to_owned(), start, end)]
}

#[test]
fn a_valid_layout_does_not_claim_compatibility() {
    let report = inspect_bytes(&tiny_elf(), &one_function(0x0010_0008, 0x0010_0010)).unwrap();
    assert!(!report.known_edition(), "a made-up image is not the retail edition (CRC {:08X})", EXPECTED_CRC);
    assert_eq!(report.entry, 0x0010_0008);
    assert_eq!(report.mapped_functions, 1);
    let json = report.to_json();
    assert!(json.contains("\"layout_valid\": true") && json.contains("\"compatibility_verified\": false"));
    assert!(json.contains("\"known_edition\": false"));
}

#[test]
fn a_truncated_segment_is_refused() {
    let mut data = tiny_elf();
    data.truncate(data.len() - 8);
    let error = inspect_bytes(&data, &one_function(0x0010_0008, 0x0010_0010)).unwrap_err();
    assert_eq!(error, "Truncated or inconsistent segment");
}

#[test]
fn a_function_outside_the_segments_is_refused() {
    let error = inspect_bytes(&tiny_elf(), &one_function(0x0020_0000, 0x0020_0004)).unwrap_err();
    assert!(error.contains("outside the ELF"), "{error}");
}

#[test]
fn another_architecture_is_refused() {
    let mut data = tiny_elf();
    data[18..20].copy_from_slice(&62u16.to_le_bytes());
    let error = inspect_bytes(&data, &one_function(0x0010_0008, 0x0010_0010)).unwrap_err();
    assert!(error.contains("architecture"), "{error}");
    assert!(inspect_bytes(b"not an elf", &[]).unwrap_err().contains("ELF32"));
}

// ---- iso-extract ------------------------------------------------------------------------------------------------------

/// A one-file ISO9660 image: the volume descriptor at sector 16, the root directory at sector 18 and `HELLO.TXT;1` at 19.
fn tiny_iso() -> Vec<u8> {
    const SECTOR: usize = 2048;
    let mut image = vec![0u8; 22 * SECTOR];
    let record = |lba: u32, length: u32, flags: u8, name: &[u8]| -> Vec<u8> {
        let mut r = vec![0u8; 33 + name.len() + (name.len() + 1) % 2];
        r[0] = r.len() as u8;
        r[2..6].copy_from_slice(&lba.to_le_bytes());
        r[10..14].copy_from_slice(&length.to_le_bytes());
        r[25] = flags;
        r[32] = name.len() as u8;
        r[33..33 + name.len()].copy_from_slice(name);
        r
    };
    let pvd = &mut image[16 * SECTOR..17 * SECTOR];
    pvd[0] = 1;
    pvd[1..6].copy_from_slice(b"CD001");
    let root = record(18, SECTOR as u32, 2, &[0]);
    pvd[156..156 + root.len()].copy_from_slice(&root);
    pvd[80..84].copy_from_slice(&22u32.to_le_bytes()); // volume space size

    let dir = &mut image[18 * SECTOR..19 * SECTOR];
    let mut offset = 0;
    for entry in [record(18, 2048, 2, &[0]), record(18, 2048, 2, &[1]), record(19, 5, 0, b"HELLO.TXT;1")] {
        dir[offset..offset + entry.len()].copy_from_slice(&entry);
        offset += entry.len();
    }
    image[19 * SECTOR..19 * SECTOR + 5].copy_from_slice(b"hello");
    image[20 * SECTOR + 3] = 9; // a non-zero sector that belongs to no file: it goes to .disc_meta
    image
}

#[test]
fn the_iso_lists_extracts_and_records_its_layout() {
    let directory = scratch("iso");
    let iso_path = directory.join("tiny.iso");
    fs::write(&iso_path, tiny_iso()).unwrap();

    let (files, layer1) = iso::scan(&mut fs::File::open(&iso_path).unwrap()).unwrap();
    assert_eq!(files, vec![iso::DiscFile { name: "HELLO.TXT".into(), lba: 19, size: 5 }]);
    assert_eq!(layer1, 0);

    let output = directory.join("out");
    fs::create_dir_all(&output).unwrap();
    iso::run(&iso::Options { iso: iso_path, output: Some(output.clone()), layout: true, ..Default::default() })
        .unwrap();
    assert_eq!(fs::read(output.join("HELLO.TXT")).unwrap(), b"hello");
    assert_eq!(fs::read_to_string(output.join(".disc_layout")).unwrap(), "sectors 22\nlayer1 0\nfile 19 5 HELLO.TXT\n");
    // Sectors 16 (descriptor), 18 (directory) and 20 (the stray one) are not part of a file.
    let meta = fs::read(output.join(".disc_meta")).unwrap();
    let sectors: Vec<u32> = meta.chunks_exact(4 + 2048).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    assert_eq!(sectors, vec![16, 18, 20]);
    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn a_file_that_is_not_an_iso_is_refused() {
    let directory = scratch("noiso");
    let path = directory.join("x.iso");
    fs::write(&path, vec![0u8; 40 * 2048]).unwrap();
    assert!(iso::scan(&mut fs::File::open(&path).unwrap()).is_err());
    let _ = fs::remove_dir_all(&directory);
}

// ---- vu1-manifest -----------------------------------------------------------------------------------------------------

/// A pseudo-random "ELF" (the manifest tools only look at its bytes) and a manifest that rebuilds one image from it.
fn manifest_fixture() -> (Vec<u8>, String, Vec<u8>) {
    let mut state = 0x1234_5678u32;
    let elf: Vec<u8> = (0..40_000)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 24) as u8
        })
        .collect();
    // Two pieces: 64 bytes of the ELF at micro address 0, 32 bytes at 0x100.
    let pieces = [(0x400usize, 64usize, 0usize), (0x900, 32, 0x100)];
    let mut image = vec![0u8; vu1::SIZE];
    let mut text = format!("V 1\nE {} {}\n", elf.len(), blake2b_64_hex(&elf));
    for (offset, length, address) in pieces {
        image[address..address + length].copy_from_slice(&elf[offset..offset + length]);
        text.push_str(&format!("R {offset:x} {length} {}\n", blake2b_64_hex(&elf[offset..offset + length])));
    }
    text.push_str("I 0:0 100:1\n");
    (elf, text, image)
}

#[test]
fn the_manifest_rebuilds_the_image_under_its_game_name_and_verifies() {
    let (elf, manifest, image) = manifest_fixture();
    let directory = scratch("manifest");
    let (elf_path, manifest_path, output) =
        (directory.join("game.elf"), directory.join("manifest.txt"), directory.join("vu1"));
    fs::write(&elf_path, &elf).unwrap();
    fs::write(&manifest_path, &manifest).unwrap();

    assert_eq!(vu1::apply_manifest(&elf_path, &manifest_path, &output).unwrap(), 0);
    let produced = output.join(format!("vu1_{:016x}.bin", fnv(&image)));
    assert_eq!(fs::read(&produced).unwrap(), image);
    // Applying again changes nothing; verifying against the same folder is a 100 % match.
    assert_eq!(vu1::apply_manifest(&elf_path, &manifest_path, &output).unwrap(), 0);
    assert_eq!(fs::read_dir(&output).unwrap().count(), 1);
    assert_eq!(vu1::verify_manifest(&elf_path, &manifest_path, &output).unwrap(), 0);
    // The manifest holds no code: none of the image's bytes are in it.
    assert!(!manifest.contains(&image.iter().map(|b| format!("{b:02x}")).collect::<String>()));
    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn a_foreign_elf_writes_nothing() {
    let (mut elf, manifest, _) = manifest_fixture();
    for byte in &mut elf[0x400..0x440] {
        *byte ^= 0xFF;
    }
    let directory = scratch("foreign");
    let (elf_path, manifest_path, output) =
        (directory.join("game.elf"), directory.join("manifest.txt"), directory.join("vu1"));
    fs::write(&elf_path, &elf).unwrap();
    fs::write(&manifest_path, &manifest).unwrap();
    assert_eq!(vu1::apply_manifest(&elf_path, &manifest_path, &output).unwrap(), 1);
    assert_eq!(fs::read_dir(&output).unwrap().count(), 0);
    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn an_unsupported_manifest_version_is_an_error() {
    let directory = scratch("version");
    let (elf_path, manifest_path) = (directory.join("game.elf"), directory.join("manifest.txt"));
    fs::write(&elf_path, [0u8; 64]).unwrap();
    fs::write(&manifest_path, "V 9\n").unwrap();
    assert!(vu1::apply_manifest(&elf_path, &manifest_path, &directory.join("out"))
        .unwrap_err()
        .contains("unsupported version"));
    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn extending_a_manifest_records_only_elf_locations() {
    let directory = scratch("manifest-extend");
    let (elf_path, manifest_path, captures, output) = (
        directory.join("game.elf"),
        directory.join("manifest.txt"),
        directory.join("captures"),
        directory.join("extended.txt"),
    );
    let mut elf = vec![0u8; 64];
    elf[0x10..0x20].copy_from_slice(b"abcdefghijklmnop");
    elf[0x20..].fill(0xff);
    let mut capture = vec![0u8; vu1::SIZE];
    capture[..16].copy_from_slice(&elf[0x10..0x20]);
    fs::create_dir_all(&captures).unwrap();
    fs::write(&elf_path, &elf).unwrap();
    fs::write(&manifest_path, format!("V 1\nE {} {}\n", elf.len(), blake2b_64_hex(&elf))).unwrap();
    fs::write(captures.join("capture.bin"), &capture).unwrap();

    assert_eq!(vu1::extend_manifest(&elf_path, &manifest_path, &captures, &output).unwrap(), 0);
    let text = fs::read_to_string(&output).unwrap();
    assert!(text.contains("R 10 16 "), "{text}");
    assert!(text.contains("I 0:0"), "{text}");
    assert_eq!(vu1::verify_manifest(&elf_path, &output, &captures).unwrap(), 0);
    // The output is a reconstruction recipe, not the captured byte sequence.
    assert!(!text.contains("6162636465666768696a6b6c6d6e6f70"));
    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn extending_a_manifest_compacts_duplicate_pieces() {
    let directory = scratch("manifest-compact");
    let (elf_path, manifest_path, captures, output) = (
        directory.join("game.elf"),
        directory.join("manifest.txt"),
        directory.join("captures"),
        directory.join("compact.txt"),
    );
    let mut elf = vec![0u8; 64];
    elf[0x10..0x20].copy_from_slice(b"abcdefghijklmnop");
    let mut capture = vec![0u8; vu1::SIZE];
    capture[..16].copy_from_slice(&elf[0x10..0x20]);
    let fingerprint = blake2b_64_hex(&elf[0x10..0x20]);
    fs::create_dir_all(&captures).unwrap();
    fs::write(&elf_path, &elf).unwrap();
    fs::write(
        &manifest_path,
        format!("V 1\nE {} {}\nR 10 16 {fingerprint}\nR 10 16 {fingerprint}\nI 0:1\n", elf.len(), blake2b_64_hex(&elf)),
    )
    .unwrap();
    fs::write(captures.join("capture.bin"), &capture).unwrap();

    assert_eq!(vu1::extend_manifest(&elf_path, &manifest_path, &captures, &output).unwrap(), 0);
    let text = fs::read_to_string(&output).unwrap();
    assert_eq!(text.lines().filter(|line| line.starts_with("R ")).count(), 1, "{text}");
    assert!(text.contains("I 0:0"), "{text}");
    assert_eq!(vu1::verify_manifest(&elf_path, &output, &captures).unwrap(), 0);
    let _ = fs::remove_dir_all(&directory);
}

// ---- vu1-programs -----------------------------------------------------------------------------------------------------

/// An ELF with `.DVP.ovlytab` declaring two programs, their `.DVP.overlay.*` sections and one PT_LOAD segment.
fn dvp_elf() -> (Vec<u8>, Vec<u8>) {
    let mut payload = vec![0u8; 0x400];
    for (index, byte) in payload.iter_mut().enumerate() {
        *byte = (index % 251) as u8 + 1; // never all zero
    }
    let base_vaddr = 0x0010_0000u32;
    let table_vaddr = base_vaddr + 0x400;
    // ovlytab entries: {name, EE address, micro address}.
    let mut table = Vec::new();
    for (ee, micro) in [(base_vaddr, 0u32), (base_vaddr + 0x100, 0x200)] {
        for value in [0u32, ee, micro] {
            table.extend_from_slice(&value.to_le_bytes());
        }
    }
    let mut shstr = vec![0u8];
    let mut name = |text: &str| {
        let at = shstr.len() as u32;
        shstr.extend_from_slice(text.as_bytes());
        shstr.push(0);
        at
    };
    let (n_tab, n_ov0, n_ov1, n_str) =
        (name(".DVP.ovlytab"), name(".DVP.overlay.0"), name(".DVP.overlay.1"), name(".shstrtab"));

    let mut file = vec![0u8; 52 + 32];
    let data_off = file.len() as u32;
    file.extend_from_slice(&payload);
    let table_off = file.len() as u32;
    file.extend_from_slice(&table);
    let shstr_off = file.len() as u32;
    file.extend_from_slice(&shstr);
    while file.len() % 4 != 0 {
        file.push(0);
    }
    let shoff = file.len() as u32;
    let header = |name: u32, offset: u32, size: u32| -> Vec<u8> {
        let mut h = Vec::new();
        for value in [name, 1, 0, 0, offset, size, 0, 0, 1, 0] {
            h.extend_from_slice(&value.to_le_bytes());
        }
        h
    };
    for h in [
        vec![0u8; 40],
        header(n_tab, table_off, table.len() as u32),
        header(n_ov0, data_off, 0x100),
        header(n_ov1, data_off + 0x100, 0x80),
        header(n_str, shstr_off, shstr.len() as u32),
    ] {
        file.extend_from_slice(&h);
    }
    file[0..4].copy_from_slice(b"\x7fELF");
    file[28..32].copy_from_slice(&52u32.to_le_bytes()); // phoff
    file[32..36].copy_from_slice(&shoff.to_le_bytes());
    file[42..44].copy_from_slice(&32u16.to_le_bytes()); // phentsize
    file[44..46].copy_from_slice(&1u16.to_le_bytes()); // phnum
    file[46..48].copy_from_slice(&40u16.to_le_bytes()); // shentsize
    file[48..50].copy_from_slice(&5u16.to_le_bytes()); // shnum
    file[50..52].copy_from_slice(&4u16.to_le_bytes()); // shstrndx
    for (index, value) in
        [1u32, data_off, base_vaddr, base_vaddr, 0x400 + table.len() as u32, 0x400 + table.len() as u32, 5, 4]
            .into_iter()
            .enumerate()
    {
        file[52 + index * 4..56 + index * 4].copy_from_slice(&value.to_le_bytes());
    }
    let _ = table_vaddr;
    (file, payload)
}

#[test]
fn the_declared_programs_come_from_the_overlay_table() {
    let (elf, _) = dvp_elf();
    let programs = vu1::declared_programs(&elf);
    assert_eq!(programs.len(), 2);
    assert_eq!((programs[0].0, programs[0].1, programs[0].3), (0, 0, 0x100));
    assert_eq!((programs[1].0, programs[1].1, programs[1].3), (1, 0x200, 0x80));
    assert_eq!(programs[1].2 - programs[0].2, 0x100, "program 1 starts 0x100 bytes after program 0 in the file");
    assert!(vu1::declared_programs(b"not an elf").is_empty());
}

#[test]
fn what_the_manifest_covers_is_removed_and_the_rest_is_written_once() {
    let (elf, payload) = dvp_elf();
    let directory = scratch("programs");
    let (elf_path, manifest_path, output) =
        (directory.join("game.elf"), directory.join("manifest.txt"), directory.join("vu1"));
    fs::write(&elf_path, &elf).unwrap();
    let programs = vu1::declared_programs(&elf);

    // Without covered pieces both programs come out, each at its micro-memory address.
    fs::write(&manifest_path, "V 1\n").unwrap();
    let images = vu1::static_images(&elf, &manifest_path).unwrap();
    assert_eq!(images.len(), 2);
    assert_eq!(&images[0].1[0..0x100], &payload[0..0x100]);
    assert_eq!(&images[1].1[0x200..0x280], &payload[0x100..0x180]);

    // Cover the first program entirely: only the second remains.
    fs::write(&manifest_path, format!("V 1\nR {:x} 256 0000000000000000\n", programs[0].2)).unwrap();
    let images = vu1::static_images(&elf, &manifest_path).unwrap();
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].0, 1);

    assert_eq!(vu1::apply_programs(&elf_path, &manifest_path, &output).unwrap(), 0);
    let name = format!("vu1s_{:016x}.bin", fnv(&images[0].1));
    assert_eq!(fs::read(output.join(&name)).unwrap(), images[0].1);
    assert_eq!(vu1::apply_programs(&elf_path, &manifest_path, &output).unwrap(), 0);
    assert_eq!(fs::read_dir(&output).unwrap().count(), 1, "idempotent");
    let _ = fs::remove_dir_all(&directory);
}

// ---- iop-gen ----------------------------------------------------------------------------------------------------------

#[test]
fn iop_gen_matches_the_python_tool_on_a_small_module() {
    let output = scratch("iopgen");
    iop_gen::run(&golden("iop"), &output, "").unwrap();
    for name in ["iopn_gold_1000.cpp", "iopn_registry.cpp"] {
        assert_eq!(normalized(&output.join(name)), normalized(&golden("iop/expected").join(name)), "{name}");
    }
    let _ = fs::remove_dir_all(&output);
}

fn modified(path: &Path) -> std::time::SystemTime {
    fs::metadata(path).unwrap_or_else(|e| panic!("{}: {e}", path.display())).modified().unwrap()
}

/// A pause longer than the file system's timestamp granularity, so that a rewrite is visible in `modified`.
fn let_timestamps_move() {
    std::thread::sleep(std::time::Duration::from_millis(150));
}

#[test]
fn write_if_changed_writes_only_what_differs() {
    let dir = scratch("writeifchanged");
    let file = dir.join("out.cpp");
    assert!(ps2_tools::write_if_changed(&file, "a\nb\n").unwrap(), "a new file is written");
    let first = modified(&file);
    let_timestamps_move();
    assert!(!ps2_tools::write_if_changed(&file, "a\nb\n").unwrap(), "the same text is not written again");
    assert_eq!(modified(&file), first);
    fs::write(&file, "a\r\nb\r\n").unwrap();
    let crlf = modified(&file);
    let_timestamps_move();
    assert!(!ps2_tools::write_if_changed(&file, "a\nb\n").unwrap(), "Windows line endings count as the same text");
    assert_eq!(modified(&file), crlf);
    assert!(ps2_tools::write_if_changed(&file, "a\nc\n").unwrap(), "different text is written");
    assert_eq!(fs::read_to_string(&file).unwrap(), "a\nc\n");
    fs::write(&file, [0xff, 0xfe, 0x00]).unwrap();
    assert!(ps2_tools::write_if_changed(&file, "a\nc\n").unwrap(), "a file that is not UTF-8 is replaced");
    assert!(
        ps2_tools::write_if_changed(&dir.join("missing").join("out.cpp"), "x").is_err(),
        "an unwritable path is an error"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn iop_gen_leaves_the_files_alone_when_nothing_changed() {
    let output = scratch("iopgen-twice");
    iop_gen::run(&golden("iop"), &output, "").unwrap();
    let names = ["iopn_gold_1000.cpp", "iopn_registry.cpp"];
    let first: Vec<_> = names.iter().map(|n| modified(&output.join(n))).collect();
    let_timestamps_move();
    iop_gen::run(&golden("iop"), &output, "").unwrap();
    for (name, before) in names.iter().zip(first) {
        assert_eq!(modified(&output.join(name)), before, "{name} was rewritten");
        assert_eq!(normalized(&output.join(name)), normalized(&golden("iop/expected").join(name)), "{name}");
    }
    let _ = fs::remove_dir_all(&output);
}

#[test]
fn iop_gen_rewrites_only_the_module_whose_code_changed() {
    let dump = scratch("iopgen-dump");
    let output = scratch("iopgen-changed");
    let code = fs::read(golden("iop/gold.bin")).unwrap();
    fs::write(dump.join("gold.bin"), &code).unwrap();
    fs::write(dump.join("other.bin"), &code).unwrap();
    // The listing carries the module's hash; it is left as it is, so the registry does not change either.
    fs::write(dump.join("modules.txt"), "gold 1000 58 1234\nother 2000 58 1234\n").unwrap();
    iop_gen::run(&dump, &output, "").unwrap();
    let gold = output.join("iopn_gold_1000.cpp");
    let other = output.join("iopn_other_2000.cpp");
    let registry = output.join("iopn_registry.cpp");
    let (gold_time, other_time, registry_time) = (modified(&gold), modified(&other), modified(&registry));
    let other_before = fs::read_to_string(&other).unwrap();

    // The first instruction becomes a different one (the lowest bit of its first byte flips).
    let mut changed = code.clone();
    changed[0] ^= 1;
    fs::write(dump.join("other.bin"), &changed).unwrap();
    let_timestamps_move();
    iop_gen::run(&dump, &output, "").unwrap();
    assert_ne!(fs::read_to_string(&other).unwrap(), other_before, "the changed module must generate other code");
    assert_ne!(modified(&other), other_time, "the changed module is rewritten");
    assert_eq!(modified(&gold), gold_time, "the unchanged module is not rewritten");
    assert_eq!(modified(&registry), registry_time, "the registry did not change");
    let _ = fs::remove_dir_all(&dump);
    let _ = fs::remove_dir_all(&output);
}

#[test]
fn iop_gen_reads_the_overrides_that_ship_with_it() {
    let overrides = iop_gen::parse_overrides(iop_gen::DEFAULT_OVERRIDES);
    assert!(overrides.get("libsd_irx").is_some_and(|offsets| offsets.contains(&0x456c)));
    assert!(iop_gen::parse_overrides("# comment only\n\nbad\n").is_empty());
}

// ---- vu1n-gen ---------------------------------------------------------------------------------------------------------

fn run_vu1n(output: &Path, inputs: Vec<PathBuf>, parts: usize) -> String {
    vu1n_gen::run(&vu1n_gen::Options { output: output.to_path_buf(), inputs, heads: None, parts }).unwrap()
}

#[test]
fn vu1n_gen_matches_the_python_tool_on_a_small_image() {
    let output = scratch("vu1ngen");
    let summary = run_vu1n(&output, vec![golden("vu1n/image.bin")], 1);
    assert!(summary.contains("3 generated"), "{summary}");
    assert_eq!(normalized(&output.join("vu1n_part0.cpp")), normalized(&golden("vu1n/expected/vu1n_part0.cpp")));
    let _ = fs::remove_dir_all(&output);
}

#[test]
fn the_same_code_at_another_position_is_the_same_block() {
    // One simple block (an FMAC add pair, then an E-bit pair) placed at two micro-memory addresses.
    let block: [(u32, u32); 2] =
        [(0x8000_033C, (0xF << 21) | (3 << 16) | (2 << 11) | (1 << 6) | 0x28), (0x8000_033C, (1 << 30) | 0x2FF)];
    let place = |positions: &[usize]| -> Vec<u8> {
        let mut image = vec![0u8; vu1n_gen_size()];
        for &position in positions {
            for (index, (lower, upper)) in block.iter().enumerate() {
                let at = position + index * 8;
                image[at..at + 4].copy_from_slice(&lower.to_le_bytes());
                image[at + 4..at + 8].copy_from_slice(&upper.to_le_bytes());
            }
        }
        image
    };
    let directory = scratch("reloc");
    fs::write(directory.join("once.bin"), place(&[0x100])).unwrap();
    fs::write(directory.join("twice.bin"), place(&[0x100, 0x2000])).unwrap();
    let once = run_vu1n(&directory.join("a"), vec![directory.join("once.bin")], 1);
    let twice = run_vu1n(&directory.join("b"), vec![directory.join("twice.bin")], 1);
    assert!(once.contains("1 distinct blocks") && twice.contains("1 distinct blocks"), "{once} / {twice}");

    let text = normalized(&directory.join("b/vu1n_part0.cpp"));
    assert!(!text.contains("pc=0x"), "relocated text has no absolute pcs");
    assert!(text.contains("n.pc +"), "pcs are relative to the block's entry");
    let _ = fs::remove_dir_all(&directory);
}

fn vu1n_gen_size() -> usize {
    vu1::SIZE
}
