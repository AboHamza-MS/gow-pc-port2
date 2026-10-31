//! Prints a summary of a `recomp.toml`, used to pin the parser's tests against
//! the real configuration in the repository.
//!
//! ```sh
//! cargo run -p ps2-config --example inspect -- ../../games/scus-97399/gow-port.toml
//! ```

use std::path::PathBuf;

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: inspect <recomp.toml>");
        std::process::exit(2);
    };
    match ps2_config::RecompilerConfig::load(&PathBuf::from(&path)) {
        Ok((config, report)) => {
            println!("input            = {}", config.input_path);
            println!("output           = {}", config.output_path);
            println!("ghidra_output    = {}", config.ghidra_map_path);
            println!("single_file      = {}", config.single_file_output);
            println!("low_memory_mode  = {}", config.low_memory_mode);
            println!("worker_threads   = {}", config.output_worker_threads);
            println!("patch_syscalls   = {}", config.patch_syscalls);
            println!("patch_cop0       = {}", config.patch_cop0);
            println!("patch_cache      = {}", config.patch_cache);
            println!("stubs            = {}", config.stub_implementations.len());
            println!("skip             = {}", config.skip_functions.len());
            println!("entry_points     = {}", config.entry_point_hints.len());
            println!("patches          = {}", config.patches.len());
            println!("mmio             = {}", config.mmio_by_instruction_address.len());
            println!("jump_tables      = {}", config.jump_tables.len());
            println!(
                "jump_table_entries = {}",
                config.jump_tables.iter().map(|table| table.entries.len()).sum::<usize>()
            );
            let bound = config.stub_implementations.iter().filter(|stub| stub.contains('@')).count();
            println!("stubs_with_address = {bound}");
            if let Some(first) = config.stub_implementations.first() {
                println!("first_stub       = {first}");
            }
            if let Some((address, value)) = config.patches.iter().next() {
                println!("first_patch      = 0x{address:x} = {value}");
            }
            for warning in &report.warnings {
                println!("warning: {warning}");
            }
        }
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
    }
}
