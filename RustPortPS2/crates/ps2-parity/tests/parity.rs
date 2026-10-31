use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use ps2_parity::{
    collect_tree, compare_files, counts_from_log, first_difference, sha256_hex, Counts, FileEntry, Manifest,
};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ps2-parity-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn sha256_matches_the_standard_vectors() {
    assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    assert_eq!(
        sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
    // Padding crosses a block boundary at 56 bytes.
    assert_eq!(sha256_hex(&[b'a'; 1000]), "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3");
}

#[test]
fn compare_reports_missing_extra_and_different_files() {
    let a = scratch("a");
    let b = scratch("b");
    fs::write(a.join("same.cpp"), "x\n").unwrap();
    fs::write(b.join("same.cpp"), "x\n").unwrap();
    fs::write(a.join("changed.cpp"), "one\ntwo\n").unwrap();
    fs::write(b.join("changed.cpp"), "one\n2\n").unwrap();
    fs::write(a.join("gone.cpp"), "g").unwrap();
    fs::create_dir_all(b.join("nested")).unwrap();
    fs::write(b.join("nested/extra.cpp"), "e").unwrap();

    let comparison = compare_files(&collect_tree(&a).unwrap(), &collect_tree(&b).unwrap());
    assert_eq!(comparison.common, 2);
    assert_eq!(comparison.only_in_a, vec!["gone.cpp"]);
    assert_eq!(comparison.only_in_b, vec!["nested/extra.cpp"]);
    assert_eq!(comparison.different, vec!["changed.cpp"]);
    assert!(!comparison.is_identical());
}

#[test]
fn first_difference_names_the_line() {
    assert_eq!(first_difference(b"a\nb\nc", b"a\nb\nc"), None);
    assert_eq!(first_difference(b"a\nb\n", b"a\nX\n"), Some((2, "b".into(), "X".into())));
    assert_eq!(first_difference(b"a\n", b"a\nb\n"), Some((2, "".into(), "b".into())));
}

#[test]
fn manifest_round_trips_through_json() {
    let mut files = BTreeMap::new();
    files.insert("func_00100008.cpp".to_owned(), FileEntry { size: 12034, sha256: sha256_hex(b"one") });
    files.insert("with \"quote\".cpp".to_owned(), FileEntry { size: 0, sha256: sha256_hex(b"") });
    let manifest = Manifest {
        generator: "ps2_recomp".into(),
        generator_version: "scus-97399-cpp-baseline".into(),
        operating_system: "windows".into(),
        elf_sha256: sha256_hex(b"elf"),
        files,
        counts: Counts { functions: 6414, stubs: 78, skips: 0, unresolved: 3 },
    };
    let text = manifest.to_json();
    assert_eq!(Manifest::from_json(&text).unwrap(), manifest);
}

#[test]
fn counts_come_from_the_report() {
    let log = "\n========== PS2Recomp report ==========\n\
        Functions discovered: 6414\n\
        Functions processed: 6414, recompiled: 6336, stubs: 78, skipped: 2, decode failures: 0\n\
        Generated functions: 6418\n\
        Indirect fallback promotions: 12 (3400 fallback entries)\n";
    assert_eq!(counts_from_log(log), Counts { functions: 6418, stubs: 78, skips: 2, unresolved: 12 });
}
