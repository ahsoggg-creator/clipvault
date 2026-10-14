//! XChaCha20-Poly1305 for every record plus the master key protected by Windows DPAPI.
//!
//! The key file (`vault.key`) holds the 32-byte master key wrapped with `CryptProtectData` for the current
//! Windows user: another user, another machine or a copy of the folder on a USB stick cannot unwrap it.

use std::path::Path;

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};

use crate::error::{Error, Result};

pub const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 24;

pub type MasterKey = [u8; KEY_LEN];

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).expect("the operating system random number generator is unavailable");
    b
}

pub struct Cipher(XChaCha20Poly1305);

impl Cipher {
    pub fn new(key: &MasterKey) -> Cipher {
        Cipher(XChaCha20Poly1305::new(&Key::from(*key)))
    }

    /// `nonce || ciphertext || tag`; `aad` is authenticated but not stored.
    pub fn seal(&self, plain: &[u8], aad: &[u8]) -> Vec<u8> {
        let nonce = random_bytes::<NONCE_LEN>();
        let mut out = nonce.to_vec();
        let ct = self
            .0
            .encrypt(&XNonce::from(nonce), Payload { msg: plain, aad })
            .expect("XChaCha20-Poly1305 encryption cannot fail for in-memory buffers");
        out.extend_from_slice(&ct);
        out
    }

    pub fn open(&self, sealed: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
        if sealed.len() < NONCE_LEN + 16 {
            return Err(Error::Corrupt);
        }
        let (nonce, ct) = sealed.split_at(NONCE_LEN);
        let nonce = XNonce::try_from(nonce).map_err(|_| Error::Corrupt)?;
        self.0
            .decrypt(&nonce, Payload { msg: ct, aad })
            .map_err(|_| Error::Corrupt)
    }
}

/// Wraps / unwraps bytes for the current user. On Windows this is DPAPI; elsewhere there is no protection
/// and the type exists only so the logic can be tested on any machine.
pub trait KeyWrap {
    fn wrap(&self, data: &[u8]) -> Result<Vec<u8>>;
    fn unwrap(&self, data: &[u8]) -> Result<Vec<u8>>;
}

/// Identity wrapping, for tests and in-memory histories only.
pub struct NoWrap;

impl KeyWrap for NoWrap {
    fn wrap(&self, data: &[u8]) -> Result<Vec<u8>> {
        Ok(data.to_vec())
    }
    fn unwrap(&self, data: &[u8]) -> Result<Vec<u8>> {
        Ok(data.to_vec())
    }
}

#[cfg(windows)]
pub struct Dpapi;

#[cfg(windows)]
const ENTROPY: &[u8] = b"Clipvault vault key v1";

#[cfg(windows)]
impl KeyWrap for Dpapi {
    fn wrap(&self, data: &[u8]) -> Result<Vec<u8>> {
        use windows_sys::Win32::Security::Cryptography::{CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData};
        dpapi_call(data, |inp, ent, out| {
            // SAFETY: the blobs reference live buffers; the description and prompt are null; `out` is written by the call.
            unsafe {
                CryptProtectData(
                    inp,
                    std::ptr::null(),
                    ent,
                    std::ptr::null(),
                    std::ptr::null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    out,
                )
            }
        })
    }

    fn unwrap(&self, data: &[u8]) -> Result<Vec<u8>> {
        use windows_sys::Win32::Security::Cryptography::{CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData};
        dpapi_call(data, |inp, ent, out| {
            // SAFETY: as in `wrap`.
            unsafe {
                CryptUnprotectData(
                    inp,
                    std::ptr::null_mut(),
                    ent,
                    std::ptr::null(),
                    std::ptr::null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    out,
                )
            }
        })
    }
}

#[cfg(windows)]
fn dpapi_call(
    data: &[u8],
    call: impl FnOnce(
        *const windows_sys::Win32::Security::Cryptography::CRYPT_INTEGER_BLOB,
        *const windows_sys::Win32::Security::Cryptography::CRYPT_INTEGER_BLOB,
        *mut windows_sys::Win32::Security::Cryptography::CRYPT_INTEGER_BLOB,
    ) -> i32,
) -> Result<Vec<u8>> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::CRYPT_INTEGER_BLOB;
    let inp = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let ent = CRYPT_INTEGER_BLOB {
        cbData: ENTROPY.len() as u32,
        pbData: ENTROPY.as_ptr() as *mut u8,
    };
    let mut out = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    if call(&inp, &ent, &mut out) == 0 {
        return Err(Error::Key(
            "DPAPI отказал (другой пользователь или повреждённый ключ)".into(),
        ));
    }
    // SAFETY: on success `out` points to `cbData` bytes allocated by the system; they are copied, then freed once.
    let v = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
    unsafe { LocalFree(out.pbData as _) };
    Ok(v)
}

/// The key protector used by the real program.
#[cfg(windows)]
pub fn default_wrap() -> Box<dyn KeyWrap> {
    Box::new(Dpapi)
}

#[cfg(not(windows))]
pub fn default_wrap() -> Box<dyn KeyWrap> {
    Box::new(NoWrap)
}

/// Reads the master key from `path`, or creates a fresh one. A key file that cannot be unwrapped is an
/// error, never silently replaced: that would orphan the existing history.
pub fn load_or_create_key(path: &Path, wrap: &dyn KeyWrap) -> Result<MasterKey> {
    if path.exists() {
        let blob = std::fs::read(path)?;
        let raw = wrap.unwrap(&blob)?;
        return raw.try_into().map_err(|_| Error::Key("файл ключа повреждён".into()));
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let key: MasterKey = random_bytes();
    let tmp = path.with_extension("key.tmp");
    std::fs::write(&tmp, wrap.wrap(&key)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_open_roundtrip_and_tamper() {
        let c = Cipher::new(&random_bytes());
        let sealed = c.seal(b"hello", b"aad");
        assert_eq!(c.open(&sealed, b"aad").unwrap(), b"hello");
        assert!(c.open(&sealed, b"other").is_err());
        let mut bad = sealed.clone();
        *bad.last_mut().unwrap() ^= 1;
        assert!(c.open(&bad, b"aad").is_err());
        assert_ne!(c.seal(b"hello", b"aad"), sealed, "nonce must be fresh every time");
    }

    #[test]
    fn key_file_is_stable() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("vault.key");
        let a = load_or_create_key(&p, &*default_wrap()).unwrap();
        let b = load_or_create_key(&p, &*default_wrap()).unwrap();
        assert_eq!(a, b);
        let raw = std::fs::read(&p).unwrap();
        #[cfg(windows)]
        assert!(
            !raw.windows(32).any(|w| w == a),
            "the key must not lie on disk in the clear"
        );
        let _ = raw;
    }
}
