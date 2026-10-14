//! Secrets kept in Windows Credential Manager (Control Panel > Credential
//! Manager > Windows Credentials), never in LightLine's own files. Each is a
//! generic credential under a target name; Windows encrypts it for the
//! signed-in user.

use std::io;
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::{ERROR_NOT_FOUND, GetLastError};
use windows_sys::Win32::Security::Credentials::{
    CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredFree, CredReadW,
    CredWriteW,
};

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// The secret stored under `target`, if any.
pub fn read(target: &str) -> Option<String> {
    let target = wide(target);
    let mut found: *mut CREDENTIALW = null_mut();
    unsafe {
        if CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut found) == 0 || found.is_null() {
            return None;
        }
        let credential = &*found;
        let bytes = if credential.CredentialBlob.is_null() {
            &[][..]
        } else {
            std::slice::from_raw_parts(
                credential.CredentialBlob,
                credential.CredentialBlobSize as usize,
            )
        };
        let secret = String::from_utf8(bytes.to_vec()).ok();
        CredFree(found.cast());
        secret.filter(|secret| !secret.is_empty())
    }
}

/// Stores `secret` under `target`, replacing what was there.
pub fn write(target: &str, user: &str, secret: &str) -> io::Result<()> {
    let target = wide(target);
    let user = wide(user);
    let mut blob = secret.as_bytes().to_vec();
    let credential = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: target.as_ptr() as *mut u16,
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        UserName: user.as_ptr() as *mut u16,
        ..unsafe { std::mem::zeroed() }
    };
    let written = unsafe { CredWriteW(&credential, 0) } != 0;
    blob.fill(0);
    if written {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Removes the secret under `target`. False when there was none.
pub fn delete(target: &str) -> io::Result<bool> {
    let target = wide(target);
    if unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) } != 0 {
        return Ok(true);
    }
    if unsafe { GetLastError() } == ERROR_NOT_FOUND {
        Ok(false)
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Uses the real store of whoever runs it, cleaning up after itself.
    #[test]
    #[ignore = "writes to Windows Credential Manager"]
    fn a_secret_is_written_read_and_deleted() {
        let target = format!("LightLine test credential {}", std::process::id());
        assert_eq!(read(&target), None);
        write(&target, "LightLine", "s3cret value").unwrap();
        assert_eq!(read(&target).as_deref(), Some("s3cret value"));
        write(&target, "LightLine", "replaced").unwrap();
        assert_eq!(read(&target).as_deref(), Some("replaced"));
        assert!(delete(&target).unwrap());
        assert_eq!(read(&target), None);
        assert!(!delete(&target).unwrap());
    }
}
