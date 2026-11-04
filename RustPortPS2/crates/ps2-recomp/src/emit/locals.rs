//! GOW-Port (decision D2, `[codegen] register_locals`): a function body that keeps the guest GPRs in C++ locals.
//!
//! The body is generated with the classic macros, in chunks (a label or comment, one instruction, one branch with its
//! delay slot, a GIF DMA kick); then every chunk that only computes is rewritten to the locals macros of
//! `ps2xRuntime/include/ps2_cg.h`, and every chunk that hands ctx to other code (a call, a return, a syscall, a stub, an
//! exception, a helper taking ctx) stays classic between `PS2_FLUSH()` and `PS2_RELOAD()`. The loop checkpoint writes the
//! registers back only when the scheduler has to look (`PS2_CG_DUE()`). A function the rewrite cannot place is emitted
//! classic as a whole. Idea from gow-native-pc patch 08 and the Killzone port's patch 0017; `ctx->pc` stores are kept.

use std::collections::BTreeSet;
use std::fmt::Write as _;

/// What a piece of the classic body is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkKind {
    /// A label definition or a disassembly comment.
    Label,
    /// One non-branch instruction (its `ctx->pc` store and its statement).
    Instruction,
    /// A branch or jump with its delay slot (ControlFlowEmitter).
    Branch,
    /// The GIF DMA capture / kick replacement.
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub kind: ChunkKind,
    pub text: String,
}

/// The registers a body uses: low 64 bits, and the upper 64 bits for 128-bit operations.
#[derive(Debug, Default)]
struct Usage {
    used: BTreeSet<u32>,
    written: BTreeSet<u32>,
    high_used: BTreeSet<u32>,
    high_written: BTreeSet<u32>,
}

const GPR_READS: [&str; 5] = ["GPR_U32", "GPR_S32", "GPR_U64", "GPR_S64", "GPR_VEC"];
const GPR_WRITES: [&str; 6] =
    ["SET_GPR_U32", "SET_GPR_S32", "SET_GPR_ZE32", "SET_GPR_U64", "SET_GPR_S64", "SET_GPR_VEC"];
const CHECKPOINT: &str = "runtime->eeCheckpointDue()";
const LOCALS_CHECKPOINT: &str = "PS2_CG_DUE()";

fn is_ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// Occurrences of `word` in `text` that stand alone as an identifier.
fn word_positions<'a>(text: &'a str, word: &'a str) -> impl Iterator<Item = usize> + 'a {
    let bytes = text.as_bytes();
    text.match_indices(word).map(|(at, _)| at).filter(move |&at| {
        let before_ok = at == 0 || !is_ident(bytes[at - 1]);
        let end = at + word.len();
        let after_ok = end >= bytes.len() || !is_ident(bytes[end]);
        before_ok && after_ok
    })
}

/// The name of the call whose argument list holds the `ctx` at `at` (`GPR_U32`, `runtime->Load32`...), when `ctx` is its
/// first argument or follows `rdram,`; `None` when ctx is used another way.
fn call_taking_ctx(text: &str, at: usize) -> Option<String> {
    let bytes = text.as_bytes();
    let mut k = at;
    let skip_spaces_back = |k: &mut usize| {
        while *k > 0 && bytes[*k - 1].is_ascii_whitespace() {
            *k -= 1;
        }
    };
    skip_spaces_back(&mut k);
    if k > 0 && bytes[k - 1] == b',' {
        k -= 1;
        skip_spaces_back(&mut k);
        if k < 5 || &text[k - 5..k] != "rdram" {
            return None;
        }
        k -= 5;
        skip_spaces_back(&mut k);
    }
    if k == 0 || bytes[k - 1] != b'(' {
        return None;
    }
    k -= 1;
    skip_spaces_back(&mut k);
    let end = k;
    while k > 0 && (is_ident(bytes[k - 1]) || bytes[k - 1] == b'>' || bytes[k - 1] == b'-' || bytes[k - 1] == b':') {
        k -= 1;
    }
    (k < end).then(|| text[k..end].to_owned())
}

