use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    XChaCha20Poly1305,
};
use rand::{rngs::SysRng, TryRng};

/// Magic version byte for the encrypted file format.
pub const ENCRYPTED_VERSION: u8 = 1;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 24;

/// Derives a 32-byte key from the PIN using Argon2id.
/// The salt must be unique per file (stored alongside the ciphertext).
fn derive_key(pin: &str, salt: &[u8]) -> Result<[u8; 32], String> {
    if salt.len() != SALT_LEN {
        return Err("invalid salt length".to_string());
    }

    // Reasonable parameters for a desktop app:
    // 64 MiB memory, 3 iterations, 1 parallelism.
    let params =
        Params::new(64 * 1024, 3, 1, Some(32)).map_err(|e| format!("argon2 params: {e}"))?;

    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

    let mut key = [0u8; 32];
    argon2
        .hash_password_into(pin.as_bytes(), salt, &mut key)
        .map_err(|e| format!("key derivation failed: {e}"))?;

    Ok(key)
}

/// Encrypts the plaintext using a key derived from the PIN.
/// Returns a versioned blob: [version][salt][nonce][ciphertext]
pub fn encrypt(plaintext: &[u8], pin: &str) -> Result<Vec<u8>, String> {
    let mut rng = SysRng;
    let mut salt = [0u8; SALT_LEN];
    rng.try_fill_bytes(&mut salt)
        .map_err(|e| format!("random salt generation failed: {e}"))?;

    let mut nonce = [0u8; NONCE_LEN];
    rng.try_fill_bytes(&mut nonce)
        .map_err(|e| format!("random nonce generation failed: {e}"))?;

    let key = derive_key(pin, &salt)?;
    let cipher = XChaCha20Poly1305::new((&key).into());

    let ciphertext = cipher
        .encrypt((&nonce).into(), plaintext)
        .map_err(|e| format!("encryption failed: {e}"))?;

    let mut out = Vec::with_capacity(1 + SALT_LEN + NONCE_LEN + ciphertext.len());
    out.push(ENCRYPTED_VERSION);
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Decrypts a versioned blob produced by `encrypt`.
/// Returns an error for wrong PIN or any tampering/corruption.
pub fn is_encrypted(data: &[u8]) -> bool {
    !data.is_empty() && data[0] == ENCRYPTED_VERSION
}

pub fn decrypt(encrypted: &[u8], pin: &str) -> Result<Vec<u8>, String> {
    if !is_encrypted(encrypted) {
        return Err("unsupported or corrupted data format".to_string());
    }

    let min_len = 1 + SALT_LEN + NONCE_LEN;
    if encrypted.len() < min_len {
        return Err("truncated encrypted data".to_string());
    }

    let salt = &encrypted[1..=SALT_LEN];
    let nonce: &[u8; NONCE_LEN] = encrypted[1 + SALT_LEN..1 + SALT_LEN + NONCE_LEN]
        .try_into()
        .map_err(|_| "invalid nonce length".to_string())?;
    let ciphertext = &encrypted[1 + SALT_LEN + NONCE_LEN..];

    let key = derive_key(pin, salt)?;
    let cipher = XChaCha20Poly1305::new((&key).into());

    cipher
        .decrypt(nonce.into(), ciphertext)
        .map_err(|_| "decryption failed — wrong PIN or data has been tampered with".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_encrypt_decrypt() {
        let data = b"important kid money data 12345";
        let pin = "1234";

        let encrypted = encrypt(data, pin).unwrap();
        let decrypted = decrypt(&encrypted, pin).unwrap();

        assert_eq!(decrypted, data);
    }

    #[test]
    fn wrong_pin_fails() {
        let data = b"secret";
        let encrypted = encrypt(data, "1234").unwrap();

        let result = decrypt(&encrypted, "9999");
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(msg.contains("wrong PIN") || msg.contains("tampered"));
    }

    #[test]
    fn tampered_data_fails() {
        let data = b"money";
        let mut encrypted = encrypt(data, "1234").unwrap();

        // flip a byte in the ciphertext
        if encrypted.len() > 30 {
            encrypted[30] ^= 0xff;
        }

        let result = decrypt(&encrypted, "1234");
        assert!(result.is_err());
    }

    #[test]
    fn invalid_version_fails() {
        let mut bad = vec![99u8]; // wrong version
        bad.extend_from_slice(&[0u8; SALT_LEN + NONCE_LEN]);
        bad.extend_from_slice(b"garbage");

        let result = decrypt(&bad, "1234");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unsupported"));
    }
}
