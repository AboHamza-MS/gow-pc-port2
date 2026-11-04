// GOW-Port: register-locals code generation (`[codegen] register_locals` in the recompiler configuration, decision D2; idea
// from gow-native-pc patch 08 and the Killzone port's patch 0017). A generated function keeps the guest GPRs it uses in C++
// locals (`gprN` low 64 bits, `gprhN` upper 64 bits when a 128-bit operation uses the register), so the host compiler can keep
// them in registers. Statements that hand ctx to other code stay on ctx between PS2_FLUSH() (locals -> ctx) and PS2_RELOAD();
// the function defines both, and PS2_FLUSH_E() / PS2_RELOAD_E() as expressions, for its own registers.
// The L_ macros match the classic GPR macros of ps2_runtime_macros.h bit for bit: 32-bit writes extend to 64 bits (sign or
// zero) and keep the upper half, SET_GPR_VEC writes all 128 bits. Only generated code includes this file, after ps2_runtime.h.
#ifndef PS2_CG_H
#define PS2_CG_H

#include <cstdint>
#include <cstring>
#include <smmintrin.h>

static inline uint64_t ps2CgLo(const R5900Context *context, int reg)
{
    uint64_t value;
    std::memcpy(&value, &context->r[reg], sizeof(value));
    return value;
}

static inline uint64_t ps2CgHi(const R5900Context *context, int reg)
{
    uint64_t value;
    std::memcpy(&value, reinterpret_cast<const unsigned char *>(&context->r[reg]) + 8, sizeof(value));
    return value;
}

static inline void ps2CgSetLo(R5900Context *context, int reg, uint64_t value)
{
    std::memcpy(&context->r[reg], &value, sizeof(value));
}

static inline void ps2CgSetHi(R5900Context *context, int reg, uint64_t value)
{
    std::memcpy(reinterpret_cast<unsigned char *>(&context->r[reg]) + 8, &value, sizeof(value));
}

#define L_GPR_U32(reg) static_cast<uint32_t>(gpr##reg)
#define L_GPR_S32(reg) static_cast<int32_t>(static_cast<uint32_t>(gpr##reg))
#define L_GPR_U64(reg) static_cast<uint64_t>(gpr##reg)
#define L_GPR_S64(reg) static_cast<int64_t>(gpr##reg)
#define L_GPR_VEC(reg) _mm_set_epi64x(static_cast<int64_t>(gprh##reg), static_cast<int64_t>(gpr##reg))

#define L_SET_GPR_U32(reg, val) (gpr##reg = static_cast<uint64_t>(static_cast<int64_t>(static_cast<int32_t>(val))))
#define L_SET_GPR_S32(reg, val) (gpr##reg = static_cast<uint64_t>(static_cast<int64_t>(static_cast<int32_t>(val))))
#define L_SET_GPR_ZE32(reg, val) (gpr##reg = static_cast<uint64_t>(static_cast<uint32_t>(val)))
#define L_SET_GPR_U64(reg, val) (gpr##reg = static_cast<uint64_t>(static_cast<int64_t>(val)))
#define L_SET_GPR_S64(reg, val) L_SET_GPR_U64(reg, val)
#define L_SET_GPR_VEC(reg, val)                                                  \
    do                                                                           \
    {                                                                            \
        const __m128i ps2CgVec = (val);                                          \
        gpr##reg = static_cast<uint64_t>(_mm_cvtsi128_si64(ps2CgVec));           \
        gprh##reg = static_cast<uint64_t>(_mm_extract_epi64(ps2CgVec, 1));       \
    } while (0)

// A write to $zero: the value is still evaluated (it can read memory), then dropped.
#define PS2_DISCARD(val) ((void)(val))

// The loop checkpoint: the quiet clock first, inline; only when the scheduler has to look are the registers written back,
// and read again when no checkpoint was due after all.
#define PS2_CG_DUE() \
    (!runtime->eeCheckpointClock().advanceQuiet(32u) && (PS2_FLUSH_E(), runtime->eeCheckpointDueSlow(32u) || (PS2_RELOAD_E(), false)))

#endif // PS2_CG_H