/// Does the statement hand ctx (its GPRs) to code outside this function? The classic GPR macros, the FPU accumulator and
/// the memory accessors use ctx only for themselves (the memory macros take it implicitly), so they do not count.
fn needs_sync(code: &str) -> bool {
    if code.contains("ctx->r[") || code.contains("ps2_syscalls::") || code.contains("ps2_stubs::") {
        return true;
    }
    for at in word_positions(code, "ctx") {
        let end = at + 3;
        if code[end..].starts_with("->") {
            continue; // a field (pc, delay slot, FPU, HI/LO...): not a GPR
        }
        match call_taking_ctx(code, at) {
            Some(name)
                if GPR_READS.contains(&name.as_str())
                    || GPR_WRITES.contains(&name.as_str())
                    || name == "FPU_SET_ACC"
                    || name.starts_with("runtime->Load")
                    || name.starts_with("runtime->Store") => {}
            _ => return true,
        }
    }
    for (at, _) in code.match_indices("runtime->") {
        let rest = &code[at + "runtime->".len()..];
        if !(rest.starts_with("Load")
            || rest.starts_with("Store")
            || rest.starts_with("memory()")
            || rest.starts_with("eeCheckpointDue()"))
        {
            return true;
        }
    }
    false
}

/// Rewrites the classic GPR macros to their locals forms and records the registers:
/// `GPR_U32(ctx, 4)` -> `L_GPR_U32(4)`, `SET_GPR_S32(ctx, 4, v)` -> `L_SET_GPR_S32(4, v)` (register 0: `PS2_DISCARD(v)`).
/// `None` when the text has a form it does not understand (the function then stays classic).
fn rewrite(code: &str, usage: &mut Usage) -> Option<String> {
    let bytes = code.as_bytes();
    let n = bytes.len();
    let mut out = String::with_capacity(code.len());
    let mut local = Usage::default();
    let mut i = 0usize;
    let skip = |k: &mut usize| {
        while *k < n && (bytes[*k] == b' ' || bytes[*k] == b'\t') {
            *k += 1;
        }
    };
    while i < n {
        if !(bytes[i].is_ascii_alphabetic() || bytes[i] == b'_') || (i > 0 && is_ident(bytes[i - 1])) {
            let ch = code[i..].chars().next()?;
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        let mut j = i;
        while j < n && is_ident(bytes[j]) {
            j += 1;
        }
        let ident = &code[i..j];
        let read = GPR_READS.contains(&ident);
        let write = GPR_WRITES.contains(&ident);
        if !read && !write {
            out.push_str(ident);
            i = j;
            continue;
        }
        let mut k = j;
        skip(&mut k);
        if k >= n || bytes[k] != b'(' {
            return None;
        }
        k += 1;
        skip(&mut k);
        if !code[k..].starts_with("ctx") {
            return None;
        }
        k += 3;
        skip(&mut k);
        if k >= n || bytes[k] != b',' {
            return None;
        }
        k += 1;
        skip(&mut k);
        let digits = k;
        while k < n && bytes[k].is_ascii_digit() {
            k += 1;
        }
        let reg: u32 = code[digits..k].parse().ok()?;
        if reg > 31 {
            return None;
        }
        skip(&mut k);
        let vector = ident.ends_with("VEC");
        if write {
            if k >= n || bytes[k] != b',' {
                return None;
            }
            k += 1;
            skip(&mut k);
            if reg == 0 {
                out.push_str("PS2_DISCARD(");
            } else {
                let _ = write!(out, "L_{ident}({reg}, ");
                local.used.insert(reg);
                local.written.insert(reg);
                if vector {
                    local.high_used.insert(reg);
                    local.high_written.insert(reg);
                }
            }
        } else {
            if k >= n || bytes[k] != b')' {
                return None;
            }
            let _ = write!(out, "L_{ident}({reg}");
            if reg != 0 {
                local.used.insert(reg);
                if vector {
                    local.high_used.insert(reg);
                }
            }
        }
        i = k;
    }
    usage.used.extend(local.used);
    usage.written.extend(local.written);
    usage.high_used.extend(local.high_used);
    usage.high_written.extend(local.high_written);
    Some(out)
}

/// Does `text` define a label (`label_...:` at the start of a line)?
fn defines_label(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with("label_") && {
            let name_end = line.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(line.len());
            line[name_end..].trim_start().starts_with(':')
        }
    })
}

