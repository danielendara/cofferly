use std::fs;
use std::io::ErrorKind;
use std::io::Write;
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

use crate::crypto::SessionCrypto;
use crate::data::{normalize_app_data, AppData, Wallet, DEFAULT_PARENT_PIN};
use crate::{
    AIRWALLET_LEGACY_APP_NAME, AIRWALLET_LEGACY_DATA_FILE_NAME, APP_NAME, ATLAS_LEGACY_APP_NAME,
    ATLAS_LEGACY_DATA_FILE_NAME, DATA_FILE_NAME, LEGACY_APP_NAME, LEGACY_DATA_FILE_NAME,
};

/// Result of loading (or migrating) on-disk app data before PIN unlock.
#[derive(Debug)]
pub struct LoadOutcome {
    pub data: Option<AppData>,
    /// Set when a legacy plaintext file was migrated into an encrypted Cofferly file.
    pub migrated_from_legacy: bool,
    /// Path of a retired legacy file (for status messaging), if any.
    pub retired_legacy_path: Option<PathBuf>,
}

pub fn data_path() -> PathBuf {
    app_data_base().join(APP_NAME).join(DATA_FILE_NAME)
}

fn atlas_legacy_data_path() -> PathBuf {
    app_data_base()
        .join(ATLAS_LEGACY_APP_NAME)
        .join(ATLAS_LEGACY_DATA_FILE_NAME)
}

fn atlas_generic_legacy_data_path() -> PathBuf {
    app_data_base()
        .join(ATLAS_LEGACY_APP_NAME)
        .join(DATA_FILE_NAME)
}

fn legacy_data_path() -> PathBuf {
    app_data_base()
        .join(LEGACY_APP_NAME)
        .join(LEGACY_DATA_FILE_NAME)
}

fn airwallet_legacy_data_path() -> PathBuf {
    app_data_base()
        .join(AIRWALLET_LEGACY_APP_NAME)
        .join(AIRWALLET_LEGACY_DATA_FILE_NAME)
}

