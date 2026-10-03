//! Native secrets never cross the webview boundary after they are saved.
use zeroize::Zeroize;

pub fn gateway_credential_id(paths: &crate::core::AppPaths) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(paths.data.to_string_lossy().to_lowercase().as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    uuid::Uuid::from_bytes(bytes).to_string()
}
pub fn gateway_token(paths: &crate::core::AppPaths) -> Result<String, String> {
    let _lock = paths.lock()?;
    let id = gateway_credential_id(paths);
    if let Ok(token) = get_secret(&id) {
        return Ok(token);
    }
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    set_secret(&id, &token)?;
    crate::core::atomic_write(&paths.data.join("gateway-credential-id"), id.as_bytes())?;
    Ok(token)
}

pub fn credential_target(id: &str) -> Result<String, String> {
    crate::core::validate_id(id)?;
    Ok(format!("Vela/connection/{id}"))
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
pub fn set_secret(id: &str, secret: &str) -> Result<(), String> {
    use windows_sys::Win32::Security::Credentials::*;
    let target = wide(&credential_target(id)?);
    let username = wide("Vela");
    let mut bytes = secret.as_bytes().to_vec();
    if bytes.is_empty() || bytes.len() > 2560 {
        bytes.zeroize();
        return Err("Key 必须为 1–2560 字节。".into());
    }
    let mut credential: CREDENTIALW = unsafe { std::mem::zeroed() };
    credential.Type = CRED_TYPE_GENERIC;
    credential.TargetName = target.as_ptr() as *mut u16;
    credential.UserName = username.as_ptr() as *mut u16;
    credential.CredentialBlobSize = bytes.len() as u32;
    credential.CredentialBlob = bytes.as_mut_ptr();
    credential.Persist = CRED_PERSIST_LOCAL_MACHINE;
    let result = unsafe { CredWriteW(&credential, 0) };
    bytes.zeroize();
    if result == 0 {
        Err("Windows 凭据管理器无法保存 Key。".into())
    } else {
        Ok(())
    }
}

#[cfg(windows)]
pub fn get_secret(id: &str) -> Result<String, String> {
    use windows_sys::Win32::Security::Credentials::*;
    let target = wide(&credential_target(id)?);
    let mut credential = std::ptr::null_mut();
    if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) } == 0 {
        return Err("未找到此连接的 Windows 凭据，请重新填写 Key。".into());
    }
    let result = unsafe {
        if (*credential).CredentialBlobSize == 0 || (*credential).CredentialBlob.is_null() {
            CredFree(credential as *const _);
            return Err("保存的凭据为空，请重新填写 Key。".into());
        }
        let bytes = std::slice::from_raw_parts(
            (*credential).CredentialBlob,
            (*credential).CredentialBlobSize as usize,
        );
        let value = String::from_utf8(bytes.to_vec())
            .map_err(|_| "保存的凭据无法读取，请重新填写 Key。".to_string());
        std::ptr::write_bytes(
            (*credential).CredentialBlob,
            0,
            (*credential).CredentialBlobSize as usize,
        );
        CredFree(credential as *const _);
        value
    };
    result
}

#[cfg(windows)]
pub fn remove_secret(id: &str) -> Result<(), String> {
    use windows_sys::Win32::{Foundation::GetLastError, Security::Credentials::*};
    let target = wide(&credential_target(id)?);
    if unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) } == 0 {
        // ERROR_NOT_FOUND also means the desired final state has been reached.
        if unsafe { GetLastError() } != 1168 {
            return Err("无法删除 Windows 凭据。".into());
        }
    }
    Ok(())
}

#[cfg(windows)]
pub fn protect(bytes: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::{Foundation::LocalFree, Security::Cryptography::*};
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let mut output: CRYPT_INTEGER_BLOB = unsafe { std::mem::zeroed() };
    let label = wide("Vela configuration recovery");
    let ok = unsafe {
        CryptProtectData(
            &input,
            label.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 {
        return Err("无法使用当前 Windows 用户加密配置备份。".into());
    }
    let mut result = b"VLB1".to_vec();
    unsafe {
        result.extend_from_slice(std::slice::from_raw_parts(
            output.pbData,
            output.cbData as usize,
        ));
        LocalFree(output.pbData as *mut _);
    }
    Ok(result)
}

#[cfg(windows)]
pub fn unprotect(bytes: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::{Foundation::LocalFree, Security::Cryptography::*};
    let encrypted = bytes.strip_prefix(b"VLB1").ok_or("备份格式无法识别。")?;
    let input = CRYPT_INTEGER_BLOB {
        cbData: encrypted.len() as u32,
        pbData: encrypted.as_ptr() as *mut u8,
    };
    let mut output: CRYPT_INTEGER_BLOB = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        CryptUnprotectData(
            &input,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 {
        return Err("此备份无法解密。备份只能由创建它的 Windows 用户恢复。".into());
    }
    let result = unsafe {
        let value = if output.cbData == 0 {
            Vec::new()
        } else {
            std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec()
        };
        if output.cbData > 0 {
            std::ptr::write_bytes(output.pbData, 0, output.cbData as usize);
        }
        LocalFree(output.pbData as *mut _);
        value
    };
    Ok(result)
}

#[cfg(not(windows))]
pub fn set_secret(_: &str, _: &str) -> Result<(), String> {
    Err("此版本仅支持 Windows 凭据管理器。".into())
}
#[cfg(not(windows))]
pub fn get_secret(_: &str) -> Result<String, String> {
    Err("此版本仅支持 Windows 凭据管理器。".into())
}
#[cfg(not(windows))]
pub fn remove_secret(_: &str) -> Result<(), String> {
    Err("此版本仅支持 Windows 凭据管理器。".into())
}
#[cfg(not(windows))]
pub fn protect(_: &[u8]) -> Result<Vec<u8>, String> {
    Err("此版本仅支持 Windows DPAPI 加密。".into())
}
#[cfg(not(windows))]
pub fn unprotect(_: &[u8]) -> Result<Vec<u8>, String> {
    Err("此版本仅支持 Windows DPAPI 加密。".into())
}