/// `goto label_X;` -> `{ PS2_RELOAD(); goto label_X; }`: a jump out of a classic chunk reads the registers first.
fn reload_before_gotos(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 32);
    let mut rest = text;
    while let Some(at) = rest.find("goto label_") {
        let Some(semicolon) = rest[at..].find(';') else { break };
        out.push_str(&rest[..at]);
        let _ = write!(out, "{{ PS2_RELOAD(); {} }}", &rest[at..at + semicolon + 1]);
        rest = &rest[at + semicolon + 1..];
    }
    out.push_str(rest);
    out
}

fn count(text: &str, what: &str) -> usize {
    word_positions(text, what).count()
}

/// The locals body of a function: the register locals, the per-function flush/reload macros, `head` (the resume switch
/// and the entry `ctx->pc` store), the body and the closing write-back (with `tail`, the fall-through `ctx->pc` store).
/// `None` when the function has to stay classic.
pub fn build_body(chunks: &[Chunk], head: &str, tail: &str) -> Option<String> {
    let mut usage = Usage::default();
    let mut body = String::new();
    for chunk in chunks {
        if chunk.kind == ChunkKind::Label {
            body.push_str(&chunk.text);
            continue;
        }
        if !needs_sync(&chunk.text) {
            // the only return allowed outside a classic chunk is the loop checkpoint's
            let text = chunk.text.replace(CHECKPOINT, LOCALS_CHECKPOINT);
            if count(&text, "return") == text.matches(LOCALS_CHECKPOINT).count() {
                body.push_str(&rewrite(&text, &mut usage)?);
                continue;
            }
        }
        // Classic text on ctx. A label inside it could be entered with ctx out of date: such a function stays classic.
        if defines_label(&chunk.text) {
            return None;
        }
        body.push_str("    PS2_FLUSH();\n");
        body.push_str(&reload_before_gotos(&chunk.text));
        body.push_str("    PS2_RELOAD();\n");
    }

    let mut out = String::new();
    for &reg in &usage.used {
        let _ = writeln!(out, "    uint64_t gpr{reg} = ps2CgLo(ctx, {reg});");
        if usage.high_used.contains(&reg) {
            let _ = writeln!(out, "    uint64_t gprh{reg} = ps2CgHi(ctx, {reg});");
        }
    }
    out.push_str("    constexpr uint64_t gpr0 = 0; constexpr uint64_t gprh0 = 0; (void)gpr0; (void)gprh0;\n");
    let mut flush = String::new();
    for &reg in &usage.written {
        let _ = write!(flush, "ps2CgSetLo(ctx, {reg}, gpr{reg}), ");
        if usage.high_written.contains(&reg) {
            let _ = write!(flush, "ps2CgSetHi(ctx, {reg}, gprh{reg}), ");
        }
    }
    let mut reload = String::new();
    for &reg in &usage.used {
        let _ = write!(reload, "gpr{reg} = ps2CgLo(ctx, {reg}), ");
        if usage.high_used.contains(&reg) {
            let _ = write!(reload, "gprh{reg} = ps2CgHi(ctx, {reg}), ");
        }
    }
    let _ = writeln!(out, "#define PS2_FLUSH_E() ({flush}0)");
    let _ = writeln!(out, "#define PS2_RELOAD_E() ({reload}0)");
    out.push_str("#define PS2_FLUSH() ((void)PS2_FLUSH_E())\n");
    out.push_str("#define PS2_RELOAD() ((void)PS2_RELOAD_E())\n\n");
    out.push_str(head);
    out.push_str(&body);
    out.push_str("    PS2_FLUSH();\n");
    out.push_str(tail);
    out.push_str("#undef PS2_FLUSH\n#undef PS2_RELOAD\n#undef PS2_FLUSH_E\n#undef PS2_RELOAD_E\n");
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(kind: ChunkKind, text: &str) -> Chunk {
        Chunk { kind, text: text.to_owned() }
    }

    #[test]
    fn computing_statements_use_locals_and_calls_stay_classic() {
        let chunks = [
            chunk(
                ChunkKind::Instruction,
                "    ctx->pc = 0x100u;\n    SET_GPR_S32(ctx, 2, (int32_t)ADD32(GPR_U32(ctx, 4), 1));\n",
            ),
            chunk(ChunkKind::Label, "label_104:\n"),
            chunk(ChunkKind::Instruction, "    ctx->pc = 0x104u;\n    SET_GPR_S32(ctx, 0, READ32(GPR_U32(ctx, 2)));\n"),
            chunk(
                ChunkKind::Branch,
                "    sub_200(rdram, ctx, runtime); if (ctx->pc != 0x10Cu) { return; }\n    goto label_104;\n",
            ),
        ];
        let body = build_body(&chunks, "", "").expect("this body can use locals");
        assert!(body.contains("L_SET_GPR_S32(2, (int32_t)ADD32(L_GPR_U32(4), 1));"), "{body}");
        assert!(body.contains("PS2_DISCARD(READ32(L_GPR_U32(2)));"), "a write to $zero still evaluates: {body}");
        assert!(body.contains("    PS2_FLUSH();\n    sub_200(rdram, ctx, runtime);"), "{body}");
        assert!(body.contains("{ PS2_RELOAD(); goto label_104; }"), "{body}");
        assert!(
            body.contains("uint64_t gpr2 = ps2CgLo(ctx, 2);") && body.contains("uint64_t gpr4 = ps2CgLo(ctx, 4);"),
            "{body}"
        );
        assert!(
            body.contains("#define PS2_FLUSH_E() (ps2CgSetLo(ctx, 2, gpr2), 0)"),
            "only written registers go back: {body}"
        );
    }

    #[test]
    fn the_loop_checkpoint_writes_back_only_on_its_slow_path() {
        let branch = "    if (GPR_U32(ctx, 3) != 0) {\n        ctx->pc = 0x100u;\n        if (runtime->eeCheckpointDue()) {\n            return;\n        }\n        goto label_100;\n    }\n";
        let body = build_body(&[chunk(ChunkKind::Branch, branch)], "", "").unwrap();
        assert!(body.contains("if (PS2_CG_DUE()) {"), "{body}");
        assert!(body.contains("if (L_GPR_U32(3) != 0)"), "{body}");
        assert!(!body.contains("PS2_RELOAD(); goto"), "a computing branch keeps its plain gotos: {body}");
    }

    #[test]
    fn vector_registers_keep_both_halves() {
        let code = "    SET_GPR_VEC(ctx, 5, _mm_add_epi32(GPR_VEC(ctx, 6), GPR_VEC(ctx, 7)));\n";
        let body = build_body(&[chunk(ChunkKind::Instruction, code)], "", "").unwrap();
        assert!(body.contains("uint64_t gprh6 = ps2CgHi(ctx, 6);"), "{body}");
        assert!(body.contains("ps2CgSetHi(ctx, 5, gprh5)"), "{body}");
        assert!(!body.contains("ps2CgSetHi(ctx, 6"), "a register only read is not written back: {body}");
    }

    #[test]
    fn unknown_forms_and_labels_inside_classic_code_keep_the_function_classic() {
        // a register that is not a literal
        assert!(build_body(&[chunk(ChunkKind::Instruction, "    SET_GPR_U32(ctx, rd, 1);\n")], "", "").is_none());
        // a classic chunk defining a label
        let branch = "    sub_1(rdram, ctx, runtime);\nlabel_fallthrough_0x10:\n    return;\n";
        assert!(build_body(&[chunk(ChunkKind::Branch, branch)], "", "").is_none());
        // a helper taking ctx, the raw register file, syscalls and runtime calls are classic, not refused
        for code in
            ["    ps2PmulthExec(ctx, 1, 2, 3);\n", "    ctx->r[4] = x;\n", "    runtime->SignalException(ctx, 1);\n"]
        {
            let body = build_body(&[chunk(ChunkKind::Instruction, code)], "", "").unwrap();
            assert!(body.contains("PS2_FLUSH();\n") && body.contains(code), "{code}: {body}");
        }
    }

    #[test]
    fn memory_accessors_and_ctx_fields_do_not_need_a_write_back() {
        assert!(!needs_sync("    SET_GPR_U32(ctx, 2, READ32(GPR_U32(ctx, 4)));\n    ctx->in_delay_slot = true;\n"));
        assert!(!needs_sync("    x = runtime->Load32(rdram, ctx, a);\n"));
        assert!(needs_sync("    runtime->dispatchGuestBranch(rdram, ctx, 1u, 2u, 3u);\n"));
        assert!(needs_sync("    sub_00100000(rdram, ctx, runtime);\n"));
    }
}
