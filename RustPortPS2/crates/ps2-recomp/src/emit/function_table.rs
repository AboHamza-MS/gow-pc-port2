//! The generated function registration file (`register_functions.cpp`).
//!
//! Port of `FunctionTableEmitter::emit` (`function_table_emitter.cpp`).

use std::collections::BTreeSet;
use std::fmt::Write as _;

use crate::calls::{resolve_stub_target, StubTarget};
use crate::generator::{CodeGenerator, GenerationError};
use crate::types::Function;

/// `CodeGenerator::generateFunctionRegistration`.
pub fn emit_function_table(cg: &CodeGenerator, functions: &[Function]) -> Result<String, GenerationError> {
    let mut entries: Vec<(u32, String)> = Vec::new();
    let mut registered_addresses: BTreeSet<u32> = BTreeSet::new();

    let mut add_entry = |address: u32, name: &str| -> Result<(), GenerationError> {
        if name.is_empty() {
            return Ok(());
        }
        if address & 3 != 0 {
            let message = format!("Unaligned function table entry for {name} at 0x{address:x}");
            if let Some(reporter) = cg.reporter() {
                reporter.error_at("function-table", name, address, &message);
            }
            return Err(GenerationError(message));
        }
        if !registered_addresses.insert(address) {
            return Ok(());
        }
        entries.push((address, name.to_owned()));
        Ok(())
    };

    let mut normal_functions: Vec<(u32, String)> = Vec::new();
    let mut stub_functions: Vec<(u32, String)> = Vec::new();
    let mut system_call_functions: Vec<(u32, String)> = Vec::new();
    let mut library_functions: Vec<(u32, String)> = Vec::new();

    for function in functions {
        if !function.is_recompiled && !function.is_stub && !function.is_skipped {
            continue;
        }

        let generated_name = cg.get_function_name(function.start);

        if function.is_skipped {
            library_functions.push((function.start, generated_name));
        } else if function.is_stub {
            if resolve_stub_target(&function.name) == StubTarget::Syscall {
                system_call_functions.push((function.start, generated_name));
            } else {
                stub_functions.push((function.start, generated_name));
            }
        } else {
            normal_functions.push((function.start, generated_name));
        }
    }

    let bootstrap = cg.bootstrap();
    if bootstrap.valid {
        let mut entry_target = bootstrap.entry_name.clone();
        if entry_target.is_empty() {
            entry_target = cg.get_function_name(bootstrap.entry);
        }
        if entry_target.is_empty() {
            return Err(GenerationError("No entry function name available for registration.".to_owned()));
        }
        add_entry(bootstrap.entry, &entry_target)?;
    }

    for (address, name) in &normal_functions {
        add_entry(*address, name)?;
    }

    for (owner_start, targets) in cg.resume_entry_targets_by_owner() {
        let owner_name = cg.get_function_name(*owner_start);
        if owner_name.is_empty() {
            continue;
        }
        for target in targets {
            add_entry(*target, &owner_name)?;
        }
    }

    for (address, name) in &stub_functions {
        add_entry(*address, name)?;
    }
    for (address, name) in &system_call_functions {
        add_entry(*address, name)?;
    }
    for (address, name) in &library_functions {
        add_entry(*address, name)?;
    }

    // Addresses are unique after the registration above, so the order is total.
    entries.sort_by_key(|(address, _)| *address);

    let mut table_base = 0u32;
    let mut table_end = 0u32;
    let mut slot_count = 0u32;
    if let (Some(first), Some(last)) = (entries.first(), entries.last()) {
        table_base = first.0 & !3;
        table_end = last.0.wrapping_add(4).wrapping_add(3) & !3;
        slot_count = table_end.wrapping_sub(table_base) >> 2;
    }

    let mut ss = String::new();
    ss.push_str("#include \"ps2_runtime.h\"\n");
    ss.push_str("#include <ps2_recompiled_functions.h>\n");
    ss.push_str("#include \"ps2_stubs.h\"\n");
    ss.push_str("#include <ps2_recompiled_stubs.h>\n");
    ss.push_str("#include \"ps2_syscalls.h\"\n\n");

    let _ = writeln!(ss, "extern const uint32_t g_ps2RecompiledFunctionTableBase = 0x{table_base:x}u;");
    let _ = writeln!(ss, "extern const uint32_t g_ps2RecompiledFunctionTableEnd = 0x{table_end:x}u;");
    let _ = writeln!(ss, "extern const uint32_t g_ps2RecompiledFunctionTableSlotCount = {slot_count}u;");
    let _ = write!(
        ss,
        "PS2Runtime::RecompiledFunction g_ps2RecompiledFunctionTable[{}u] = {{}};\n\n",
        if slot_count == 0 { 1 } else { slot_count }
    );

    ss.push_str("namespace {\n");
    ss.push_str("struct GeneratedFunctionTableInitializer {\n");
    ss.push_str("    GeneratedFunctionTableInitializer() {\n");
    for (address, name) in &entries {
        let slot = address.wrapping_sub(table_base) >> 2;
        let _ = writeln!(ss, "        g_ps2RecompiledFunctionTable[{slot}] = {name}; // 0x{address:x}");
    }
    ss.push_str("    }\n");
    ss.push_str("};\n");
    ss.push_str("static const GeneratedFunctionTableInitializer g_generatedFunctionTableInitializer;\n");
    ss.push_str("}\n");

    Ok(ss)
}
