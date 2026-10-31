//! RetroAchievements password. It never touches the settings file: it lives in the Windows Credential Manager under
//! the same target name the C++ launcher used, so existing credentials keep working. Linux has no store yet (the
//! former launcher did not either).

#[cfg(windows)]
mod imp {
    use std::ptr;

    use windows_sys::Win32::Foundation::{FALSE, TRUE};
    use windows_sys::Win32::Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
    };

    const TARGET: &str = "PC/GodOfWar/RetroAchievements";

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn save(user: &str, password: &str) -> bool {
        let user = user.trim();
        if user.is_empty() || password.is_empty() {
            return false;
        }
        let mut target = wide(TARGET);
        let mut user_wide = wide(user);
        let mut secret = password.as_bytes().to_vec();
        let credential = CREDENTIALW {
            Flags: 0,
            Type: CRED_TYPE_GENERIC,
            TargetName: target.as_mut_ptr(),
            Comment: ptr::null_mut(),
            LastWritten: windows_sys::Win32::Foundation::FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 },
            CredentialBlobSize: secret.len() as u32,
            CredentialBlob: secret.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            AttributeCount: 0,
            Attributes: ptr::null_mut(),
            TargetAlias: ptr::null_mut(),
            UserName: user_wide.as_mut_ptr(),
        };
        // SAFETY: `credential` points at buffers that outlive the call; CredWriteW copies them.
        let ok = unsafe { CredWriteW(&credential, 0) } != FALSE;
        secret.fill(0);
        ok
    }

    pub fn read(user: &str) -> Option<String> {
        let target = wide(TARGET);
        let mut credential: *mut CREDENTIALW = ptr::null_mut();
        // SAFETY: on success CredReadW stores a buffer we release with CredFree below.
        if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) } != TRUE || credential.is_null()
        {
            return None;
        }
        // SAFETY: `credential` is valid until CredFree; the name is NUL-terminated and the blob has its own length.
        let password = unsafe {
            let entry = &*credential;
            let mut stored = String::new();
            if !entry.UserName.is_null() {
                let mut length = 0;
                while *entry.UserName.add(length) != 0 {
                    length += 1;
                }
                stored = String::from_utf16_lossy(std::slice::from_raw_parts(entry.UserName, length));
            }
            let found = if stored.to_lowercase() == user.trim().to_lowercase()
                && !entry.CredentialBlob.is_null()
                && entry.CredentialBlobSize > 0
            {
                let blob = std::slice::from_raw_parts(entry.CredentialBlob, entry.CredentialBlobSize as usize);
                Some(String::from_utf8_lossy(blob).into_owned())
            } else {
                None
            };
            CredFree(credential as *const _);
            found
        };
        password
    }

    pub fn clear() -> bool {
        let target = wide(TARGET);
        // SAFETY: plain call with a NUL-terminated name.
        unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) != FALSE }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn save(_user: &str, _password: &str) -> bool {
        false
    }
    pub fn read(_user: &str) -> Option<String> {
        None
    }
    pub fn clear() -> bool {
        false
    }
}

pub fn save_retro_achievements(user: &str, password: &str) -> bool {
    imp::save(user, password)
}

/// The stored password, only if it belongs to `user` (case-insensitive).
pub fn retro_achievements_password(user: &str) -> Option<String> {
    imp::read(user).filter(|password| !password.is_empty())
}

pub fn has_retro_achievements(user: &str) -> bool {
    retro_achievements_password(user).is_some()
}

pub fn clear_retro_achievements() -> bool {
    imp::clear()
}
