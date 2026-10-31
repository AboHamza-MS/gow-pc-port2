//! `SPECIAL` opcodes.
//!
//! Port of `PS2Recomp/ps2xRecomp/src/lib/special_translator.cpp`. The text is
//! reproduced verbatim, including the C++ quirks: `movz`/`movn` go through
//! `SET_GPR_VEC`, `sll $zero, $rt, 0` is the only NOP, and `sll` with `rd == 0`
//! produces an empty string.

use ps2_r5900::opcodes::*;
use ps2_r5900::Instruction;

use crate::translate::instruction::FamilyTranslator;

/// Translate one `SPECIAL` instruction.
pub fn translate(hooks: &mut impl FamilyTranslator, inst: &Instruction) -> String {
    match inst.function {
        SPECIAL_SLL => {
            if inst.rd == 0 && inst.rt == 0 && inst.sa == 0 {
                return "// NOP".to_owned();
            }
            if inst.rd == 0 {
                return String::new();
            }
            format!("SET_GPR_S32(ctx, {}, (int32_t)SLL32(GPR_U32(ctx, {}), {}));", inst.rd, inst.rt, inst.sa)
        }
        SPECIAL_SRL => {
            format!("SET_GPR_S32(ctx, {}, (int32_t)SRL32(GPR_U32(ctx, {}), {}));", inst.rd, inst.rt, inst.sa)
        }
        SPECIAL_SRA => format!("SET_GPR_S32(ctx, {}, SRA32(GPR_S32(ctx, {}), {}));", inst.rd, inst.rt, inst.sa),
        SPECIAL_SLLV => format!(
            "SET_GPR_S32(ctx, {}, (int32_t)SLL32(GPR_U32(ctx, {}), GPR_U32(ctx, {}) & 0x1F));",
            inst.rd, inst.rt, inst.rs
        ),
        SPECIAL_SRLV => format!(
            "SET_GPR_S32(ctx, {}, (int32_t)SRL32(GPR_U32(ctx, {}), GPR_U32(ctx, {}) & 0x1F));",
            inst.rd, inst.rt, inst.rs
        ),
        SPECIAL_SRAV => format!(
            "SET_GPR_S32(ctx, {}, SRA32(GPR_S32(ctx, {}), GPR_U32(ctx, {}) & 0x1F));",
            inst.rd, inst.rt, inst.rs
        ),
        SPECIAL_JR => format!("// JR ${} - Handled by branch logic", inst.rs),
        SPECIAL_JALR => format!("// JALR ${}, ${} - Handled by branch logic", inst.rd, inst.rs),
        SPECIAL_SYSCALL => format!(
            "ctx->pc = 0x{:X}u;\nruntime->handleSyscall(rdram, ctx, 0x{:X}u);",
            inst.address.wrapping_add(4),
            (inst.raw >> 6) & 0xFFFFF
        ),
        SPECIAL_BREAK => "runtime->handleBreak(rdram, ctx);".to_owned(),
        SPECIAL_SYNC => {
            "// SYNC instruction - memory barrier\n// In recompiled code, we don't need explicit memory barriers"
                .to_owned()
        }
        SPECIAL_MFHI => format!("SET_GPR_U64(ctx, {}, ctx->hi);", inst.rd),
        SPECIAL_MTHI => format!("ctx->hi = GPR_U64(ctx, {});", inst.rs),
        SPECIAL_MFLO => format!("SET_GPR_U64(ctx, {}, ctx->lo);", inst.rd),
        SPECIAL_MTLO => format!("ctx->lo = GPR_U64(ctx, {});", inst.rs),
        SPECIAL_MULT => {
            if inst.rd != 0 {
                return format!(
                    "{{ int64_t result = (int64_t)GPR_S32(ctx, {}) * (int64_t)GPR_S32(ctx, {}); \
                     ctx->lo = (uint64_t)(int64_t)(int32_t)result; \
                     ctx->hi = (uint64_t)(int64_t)(int32_t)(result >> 32); \
                     SET_GPR_S32(ctx, {}, (int32_t)result); }}",
                    inst.rs, inst.rt, inst.rd
                );
            }
            format!(
                "{{ int64_t result = (int64_t)GPR_S32(ctx, {}) * (int64_t)GPR_S32(ctx, {}); \
                 ctx->lo = (uint64_t)(int64_t)(int32_t)result; \
                 ctx->hi = (uint64_t)(int64_t)(int32_t)(result >> 32); }}",
                inst.rs, inst.rt
            )
        }
        SPECIAL_MULTU => {
            if inst.rd != 0 {
                return format!(
                    "{{ uint64_t result = (uint64_t)GPR_U32(ctx, {}) * (uint64_t)GPR_U32(ctx, {}); \
                     ctx->lo = (uint64_t)(int64_t)(int32_t)result; \
                     ctx->hi = (uint64_t)(int64_t)(int32_t)(result >> 32); \
                     SET_GPR_S32(ctx, {}, (int32_t)result); }}",
                    inst.rs, inst.rt, inst.rd
                );
            }
            format!(
                "{{ uint64_t result = (uint64_t)GPR_U32(ctx, {}) * (uint64_t)GPR_U32(ctx, {}); \
                 ctx->lo = (uint64_t)(int64_t)(int32_t)result; \
                 ctx->hi = (uint64_t)(int64_t)(int32_t)(result >> 32); }}",
                inst.rs, inst.rt
            )
        }
        // The adjacent literals keep their leading spaces in the C++ (`"   int32_t dividend..."`);
        // a `\` continuation would strip them, so the pieces are joined with `concat!`.
        SPECIAL_DIV => format!(
            concat!(
                "{{ int32_t divisor = GPR_S32(ctx, {}); ",
                "   int32_t dividend = GPR_S32(ctx, {}); ",
                "   if (divisor != 0) {{ ",
                "       if (divisor == -1 && dividend == INT32_MIN) {{ ",
                "           ctx->lo = (uint64_t)(int64_t)INT32_MIN; ctx->hi = 0; ",
                "       }} else {{ ",
                "           ctx->lo = (uint64_t)(int64_t)(dividend / divisor); ",
                "           ctx->hi = (uint64_t)(int64_t)(dividend % divisor); ",
                "       }} ",
                "   }} else {{ ",
                "       ctx->lo = (dividend < 0) ? 1ull : 0xFFFFFFFFFFFFFFFFull; ctx->hi = (uint64_t)(int64_t)dividend; ",
                "   }} }}"
            ),
            inst.rt, inst.rs
        ),
        SPECIAL_DIVU => format!(
            "{{ uint32_t divisor = GPR_U32(ctx, {}); if (divisor != 0) {{ \
             ctx->lo = (uint64_t)(int64_t)(int32_t)(GPR_U32(ctx, {}) / divisor); \
             ctx->hi = (uint64_t)(int64_t)(int32_t)(GPR_U32(ctx, {}) % divisor); }} else {{ \
             ctx->lo = 0xFFFFFFFFFFFFFFFFull; ctx->hi = (uint64_t)(int64_t)(int32_t)GPR_U32(ctx,{}); }} }}",
            inst.rt, inst.rs, inst.rs, inst.rs
        ),
        SPECIAL_ADD => format!(
            concat!(
                "{{ ",
                "    int32_t rs_val = GPR_S32(ctx, {}); ",
                "    int32_t rt_val = GPR_S32(ctx, {}); ",
                "    int64_t result = (int64_t)rs_val + (int64_t)rt_val; ",
                "    if (result > INT32_MAX || result < INT32_MIN) {{ ",
                "        runtime->SignalException(ctx, EXCEPTION_INTEGER_OVERFLOW); ",
                "    }} else {{ ",
                "        SET_GPR_S32(ctx, {}, (int32_t)result); ",
                "    }} ",
                "}}"
            ),
            inst.rs, inst.rt, inst.rd
        ),
        SPECIAL_ADDU => format!(
            "SET_GPR_S32(ctx, {}, (int32_t)ADD32(GPR_U32(ctx, {}), GPR_U32(ctx, {})));",
            inst.rd, inst.rs, inst.rt
        ),
        SPECIAL_SUB => format!(
            "{{ uint32_t tmp; bool ov; \
             SUB32_OV(GPR_U32(ctx, {}), GPR_U32(ctx, {}), tmp, ov); \
             if (ov) runtime->SignalException(ctx, EXCEPTION_INTEGER_OVERFLOW); \
             else SET_GPR_S32(ctx, {}, (int32_t)tmp); }}",
            inst.rs, inst.rt, inst.rd
        ),
        SPECIAL_SUBU => format!(
            "SET_GPR_S32(ctx, {}, (int32_t)SUB32(GPR_U32(ctx, {}), GPR_U32(ctx, {})));",
            inst.rd, inst.rs, inst.rt
        ),
        SPECIAL_AND => format!("SET_GPR_U64(ctx, {}, GPR_U64(ctx, {}) & GPR_U64(ctx, {}));", inst.rd, inst.rs, inst.rt),
        SPECIAL_OR => format!("SET_GPR_U64(ctx, {}, GPR_U64(ctx, {}) | GPR_U64(ctx, {}));", inst.rd, inst.rs, inst.rt),
        SPECIAL_XOR => format!("SET_GPR_U64(ctx, {}, GPR_U64(ctx, {}) ^ GPR_U64(ctx, {}));", inst.rd, inst.rs, inst.rt),
        SPECIAL_NOR => {
            format!("SET_GPR_U64(ctx, {}, ~(GPR_U64(ctx, {}) | GPR_U64(ctx, {})));", inst.rd, inst.rs, inst.rt)
        }
        SPECIAL_SLT => format!(
            "SET_GPR_U64(ctx, {}, ((int64_t)GPR_S64(ctx, {}) < (int64_t)GPR_S64(ctx, {})) ? 1 : 0);",
            inst.rd, inst.rs, inst.rt
        ),
        SPECIAL_SLTU => format!(
            "SET_GPR_U64(ctx, {}, ((uint64_t)GPR_U64(ctx, {}) < (uint64_t)GPR_U64(ctx, {})) ? 1 : 0);",
            inst.rd, inst.rs, inst.rt
        ),
        // The C++ uses the vector setter here; kept for byte parity.
        SPECIAL_MOVZ => {
            format!("if (GPR_U64(ctx, {}) == 0) SET_GPR_VEC(ctx, {}, GPR_VEC(ctx, {}));", inst.rt, inst.rd, inst.rs)
        }
        SPECIAL_MOVN => {
            format!("if (GPR_U64(ctx, {}) != 0) SET_GPR_VEC(ctx, {}, GPR_VEC(ctx, {}));", inst.rt, inst.rd, inst.rs)
        }
        SPECIAL_MFSA => format!("SET_GPR_U32(ctx, {}, ctx->sa);", inst.rd),
        SPECIAL_MTSA => format!("ctx->sa = GPR_U32(ctx, {}) & 0x7F;", inst.rs),
        SPECIAL_DADD => format!(
            "{{ int64_t a = (int64_t)GPR_S64(ctx, {}); \
             int64_t b = (int64_t)GPR_S64(ctx, {}); \
             int64_t r = a + b; \
             if (((a ^ b) >= 0) && ((a ^ r) < 0)) runtime->SignalException(ctx, EXCEPTION_INTEGER_OVERFLOW); \
             else SET_GPR_S64(ctx, {}, r); }}",
            inst.rs, inst.rt, inst.rd
        ),
        SPECIAL_DADDU => format!(
            "SET_GPR_U64(ctx, {}, (uint64_t)GPR_U64(ctx, {}) + (uint64_t)GPR_U64(ctx, {}));",
            inst.rd, inst.rs, inst.rt
        ),
        SPECIAL_DSUB => format!(
            "{{ int64_t a = (int64_t)GPR_S64(ctx, {}); \
             int64_t b = (int64_t)GPR_S64(ctx, {}); \
             int64_t r = a - b; \
             if (((a ^ b) < 0) && ((a ^ r) < 0)) runtime->SignalException(ctx, EXCEPTION_INTEGER_OVERFLOW); \
             else SET_GPR_S64(ctx, {}, r); }}",
            inst.rs, inst.rt, inst.rd
        ),
        SPECIAL_DSUBU => {
            format!("SET_GPR_U64(ctx, {}, GPR_U64(ctx, {}) - GPR_U64(ctx, {}));", inst.rd, inst.rs, inst.rt)
        }
        SPECIAL_DSLL => format!("SET_GPR_U64(ctx, {}, GPR_U64(ctx, {}) << {});", inst.rd, inst.rt, inst.sa),
        SPECIAL_DSRL => format!("SET_GPR_U64(ctx, {}, GPR_U64(ctx, {}) >> {});", inst.rd, inst.rt, inst.sa),
        SPECIAL_DSRA => format!("SET_GPR_S64(ctx, {}, GPR_S64(ctx, {}) >> {});", inst.rd, inst.rt, inst.sa),
        SPECIAL_DSLLV => {
            format!("SET_GPR_U64(ctx, {}, GPR_U64(ctx, {}) << (GPR_U32(ctx, {}) & 0x3F));", inst.rd, inst.rt, inst.rs)
        }
        SPECIAL_DSRLV => {
            format!("SET_GPR_U64(ctx, {}, GPR_U64(ctx, {}) >> (GPR_U32(ctx, {}) & 0x3F));", inst.rd, inst.rt, inst.rs)
        }
        SPECIAL_DSRAV => {
            format!("SET_GPR_S64(ctx, {}, GPR_S64(ctx, {}) >> (GPR_U32(ctx, {}) & 0x3F));", inst.rd, inst.rt, inst.rs)
        }
        SPECIAL_DSLL32 => format!("SET_GPR_U64(ctx, {}, GPR_U64(ctx, {}) << (32 + {}));", inst.rd, inst.rt, inst.sa),
        SPECIAL_DSRL32 => format!("SET_GPR_U64(ctx, {}, GPR_U64(ctx, {}) >> (32 + {}));", inst.rd, inst.rt, inst.sa),
        SPECIAL_DSRA32 => format!("SET_GPR_S64(ctx, {}, GPR_S64(ctx, {}) >> (32 + {}));", inst.rd, inst.rt, inst.sa),
        SPECIAL_TGE => format!(
            "if (GPR_S64(ctx, {}) >= GPR_S64(ctx, {})) {{ runtime->handleTrap(rdram, ctx); }}",
            inst.rs, inst.rt
        ),
        SPECIAL_TGEU => format!(
            "if (GPR_U64(ctx, {}) >= GPR_U64(ctx, {})) {{ runtime->handleTrap(rdram, ctx); }}",
            inst.rs, inst.rt
        ),
        SPECIAL_TLT => {
            format!("if (GPR_S64(ctx, {}) < GPR_S64(ctx, {})) {{ runtime->handleTrap(rdram, ctx); }}", inst.rs, inst.rt)
        }
        SPECIAL_TLTU => {
            format!("if (GPR_U64(ctx, {}) < GPR_U64(ctx, {})) {{ runtime->handleTrap(rdram, ctx); }}", inst.rs, inst.rt)
        }
        SPECIAL_TEQ => format!(
            "if (GPR_U64(ctx, {}) == GPR_U64(ctx, {})) {{ runtime->handleTrap(rdram, ctx); }}",
            inst.rs, inst.rt
        ),
        SPECIAL_TNE => format!(
            "if (GPR_U64(ctx, {}) != GPR_U64(ctx, {})) {{ runtime->handleTrap(rdram, ctx); }}",
            inst.rs, inst.rt
        ),
        _ => hooks.emit_unhandled(inst, &format!("Unhandled SPECIAL instruction: 0x{:X}", inst.function)),
    }
}
