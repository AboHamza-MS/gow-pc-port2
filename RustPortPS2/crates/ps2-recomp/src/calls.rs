//! Runtime call names.
//!
//! Port of `PS2Recomp/ps2xRuntime/include/ps2_runtime_calls.h`: the names the
//! runtime implements as syscall or stub handlers, and the lookup that tolerates
//! one leading underscore on either side. The lists themselves are generated
//! from `ps2_call_list.h` (`ps2-dev gen-call-list`).

use crate::calls_generated::{STUB_NAMES, SYSCALL_NAMES};

fn find_exact(name: &str, entries: &'static [&'static str]) -> Option<&'static str> {
    entries.iter().copied().find(|entry| *entry == name)
}

fn resolve(name: &str, entries: &'static [&'static str]) -> Option<&'static str> {
    if name.is_empty() {
        return None;
    }
    if let Some(exact) = find_exact(name, entries) {
        return Some(exact);
    }
    if let Some(stripped) = name.strip_prefix('_') {
        return find_exact(stripped, entries);
    }
    entries.iter().copied().find(|entry| entry.len() == name.len() + 1 && entry.strip_prefix('_') == Some(name))
}

/// `ps2_runtime_calls::resolveSyscallName`.
pub fn resolve_syscall_name(name: &str) -> Option<&'static str> {
    resolve(name, SYSCALL_NAMES)
}

/// `ps2_runtime_calls::resolveStubName`.
pub fn resolve_stub_name(name: &str) -> Option<&'static str> {
    resolve(name, STUB_NAMES)
}

/// `ps2_runtime_calls::isStubName`.
pub fn is_stub_name(name: &str) -> bool {
    resolve_stub_name(name).is_some()
}

/// `StubTarget`: where a stub's handler lives in the runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StubTarget {
    Unknown,
    Syscall,
    Stub,
}

/// `PS2Recompiler::resolveStubTarget`.
pub fn resolve_stub_target(name: &str) -> StubTarget {
    if resolve_syscall_name(name).is_some() {
        StubTarget::Syscall
    } else if resolve_stub_name(name).is_some() {
        StubTarget::Stub
    } else {
        StubTarget::Unknown
    }
}
