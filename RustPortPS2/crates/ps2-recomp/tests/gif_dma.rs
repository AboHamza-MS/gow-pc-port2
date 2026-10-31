use std::collections::BTreeSet;

use ps2_r5900::{decode, Instruction};
use ps2_recomp::gif_dma::{
    gif_dma_kick_call, resolve_memory_access_hint, try_build_gif_dma_kick_plan, update_constant_registers,
    ConstantRegisterState,
};

fn decode_all(words: &[u32]) -> Vec<Instruction> {
    words.iter().enumerate().map(|(index, raw)| decode(0x0010_0000 + index as u32 * 4, *raw, false)).collect()
}

#[test]
fn constants_flow_through_lui_ori_and_addiu() {
    let mut constants = ConstantRegisterState::new();
    // lui t0, 0x1000 ; ori t0, t0, 0xE020 ; addiu t1, t0, -0x10
    for inst in decode_all(&[0x3C08_1000, 0x3508_E020, 0x2509_FFF0]) {
        update_constant_registers(&inst, &mut constants);
    }
    assert_eq!(constants.read(8), Some(0x1000_E020));
    assert_eq!(constants.read(9), Some(0x1000_E010));
    assert_eq!(constants.read(0), Some(0), "$zero is always known");
    assert_eq!(constants.read(10), None);
}

#[test]
fn a_load_forgets_its_destination() {
    let mut constants = ConstantRegisterState::new();
    for inst in decode_all(&[0x3C08_1000, 0x8D08_0000]) {
        update_constant_registers(&inst, &mut constants);
    }
    assert_eq!(constants.read(8), None);
}

#[test]
fn a_store_off_a_known_base_gets_an_address_hint() {
    let mut constants = ConstantRegisterState::new();
    let program = decode_all(&[0x3C08_1000, 0xAD09_E010]); // lui t0, 0x1000 ; sw t1, 0xE010(t0)
    update_constant_registers(&program[0], &mut constants);
    let hint = resolve_memory_access_hint(&program[1], &constants);
    assert!(hint.has_address);
    // 0xE010 is sign-extended: 0x1000_0000 - 0x1FF0.
    assert_eq!(hint.address, 0x0FFF_E010);
    assert!(!resolve_memory_access_hint(&program[0], &constants).has_address, "lui is not a memory access");
}

fn kick_program() -> Vec<u32> {
    let targets = [0xE020u32, 0xE010, 0xA030, 0xA000];
    let mut words = vec![0x3C08_1000]; // lui t0, 0x1000
    for (index, target) in targets.iter().enumerate() {
        words.push(0x3500_0000 | (8 << 21) | (9 << 16) | target); // ori t1, t0, target
        words.push(0xAC00_0000 | (9 << 21) | ((10 + index as u32) << 16)); // sw (t2+n), 0(t1)
    }
    words.push(0x03E0_0008);
    words.push(0);
    words
}

#[test]
fn four_stores_to_the_dma_registers_become_one_kick() {
    let instructions = decode_all(&kick_program());
    let plan = try_build_gif_dma_kick_plan(&instructions, 0, &ConstantRegisterState::new(), &BTreeSet::new());
    assert!(plan.valid);
    assert_eq!(plan.store_indices.to_vec(), vec![2, 4, 6, 8]);
    assert!(!plan.completes_in_delay_slot);
    assert_eq!(plan.end_index, 8);
    assert!(plan.suppresses(2) && plan.suppresses(8) && !plan.suppresses(3));
    assert!(plan.completes_at(8));
    assert_eq!(
        gif_dma_kick_call(&plan),
        "runtime->kickGifDmaChainFromMMIO(rdram, ctx, gifDmaKickValue_100008_0, gifDmaKickValue_100010_1, \
         gifDmaKickValue_100018_2, gifDmaKickValue_100020_3);"
    );
    assert!(plan.captures.iter().all(|captured| *captured), "$t2.. are unknown, so each value is captured");
}

#[test]
fn a_label_inside_the_sequence_cancels_the_plan() {
    let instructions = decode_all(&kick_program());
    let labels: BTreeSet<u32> = [0x0010_0010].into_iter().collect();
    let plan = try_build_gif_dma_kick_plan(&instructions, 0, &ConstantRegisterState::new(), &labels);
    assert!(!plan.valid);
}

#[test]
fn a_store_to_another_address_is_not_a_kick() {
    let mut words = kick_program();
    words[2] = 0xAC00_0000 | (9 << 21) | (10 << 16) | 4; // sw t2, 4(t1): not D_PCR
    let plan = try_build_gif_dma_kick_plan(&decode_all(&words), 0, &ConstantRegisterState::new(), &BTreeSet::new());
    assert!(!plan.valid);
}
