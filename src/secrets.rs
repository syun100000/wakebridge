use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use anyhow::{bail, Context, Result};
use rand::{rngs::OsRng, RngCore};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

#[derive(Clone)]
pub struct SecretStore {
    key: [u8; 32],
}

#[derive(Debug)]
pub struct EncryptedSecret {
    pub nonce: [u8; 12],
    pub ciphertext: Vec<u8>,
}

impl SecretStore {
    pub fn load_or_create(data_dir: &Path) -> Result<Self> {
        fs::create_dir_all(data_dir)
            .with_context(|| format!("create data directory {}", data_dir.display()))?;
        let path = data_dir.join(if cfg!(target_os = "macos") {
            "master.key"
        } else {
            "master.key.dpapi"
        });
        #[cfg(target_os = "macos")]
        let path = {
            let legacy = data_dir.join("master.key.dpapi");
            if !path.exists() && legacy.exists() {
                fs::rename(&legacy, &path)
                    .with_context(|| format!("migrate legacy master key {}", legacy.display()))?;
            }
            path
        };
        let key = if path.exists() {
            let protected = fs::read(&path)
                .with_context(|| format!("read protected master key {}", path.display()))?;
            #[cfg(target_os = "macos")]
            fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o600))
                .with_context(|| format!("protect master key permissions {}", path.display()))?;
            let raw = platform_unprotect(&protected).context("unprotect master key")?;
            if raw.len() != 32 {
                bail!("protected master key has an unexpected length");
            }
            let mut key = [0_u8; 32];
            key.copy_from_slice(&raw);
            key
        } else {
            let mut key = [0_u8; 32];
            OsRng.fill_bytes(&mut key);
            let protected = platform_protect(&key).context("protect master key")?;
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options
                .open(&path)
                .with_context(|| format!("create protected master key {}", path.display()))?;
            file.write_all(&protected)
                .context("write protected master key")?;
            file.flush().context("flush protected master key")?;
            key
        };
        Ok(Self { key })
    }

    pub fn encrypt(&self, plaintext: &str) -> Result<EncryptedSecret> {
        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|_| anyhow::anyhow!("initialize AES-GCM"))?;
        let mut nonce = [0_u8; 12];
        OsRng.fill_bytes(&mut nonce);
        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce), plaintext.as_bytes())
            .map_err(|_| anyhow::anyhow!("encrypt secret"))?;
        Ok(EncryptedSecret { nonce, ciphertext })
    }

    pub fn decrypt(&self, nonce: &[u8], ciphertext: &[u8]) -> Result<String> {
        if nonce.len() != 12 {
            bail!("secret nonce has an unexpected length");
        }
        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|_| anyhow::anyhow!("initialize AES-GCM"))?;
        let plaintext = cipher
            .decrypt(Nonce::from_slice(nonce), ciphertext)
            .map_err(|_| anyhow::anyhow!("decrypt secret"))?;
        String::from_utf8(plaintext).context("secret is not valid UTF-8")
    }
}

#[cfg(windows)]
fn platform_protect(data: &[u8]) -> Result<Vec<u8>> {
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_LOCAL_MACHINE, CRYPT_INTEGER_BLOB,
    };

    let mut input = data.to_vec();
    let input_blob = CRYPT_INTEGER_BLOB {
        cbData: input.len() as u32,
        pbData: input.as_mut_ptr(),
    };
    let mut output_blob = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: null_mut(),
    };
    let result = unsafe {
        CryptProtectData(
            &input_blob,
            std::ptr::null(),
            std::ptr::null(),
            null_mut(),
            null_mut(),
            CRYPTPROTECT_LOCAL_MACHINE,
            &mut output_blob,
        )
    };
    if result == 0 {
        return Err(std::io::Error::last_os_error()).context("CryptProtectData failed");
    }
    let protected = unsafe {
        std::slice::from_raw_parts(output_blob.pbData, output_blob.cbData as usize).to_vec()
    };
    unsafe {
        LocalFree(output_blob.pbData as *mut _);
    }
    Ok(protected)
}

#[cfg(windows)]
fn platform_unprotect(data: &[u8]) -> Result<Vec<u8>> {
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB};

    let mut input = data.to_vec();
    let input_blob = CRYPT_INTEGER_BLOB {
        cbData: input.len() as u32,
        pbData: input.as_mut_ptr(),
    };
    let mut output_blob = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: null_mut(),
    };
    let result = unsafe {
        CryptUnprotectData(
            &input_blob,
            std::ptr::null_mut(),
            std::ptr::null(),
            null_mut(),
            null_mut(),
            0,
            &mut output_blob,
        )
    };
    if result == 0 {
        return Err(std::io::Error::last_os_error()).context("CryptUnprotectData failed");
    }
    let plaintext = unsafe {
        std::slice::from_raw_parts(output_blob.pbData, output_blob.cbData as usize).to_vec()
    };
    unsafe {
        LocalFree(output_blob.pbData as *mut _);
    }
    Ok(plaintext)
}

#[cfg(target_os = "macos")]
fn platform_protect(data: &[u8]) -> Result<Vec<u8>> {
    Ok(data.to_vec())
}

#[cfg(target_os = "macos")]
fn platform_unprotect(data: &[u8]) -> Result<Vec<u8>> {
    Ok(data.to_vec())
}

#[cfg(all(not(windows), not(target_os = "macos")))]
fn platform_protect(data: &[u8]) -> Result<Vec<u8>> {
    Ok(data.to_vec())
}

#[cfg(all(not(windows), not(target_os = "macos")))]
fn platform_unprotect(data: &[u8]) -> Result<Vec<u8>> {
    Ok(data.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypts_and_decrypts_credentials() {
        let directory =
            std::env::temp_dir().join(format!("wakebridge-secret-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        let store = SecretStore::load_or_create(&directory).expect("secret store");
        let encrypted = store.encrypt("router-password").expect("encrypt");
        assert_ne!(encrypted.ciphertext, b"router-password");
        assert_eq!(
            store
                .decrypt(&encrypted.nonce, &encrypted.ciphertext)
                .expect("decrypt"),
            "router-password"
        );
        let _ = fs::remove_dir_all(directory);
    }
}