fn app_data_base() -> PathBuf {
    dirs::data_local_dir()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn load_app_data_with_legacy(path: &PathBuf) -> Result<LoadOutcome, String> {
    load_app_data_with_paths(
        path,
        &atlas_generic_legacy_data_path(),
        &atlas_legacy_data_path(),
        &legacy_data_path(),
        &airwallet_legacy_data_path(),
    )
}

fn load_app_data_with_paths(
    path: &PathBuf,
    atlas_generic_legacy_path: &PathBuf,
    atlas_legacy_path: &PathBuf,
    legacy_path: &PathBuf,
    airwallet_legacy_path: &PathBuf,
) -> Result<LoadOutcome, String> {
    if path.exists() {
        // Plain JSON at the current path (manual copy / pre-encryption install).
        // Leave it in place until unlock re-encrypts — we do not have a confirmed
        // PIN yet if the caller only wanted a load probe; callers that migrate
        // use `migrate_plain_file_to_encrypted` after unlock.
        let data = load_app_data(path)?;
        return Ok(LoadOutcome {
            data: Some(data),
            migrated_from_legacy: false,
            retired_legacy_path: None,
        });
    }

    for legacy in [
        atlas_generic_legacy_path,
        atlas_legacy_path,
        legacy_path,
        airwallet_legacy_path,
    ] {
        let Some(data) = load_legacy_app_data(legacy) else {
            continue;
        };

        // Encrypt immediately with the PIN already stored in the legacy file so
        // the new Cofferly path never holds plaintext (including the parent PIN).
        migrate_legacy_to_encrypted(path, legacy, &data)?;

        return Ok(LoadOutcome {
            data: Some(data),
            migrated_from_legacy: true,
            retired_legacy_path: Some(legacy.clone()),
        });
    }

    Ok(LoadOutcome {
        data: None,
        migrated_from_legacy: false,
        retired_legacy_path: None,
    })
}

/// Write encrypted data to `new_path`, verify the write, then remove the legacy
/// plaintext file. Deletion only happens after the encrypted copy is confirmed.
fn migrate_legacy_to_encrypted(
    new_path: &Path,
    legacy_path: &Path,
    data: &AppData,
) -> Result<(), String> {
    let mut session = None;
    let encrypted_bytes =
        save_encrypted_with_session(new_path, data, &data.parent_pin, &mut session)?;

    // Read-back check: the atomic write succeeded and the file is encrypted.
    let on_disk = load_raw(&new_path.to_path_buf())?
        .ok_or_else(|| format!("Migrated data missing from {}", new_path.display()))?;
    if !crate::crypto::is_encrypted(&on_disk) {
        return Err(format!(
            "Migration wrote a non-encrypted file at {}",
            new_path.display()
        ));
    }
    if on_disk != encrypted_bytes {
        return Err("Migration verification failed: on-disk bytes differ from written blob".into());
    }

    // Confirm the PIN still opens the new file before deleting the only plaintext copy.
    crate::crypto::decrypt(&on_disk, &data.parent_pin).map_err(|err| {
        format!("Migration verification failed (could not decrypt new file): {err}")
    })?;

    if let Err(err) = fs::remove_file(legacy_path) {
        // Encrypted copy is good; report that cleanup failed so the parent can delete manually.
        return Err(format!(
            "Migrated and encrypted data, but could not remove legacy file {}: {err}",
            legacy_path.display()
        ));
    }

    Ok(())
}

fn load_legacy_app_data(path: &PathBuf) -> Option<AppData> {
    if !path.exists() {
        return None;
    }

    load_app_data(path).ok()
}

fn load_app_data(path: &PathBuf) -> Result<AppData, String> {
    let contents = fs::read_to_string(path)
        .map_err(|err| format!("Could not read {}: {err}", path.display()))?;

    if let Ok(data) = serde_json::from_str::<AppData>(&contents) {
        return normalize_app_data(data)
            .ok_or_else(|| format!("Saved data in {} is invalid", path.display()));
    }

    let wallets = serde_json::from_str::<Vec<Wallet>>(&contents)
        .map_err(|err| format!("Could not parse {}: {err}", path.display()))?;
    normalize_app_data(AppData {
        parent_pin: DEFAULT_PARENT_PIN.to_owned(),
        wallets,
    })
    .ok_or_else(|| format!("Saved data in {} is invalid", path.display()))
}

pub fn load_raw(path: &PathBuf) -> Result<Option<Vec<u8>>, String> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
        Err(err) => Err(format!("Could not read {}: {err}", path.display())),
    }
}

/// Encrypt and write `data`, returning the ciphertext so callers can cache it
/// without a redundant disk read.
pub fn save_encrypted(
    path: &PathBuf,
    data: &AppData,
    pin: &str,
    session: &mut Option<SessionCrypto>,
) -> Result<Vec<u8>, String> {
    save_encrypted_with_session(path, data, pin, session)
}

fn save_encrypted_with_session(
    path: &Path,
    data: &AppData,
    pin: &str,
    session: &mut Option<SessionCrypto>,
) -> Result<Vec<u8>, String> {
    let json = Zeroizing::new(
        serde_json::to_vec(data).map_err(|err| format!("Failed to serialize data: {err}"))?,
    );
    let encrypted = crate::crypto::encrypt(&json, pin, session)?;

    write_atomically(path, &encrypted)?;
    Ok(encrypted)
}

pub fn save_app_data(path: &PathBuf, data: &AppData) -> Result<(), String> {
    let contents = serde_json::to_string_pretty(data).map_err(|err| err.to_string())?;
    write_atomically(path, contents.as_bytes())
}

fn write_atomically(path: &Path, contents: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }

    let parent = path
        .parent()
        .ok_or_else(|| format!("Could not find parent folder for {}", path.display()))?;
    let mut temp_file = tempfile::NamedTempFile::new_in(parent).map_err(|err| err.to_string())?;
    temp_file
        .write_all(contents)
        .map_err(|err| err.to_string())?;
    temp_file
        .as_file_mut()
        .sync_all()
        .map_err(|err| err.to_string())?;
    temp_file
        .persist(path)
        .map_err(|err| err.error.to_string())?;

    Ok(())
}

/// Best-effort cleanup of previous print artifacts under the OS temp directory.
pub fn cleanup_temp_print_artifacts() {
    let temp = std::env::temp_dir();
    let Ok(entries) = fs::read_dir(&temp) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with("cofferly-") && name.ends_with(".html") {
            let _ = fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto;
    use crate::data::default_app_data;

    #[test]
    fn imports_legacy_data_encrypted_and_retires_plaintext() {
        let test_dir =
            std::env::temp_dir().join(format!("cofferly-migration-test-{}", std::process::id()));
        let new_path = test_dir.join(APP_NAME).join(DATA_FILE_NAME);
        let atlas_generic_legacy_path = test_dir.join(ATLAS_LEGACY_APP_NAME).join(DATA_FILE_NAME);
        let atlas_legacy_path = test_dir
            .join(ATLAS_LEGACY_APP_NAME)
            .join(ATLAS_LEGACY_DATA_FILE_NAME);
        let legacy_path = test_dir.join(LEGACY_APP_NAME).join(LEGACY_DATA_FILE_NAME);
        let airwallet_legacy_path = test_dir
            .join(AIRWALLET_LEGACY_APP_NAME)
            .join(AIRWALLET_LEGACY_DATA_FILE_NAME);
        let mut data = default_app_data();
        data.parent_pin = "5678".to_owned();
        data.wallets[0].child_name = "Migrated".to_owned();

        save_app_data(&legacy_path, &data).unwrap();
        assert!(legacy_path.exists());

        let loaded = load_app_data_with_paths(
            &new_path,
            &atlas_generic_legacy_path,
            &atlas_legacy_path,
            &legacy_path,
            &airwallet_legacy_path,
        )
        .unwrap();

        assert!(loaded.migrated_from_legacy);
        assert_eq!(
            loaded.data.as_ref().unwrap().wallets[0].child_name,
            "Migrated"
        );
        assert!(new_path.exists());
        assert!(!legacy_path.exists(), "legacy plaintext must be removed");

        let raw = fs::read(&new_path).unwrap();
        assert!(crypto::is_encrypted(&raw));
        // File must not be readable as JSON plaintext.
        assert!(serde_json::from_slice::<AppData>(&raw).is_err());
        let (plain, _) = crypto::decrypt(&raw, "5678").unwrap();
        let decrypted: AppData = serde_json::from_slice(&plain).unwrap();
        assert_eq!(decrypted.wallets[0].child_name, "Migrated");

        fs::remove_dir_all(test_dir).unwrap();
    }

    #[test]
    fn imports_atlas_generic_data_when_cofferly_data_does_not_exist() {
        let test_dir = std::env::temp_dir().join(format!(
            "cofferly-atlas-generic-data-migration-test-{}",
            std::process::id()
        ));
        let new_path = test_dir.join(APP_NAME).join(DATA_FILE_NAME);
        let atlas_generic_legacy_path = test_dir.join(ATLAS_LEGACY_APP_NAME).join(DATA_FILE_NAME);
        let atlas_legacy_path = test_dir
            .join(ATLAS_LEGACY_APP_NAME)
            .join(ATLAS_LEGACY_DATA_FILE_NAME);
        let legacy_path = test_dir.join(LEGACY_APP_NAME).join(LEGACY_DATA_FILE_NAME);
        let airwallet_legacy_path = test_dir
            .join(AIRWALLET_LEGACY_APP_NAME)
            .join(AIRWALLET_LEGACY_DATA_FILE_NAME);
        let data = default_app_data();

        save_app_data(&atlas_generic_legacy_path, &data).unwrap();

        let loaded = load_app_data_with_paths(
            &new_path,
            &atlas_generic_legacy_path,
            &atlas_legacy_path,
            &legacy_path,
            &airwallet_legacy_path,
        )
        .unwrap();

        assert!(loaded.migrated_from_legacy);
        assert_eq!(
            loaded.data.as_ref().unwrap().wallets.len(),
            data.wallets.len()
        );
        assert!(new_path.exists());
        assert!(!atlas_generic_legacy_path.exists());
        assert!(crypto::is_encrypted(&fs::read(&new_path).unwrap()));

        fs::remove_dir_all(test_dir).unwrap();
    }

    #[test]
    fn imports_atlas_named_data_when_cofferly_data_does_not_exist() {
        let test_dir = std::env::temp_dir().join(format!(
            "cofferly-atlas-named-data-migration-test-{}",
            std::process::id()
        ));
        let new_path = test_dir.join(APP_NAME).join(DATA_FILE_NAME);
        let atlas_generic_legacy_path = test_dir.join(ATLAS_LEGACY_APP_NAME).join(DATA_FILE_NAME);
        let atlas_legacy_path = test_dir
            .join(ATLAS_LEGACY_APP_NAME)
            .join(ATLAS_LEGACY_DATA_FILE_NAME);
        let legacy_path = test_dir.join(LEGACY_APP_NAME).join(LEGACY_DATA_FILE_NAME);
        let airwallet_legacy_path = test_dir
            .join(AIRWALLET_LEGACY_APP_NAME)
            .join(AIRWALLET_LEGACY_DATA_FILE_NAME);
        let data = default_app_data();

        save_app_data(&atlas_legacy_path, &data).unwrap();

        let loaded = load_app_data_with_paths(
            &new_path,
            &atlas_generic_legacy_path,
            &atlas_legacy_path,
            &legacy_path,
            &airwallet_legacy_path,
        )
        .unwrap();

        assert!(loaded.migrated_from_legacy);
        assert!(new_path.exists());
        assert!(!atlas_legacy_path.exists());

        fs::remove_dir_all(test_dir).unwrap();
    }

    #[test]
    fn imports_airwallet_data_when_newer_formats_do_not_exist() {
        let test_dir = std::env::temp_dir().join(format!(
            "cofferly-airwallet-migration-test-{}",
            std::process::id()
        ));
        let new_path = test_dir.join(APP_NAME).join(DATA_FILE_NAME);
        let atlas_generic_legacy_path = test_dir.join(ATLAS_LEGACY_APP_NAME).join(DATA_FILE_NAME);
        let atlas_legacy_path = test_dir
            .join(ATLAS_LEGACY_APP_NAME)
            .join(ATLAS_LEGACY_DATA_FILE_NAME);
        let legacy_path = test_dir.join(LEGACY_APP_NAME).join(LEGACY_DATA_FILE_NAME);
        let airwallet_legacy_path = test_dir
            .join(AIRWALLET_LEGACY_APP_NAME)
            .join(AIRWALLET_LEGACY_DATA_FILE_NAME);
        let mut data = default_app_data();
        data.wallets[0].child_name = "Imported child".to_owned();

        save_app_data(&airwallet_legacy_path, &data).unwrap();

        let loaded = load_app_data_with_paths(
            &new_path,
            &atlas_generic_legacy_path,
            &atlas_legacy_path,
            &legacy_path,
            &airwallet_legacy_path,
        )
        .unwrap();

        assert_eq!(
            loaded.data.as_ref().unwrap().wallets[0].child_name,
            "Imported child"
        );
        assert!(new_path.exists());
        assert!(!airwallet_legacy_path.exists());
        assert!(crypto::is_encrypted(&fs::read(&new_path).unwrap()));

        fs::remove_dir_all(test_dir).unwrap();
    }

    #[test]
    fn stores_current_data_in_generic_file_name() {
        assert_eq!(DATA_FILE_NAME, "data.json");
    }

    #[test]
    fn save_app_data_replaces_existing_file() {
        let test_dir =
            std::env::temp_dir().join(format!("cofferly-replace-save-test-{}", std::process::id()));
        let path = test_dir.join(APP_NAME).join(DATA_FILE_NAME);
        let mut data = default_app_data();

        save_app_data(&path, &data).unwrap();
        data.wallets[0].child_name = "Updated Child".to_owned();
        save_app_data(&path, &data).unwrap();

        let loaded = load_app_data(&path).unwrap();
        assert_eq!(loaded.wallets[0].child_name, "Updated Child");

        fs::remove_dir_all(test_dir).unwrap();
    }

    #[test]
    fn save_encrypted_replaces_existing_file() {
        let test_dir = std::env::temp_dir().join(format!(
            "cofferly-replace-encrypted-save-test-{}",
            std::process::id()
        ));
        let path = test_dir.join(APP_NAME).join(DATA_FILE_NAME);
        let mut data = default_app_data();
        let pin = "1234";
        let mut session = None;

        save_encrypted(&path, &data, pin, &mut session).unwrap();
        let first_raw = load_raw(&path).unwrap().unwrap();

        data.wallets[0].child_name = "Encrypted Child".to_owned();
        save_encrypted(&path, &data, pin, &mut session).unwrap();
        let second_raw = load_raw(&path).unwrap().unwrap();
        let (decrypted, _) = crate::crypto::decrypt(&second_raw, pin).unwrap();
        let loaded = serde_json::from_slice::<AppData>(&decrypted).unwrap();

        assert_ne!(first_raw, second_raw);
        assert_eq!(loaded.wallets[0].child_name, "Encrypted Child");
        // Second save should not need a new Argon2 wrap of a different key — same session.
        let header_len = 1 + 16 + 24 + 48;
        assert_eq!(&first_raw[..header_len], &second_raw[..header_len]);

        fs::remove_dir_all(test_dir).unwrap();
    }

    #[test]
    fn rejects_invalid_current_data_without_replacing_it_with_legacy_data() {
        let test_dir =
            std::env::temp_dir().join(format!("cofferly-invalid-data-test-{}", std::process::id()));
        let new_path = test_dir.join(APP_NAME).join(DATA_FILE_NAME);
        let atlas_generic_legacy_path = test_dir.join(ATLAS_LEGACY_APP_NAME).join(DATA_FILE_NAME);
        let atlas_legacy_path = test_dir
            .join(ATLAS_LEGACY_APP_NAME)
            .join(ATLAS_LEGACY_DATA_FILE_NAME);
        let legacy_path = test_dir.join(LEGACY_APP_NAME).join(LEGACY_DATA_FILE_NAME);
        let airwallet_legacy_path = test_dir
            .join(AIRWALLET_LEGACY_APP_NAME)
            .join(AIRWALLET_LEGACY_DATA_FILE_NAME);

        fs::create_dir_all(new_path.parent().unwrap()).unwrap();
        fs::write(&new_path, "invalid data").unwrap();
        save_app_data(&atlas_generic_legacy_path, &default_app_data()).unwrap();
        save_app_data(&atlas_legacy_path, &default_app_data()).unwrap();
        save_app_data(&legacy_path, &default_app_data()).unwrap();
        save_app_data(&airwallet_legacy_path, &default_app_data()).unwrap();

        assert!(load_app_data_with_paths(
            &new_path,
            &atlas_generic_legacy_path,
            &atlas_legacy_path,
            &legacy_path,
            &airwallet_legacy_path
        )
        .is_err());
        assert_eq!(fs::read_to_string(&new_path).unwrap(), "invalid data");
        // Legacy files must remain untouched when current path is invalid.
        assert!(atlas_generic_legacy_path.exists());
        assert!(legacy_path.exists());

        fs::remove_dir_all(test_dir).unwrap();
    }

    #[test]
    fn returns_none_when_no_current_or_legacy_data_exists() {
        let test_dir =
            std::env::temp_dir().join(format!("cofferly-no-data-test-{}", std::process::id()));
        let new_path = test_dir.join(APP_NAME).join(DATA_FILE_NAME);
        let atlas_generic_legacy_path = test_dir.join(ATLAS_LEGACY_APP_NAME).join(DATA_FILE_NAME);
        let atlas_legacy_path = test_dir
            .join(ATLAS_LEGACY_APP_NAME)
            .join(ATLAS_LEGACY_DATA_FILE_NAME);
        let legacy_path = test_dir.join(LEGACY_APP_NAME).join(LEGACY_DATA_FILE_NAME);
        let airwallet_legacy_path = test_dir
            .join(AIRWALLET_LEGACY_APP_NAME)
            .join(AIRWALLET_LEGACY_DATA_FILE_NAME);

        let loaded = load_app_data_with_paths(
            &new_path,
            &atlas_generic_legacy_path,
            &atlas_legacy_path,
            &legacy_path,
            &airwallet_legacy_path,
        )
        .unwrap();
        assert!(loaded.data.is_none());
        assert!(!loaded.migrated_from_legacy);

        let _ = fs::remove_dir_all(test_dir);
    }
}
