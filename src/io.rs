use std::fs;
use std::io::ErrorKind;
use std::io::Write;
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

use crate::crypto::SessionCrypto;
use crate::data::AppData;
use crate::{APP_NAME, DATA_FILE_NAME, PREVIOUS_DATA_FILE_NAME};
use chrono::NaiveDate;

#[derive(Debug, Default)]
pub struct DataVaultPreparation {
    /// The previous encrypted file is deliberately retained until the user has
    /// unlocked and verified the new vault on their own machine.
    pub preserved_previous_file: Option<PathBuf>,
}

pub fn data_path() -> PathBuf {
    app_data_base().join(APP_NAME).join(DATA_FILE_NAME)
}

fn previous_data_path() -> PathBuf {
    app_data_base().join(APP_NAME).join(PREVIOUS_DATA_FILE_NAME)
}

fn app_data_base() -> PathBuf {
    // Maintainer / screenshot isolation — not used in normal family installs.
    if let Ok(dir) = std::env::var("COFFERLY_DATA_DIR") {
        let trimmed = dir.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    dirs::data_local_dir()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Write vault bytes atomically (also used by the screenshot capture helper).
pub fn save_encrypted_bytes(path: &Path, contents: &[u8]) -> Result<(), String> {
    write_atomically(path, contents)
}

/// Prepare the current vault path without risking the previous encrypted file.
///
/// When only `data.json` exists, its encrypted bytes are copied atomically to
/// `vault.cofferly` and verified byte-for-byte. The source is never deleted. If
/// both files exist, the current vault always wins and neither file is changed.
pub fn prepare_data_vault() -> Result<DataVaultPreparation, String> {
    prepare_data_vault_at(&data_path(), &previous_data_path())
}

fn prepare_data_vault_at(
    current_path: &Path,
    previous_path: &Path,
) -> Result<DataVaultPreparation, String> {
    if current_path.exists() {
        return Ok(DataVaultPreparation::default());
    }

    let Some(previous_bytes) = load_raw(previous_path)? else {
        return Ok(DataVaultPreparation::default());
    };

    if !crate::crypto::is_current_format(&previous_bytes) {
        return Err(format!(
            "{} is not a supported encrypted Cofferly file. It was left untouched and no new vault was created",
            previous_path.display()
        ));
    }

    write_new_atomically(current_path, &previous_bytes).map_err(|err| {
        format!(
            "Could not copy {} to {}: {err}. The original file was left untouched",
            previous_path.display(),
            current_path.display()
        )
    })?;

    let copied_bytes = load_raw(current_path)?
        .ok_or_else(|| format!("Copied vault is missing at {}", current_path.display()))?;
    if copied_bytes != previous_bytes {
        return Err(format!(
            "Copied vault verification failed at {}. The original file remains untouched",
            current_path.display()
        ));
    }

    Ok(DataVaultPreparation {
        preserved_previous_file: Some(previous_path.to_path_buf()),
    })
}

/// Suggested file name for a vault backup made on `date`.
pub fn backup_file_name(date: NaiveDate) -> String {
    format!("Cofferly-backup-{}.cofferly", date.format("%Y-%m-%d"))
}

/// Copy the encrypted vault byte-for-byte to `dest`, then read it back and
/// compare before reporting success. An existing file at `dest` is only
/// replaced when `replace_existing` is set (the parent confirmed it).
pub fn write_backup(dest: &Path, vault_bytes: &[u8], replace_existing: bool) -> Result<(), String> {
    if !crate::crypto::is_current_format(vault_bytes) {
        return Err(
            "The current vault is not a supported encrypted Cofferly file, so it was not backed up"
                .to_owned(),
        );
    }

    let written = if replace_existing {
        write_atomically(dest, vault_bytes)
    } else if dest.exists() {
        Err(format!("{} already exists", dest.display()))
    } else {
        write_new_atomically(dest, vault_bytes)
    };
    written.map_err(|err| format!("Could not write the backup to {}: {err}", dest.display()))?;

    match load_raw(dest)? {
        Some(copied) if copied == vault_bytes => Ok(()),
        _ => Err(format!(
            "Backup verification failed at {}. Try another folder",
            dest.display()
        )),
    }
}

/// Read a chosen backup and check it is an encrypted Coffer Story vault.
/// Nothing is written.
pub fn read_backup(path: &Path) -> Result<Vec<u8>, String> {
    let bytes = load_raw(path)?.ok_or_else(|| format!("{} was not found", path.display()))?;
    if !crate::crypto::is_current_format(&bytes) {
        return Err(format!(
            "{} is not a Cofferly backup. Nothing on this PC was changed",
            path.display()
        ));
    }
    if bytes.first() != Some(&crate::crypto::STORY_VERSION) {
        return Err(
            "This backup is from before Coffer Story. Unlock it with its legacy PIN on the PC it came from, then make a new backup. Nothing on this PC was changed"
                .to_owned(),
        );
    }
    Ok(bytes)
}

/// Decrypt a backup with the Coffer Story entered for it.
pub fn open_backup(bytes: &[u8], secret: &str) -> Result<(AppData, SessionCrypto), String> {
    let (plain, session) = crate::crypto::decrypt(bytes, secret)
        .map_err(|_| "Wrong Coffer Story for this backup, or the file is damaged.".to_owned())?;
    let data = serde_json::from_slice::<AppData>(&plain)
        .ok()
        .and_then(crate::data::normalize_app_data)
        .ok_or_else(|| "The backup is invalid after decryption.".to_owned())?;
    Ok((data, session))
}

/// Replace the vault at `current_path` with `backup_bytes`.
///
/// The current vault (if any) is first kept next to it as
/// `vault.pre-restore-<stamp>.cofferly` and verified; the replace itself is
/// atomic. Any failure before the replace leaves the current vault untouched,
/// and a failed read-back after it puts the original bytes back. Returns the
/// pre-restore copy's path.
pub fn restore_vault(
    current_path: &Path,
    backup_bytes: &[u8],
    stamp: &str,
) -> Result<Option<PathBuf>, String> {
    if !crate::crypto::is_current_format(backup_bytes) {
        return Err("The backup is not a supported encrypted Cofferly file".to_owned());
    }

    let current = load_raw(current_path)?;
    let pre_restore_path = match &current {
        Some(current_bytes) => {
            let parent = current_path.parent().ok_or_else(|| {
                format!(
                    "Could not find parent folder for {}",
                    current_path.display()
                )
            })?;
            let path = parent.join(format!("vault.pre-restore-{stamp}.cofferly"));
            write_new_atomically(&path, current_bytes).map_err(|err| {
                format!(
                    "Could not keep a copy of the current vault at {}: {err}",
                    path.display()
                )
            })?;
            if load_raw(&path)?.as_deref() != Some(current_bytes.as_slice()) {
                return Err(format!(
                    "Could not verify the copy of the current vault at {}",
                    path.display()
                ));
            }
            Some(path)
        }
        None => None,
    };

    write_atomically(current_path, backup_bytes)
        .map_err(|err| format!("Could not replace {}: {err}", current_path.display()))?;

    if load_raw(current_path)?.as_deref() != Some(backup_bytes) {
        if let Some(original) = &current {
            let _ = write_atomically(current_path, original);
        }
        return Err(format!(
            "Restored vault verification failed at {}",
            current_path.display()
        ));
    }

    Ok(pre_restore_path)
}

pub fn load_raw(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
        Err(err) => Err(format!("Could not read {}: {err}", path.display())),
    }
}

/// Encrypt and write `data`, returning the ciphertext so callers can cache it
/// without a redundant disk read.
pub fn save_encrypted(
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

fn write_atomically(path: &Path, contents: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("Could not find parent folder for {}", path.display()))?;
    synced_temp_file(parent, contents)?
        .persist(path)
        .map_err(|err| err.error.to_string())?;
    Ok(())
}

/// Create a new file atomically without replacing a file another process may
/// have created after our existence check.
fn write_new_atomically(path: &Path, contents: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("Could not find parent folder for {}", path.display()))?;
    synced_temp_file(parent, contents)?
        .persist_noclobber(path)
        .map_err(|err| err.error.to_string())?;
    Ok(())
}

fn synced_temp_file(parent: &Path, contents: &[u8]) -> Result<tempfile::NamedTempFile, String> {
    fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    let mut temp_file = tempfile::NamedTempFile::new_in(parent).map_err(|err| err.to_string())?;
    temp_file
        .write_all(contents)
        .map_err(|err| err.to_string())?;
    temp_file
        .as_file_mut()
        .sync_all()
        .map_err(|err| err.to_string())?;
    Ok(temp_file)
}

/// Reserves a fresh, unpredictably-named temp file for family data (ledger
/// exports, the recovery card). Created with owner-only permissions (0600 on
/// Unix by default via `tempfile`) and a random suffix so a shared `/tmp`
/// cannot expose the contents to other local users or a pre-planted symlink.
/// Returns the persisted path; the caller writes its content and is
/// responsible for deleting it promptly (e.g. on lock/exit) rather than
/// leaving it for the next launch's best-effort cleanup.
pub fn reserve_private_temp_path(stem: &str, ext: &str) -> Result<PathBuf, String> {
    let temp_file = tempfile::Builder::new()
        .prefix(&format!("cofferly-{stem}-"))
        .suffix(&format!(".{ext}"))
        .rand_bytes(12)
        .tempfile_in(std::env::temp_dir())
        .map_err(|err| err.to_string())?;

    let (_file, path) = temp_file.keep().map_err(|err| err.error.to_string())?;
    Ok(path)
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
        if name.starts_with("cofferly-") && (name.ends_with(".html") || name.ends_with(".csv")) {
            let _ = fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::default_app_data;
    use tempfile::tempdir;

    fn encrypted_fixture(pin: &str) -> Vec<u8> {
        let serialized = serde_json::to_vec(&default_app_data()).unwrap();
        let mut session = None;
        crate::crypto::encrypt(&serialized, pin, &mut session).unwrap()
    }

    fn story_fixture(secret: &str, child_name: &str) -> Vec<u8> {
        let mut data = default_app_data();
        data.wallets[0].child_name = child_name.to_owned();
        let serialized = serde_json::to_vec(&data).unwrap();
        let mut session = Some(crate::crypto::SessionCrypto::establish(secret).unwrap());
        crate::crypto::encrypt(&serialized, secret, &mut session).unwrap()
    }

    #[test]
    fn backup_file_name_uses_the_date() {
        let date = NaiveDate::from_ymd_opt(2026, 9, 3).unwrap();
        assert_eq!(
            backup_file_name(date),
            "Cofferly-backup-2026-09-03.cofferly"
        );
    }

    #[test]
    fn backup_is_a_verified_byte_for_byte_copy() {
        let dir = tempdir().unwrap();
        let vault = story_fixture("story-secret", "Child A");
        let dest = dir
            .path()
            .join("backups")
            .join("Cofferly-backup-2026-09-03.cofferly");

        write_backup(&dest, &vault, false).unwrap();

        assert_eq!(fs::read(&dest).unwrap(), vault);
    }

    #[test]
    fn backup_never_overwrites_without_confirmation() {
        let dir = tempdir().unwrap();
        let vault = story_fixture("story-secret", "Child A");
        let dest = dir.path().join("existing.cofferly");
        fs::write(&dest, b"keep me").unwrap();

        let error = write_backup(&dest, &vault, false).unwrap_err();
        assert!(error.contains("already exists"));
        assert_eq!(fs::read(&dest).unwrap(), b"keep me");

        write_backup(&dest, &vault, true).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), vault);
    }

    #[test]
    fn backup_refuses_bytes_that_are_not_a_vault() {
        let dir = tempdir().unwrap();
        let dest = dir.path().join("backup.cofferly");

        assert!(write_backup(&dest, b"{}", false).is_err());
        assert!(!dest.exists());
    }

    #[test]
    fn restore_rejects_a_file_that_is_not_a_cofferly_backup() {
        let dir = tempdir().unwrap();
        let not_a_backup = dir.path().join("notes.cofferly");
        fs::write(&not_a_backup, br#"{"wallets":[]}"#).unwrap();

        let error = read_backup(&not_a_backup).unwrap_err();

        assert!(error.contains("not a Cofferly backup"));
        assert!(read_backup(&dir.path().join("missing.cofferly")).is_err());
    }

    #[test]
    fn restore_rejects_a_legacy_pin_backup() {
        let dir = tempdir().unwrap();
        let legacy = dir.path().join("legacy.cofferly");
        let mut bytes = encrypted_fixture("2468");
        bytes[0] = crate::crypto::LEGACY_PIN_VERSION;
        fs::write(&legacy, bytes).unwrap();

        assert!(read_backup(&legacy).unwrap_err().contains("legacy PIN"));
    }

    #[test]
    fn restore_rejects_the_wrong_story_and_accepts_the_right_one() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("backup.cofferly");
        fs::write(&path, story_fixture("right-story", "Child A")).unwrap();
        let bytes = read_backup(&path).unwrap();

        let error = open_backup(&bytes, "wrong-story").unwrap_err();
        assert!(error.contains("Wrong Coffer Story"));

        let (data, _session) = open_backup(&bytes, "right-story").unwrap();
        assert_eq!(data.wallets[0].child_name, "Child A");
    }

    #[test]
    fn restore_keeps_a_pre_restore_copy_and_replaces_the_vault() {
        let dir = tempdir().unwrap();
        let current_path = dir.path().join(DATA_FILE_NAME);
        let current = story_fixture("current-story", "Current child");
        let backup = story_fixture("backup-story", "Backup child");
        fs::write(&current_path, &current).unwrap();

        let kept = restore_vault(&current_path, &backup, "20260903-101500")
            .unwrap()
            .unwrap();

        assert_eq!(
            kept,
            dir.path()
                .join("vault.pre-restore-20260903-101500.cofferly")
        );
        assert_eq!(fs::read(&kept).unwrap(), current);
        assert_eq!(fs::read(&current_path).unwrap(), backup);
    }

    #[test]
    fn restore_onto_a_fresh_install_needs_no_pre_restore_copy() {
        let dir = tempdir().unwrap();
        let current_path = dir.path().join(DATA_FILE_NAME);
        let backup = story_fixture("backup-story", "Backup child");

        assert_eq!(
            restore_vault(&current_path, &backup, "stamp").unwrap(),
            None
        );
        assert_eq!(fs::read(&current_path).unwrap(), backup);
    }

    #[test]
    fn restore_failure_leaves_the_current_vault_untouched() {
        let dir = tempdir().unwrap();
        let current_path = dir.path().join(DATA_FILE_NAME);
        let current = story_fixture("current-story", "Current child");
        fs::write(&current_path, &current).unwrap();

        // Not a vault: rejected before anything is written.
        assert!(restore_vault(&current_path, b"not a vault", "a").is_err());
        assert_eq!(fs::read(&current_path).unwrap(), current);

        // The pre-restore copy cannot be created (name already taken): no replace.
        let taken = dir.path().join("vault.pre-restore-b.cofferly");
        fs::write(&taken, b"occupied").unwrap();
        let backup = story_fixture("backup-story", "Backup child");
        assert!(restore_vault(&current_path, &backup, "b").is_err());
        assert_eq!(fs::read(&current_path).unwrap(), current);
        assert_eq!(fs::read(&taken).unwrap(), b"occupied");
    }

    #[test]
    fn stores_current_data_in_vault_file() {
        assert_eq!(DATA_FILE_NAME, "vault.cofferly");
        assert_eq!(PREVIOUS_DATA_FILE_NAME, "data.json");
    }

    #[test]
    fn copies_previous_encrypted_file_and_preserves_the_backup() {
        let test_dir = tempdir().unwrap();
        let previous_path = test_dir.path().join(PREVIOUS_DATA_FILE_NAME);
        let current_path = test_dir.path().join(DATA_FILE_NAME);
        let previous_bytes = encrypted_fixture("2468");
        fs::write(&previous_path, &previous_bytes).unwrap();

        let preparation = prepare_data_vault_at(&current_path, &previous_path).unwrap();

        assert_eq!(
            preparation.preserved_previous_file.as_deref(),
            Some(previous_path.as_path())
        );
        assert_eq!(fs::read(&current_path).unwrap(), previous_bytes);
        assert_eq!(fs::read(&previous_path).unwrap(), previous_bytes);
        let (plaintext, _) = crate::crypto::decrypt(&previous_bytes, "2468").unwrap();
        let copied_data: AppData = serde_json::from_slice(&plaintext).unwrap();
        assert_eq!(copied_data.wallets.len(), default_app_data().wallets.len());
        assert_eq!(copied_data.wallets[0].child_name, "Child 1");
    }

    #[test]
    fn existing_current_vault_takes_precedence_without_touching_either_file() {
        let test_dir = tempdir().unwrap();
        let previous_path = test_dir.path().join(PREVIOUS_DATA_FILE_NAME);
        let current_path = test_dir.path().join(DATA_FILE_NAME);
        let previous_bytes = encrypted_fixture("1111");
        let current_bytes = encrypted_fixture("2222");
        fs::write(&previous_path, &previous_bytes).unwrap();
        fs::write(&current_path, &current_bytes).unwrap();

        let preparation = prepare_data_vault_at(&current_path, &previous_path).unwrap();

        assert!(preparation.preserved_previous_file.is_none());
        assert_eq!(fs::read(&current_path).unwrap(), current_bytes);
        assert_eq!(fs::read(&previous_path).unwrap(), previous_bytes);
    }

    #[test]
    fn unsupported_previous_file_is_left_untouched_without_creating_a_vault() {
        let test_dir = tempdir().unwrap();
        let previous_path = test_dir.path().join(PREVIOUS_DATA_FILE_NAME);
        let current_path = test_dir.path().join(DATA_FILE_NAME);
        let unsupported = br#"{"wallets":[]}"#;
        fs::write(&previous_path, unsupported).unwrap();

        let error = prepare_data_vault_at(&current_path, &previous_path).unwrap_err();

        assert!(error.contains("not a supported encrypted Cofferly file"));
        assert!(!current_path.exists());
        assert_eq!(fs::read(&previous_path).unwrap(), unsupported);
    }

    #[test]
    fn no_existing_files_is_a_clean_first_run() {
        let test_dir = tempdir().unwrap();
        let previous_path = test_dir.path().join(PREVIOUS_DATA_FILE_NAME);
        let current_path = test_dir.path().join(DATA_FILE_NAME);

        let preparation = prepare_data_vault_at(&current_path, &previous_path).unwrap();

        assert!(preparation.preserved_previous_file.is_none());
        assert!(!current_path.exists());
    }

    #[test]
    fn load_raw_returns_none_for_a_missing_file() {
        let test_dir = tempdir().unwrap();
        let path = test_dir.path().join(APP_NAME).join(DATA_FILE_NAME);

        assert_eq!(load_raw(&path).unwrap(), None);
    }

    #[test]
    fn save_encrypted_replaces_existing_file() {
        let test_dir = tempdir().unwrap();
        let path = test_dir.path().join(APP_NAME).join(DATA_FILE_NAME);
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
    }

    #[test]
    fn write_atomically_replaces_existing_bytes() {
        let test_dir = tempdir().unwrap();
        let path = test_dir.path().join("nested").join("data.bin");

        write_atomically(&path, b"first").unwrap();
        write_atomically(&path, b"second").unwrap();

        assert_eq!(load_raw(&path).unwrap().unwrap(), b"second");
    }

    #[test]
    fn write_new_atomically_refuses_to_clobber_existing_file() {
        let test_dir = tempdir().unwrap();
        let path = test_dir.path().join("data.bin");
        fs::write(&path, b"original").unwrap();

        let error = write_new_atomically(&path, b"replacement").unwrap_err();

        assert!(!error.is_empty());
        assert_eq!(fs::read(&path).unwrap(), b"original");
    }

    #[test]
    fn write_atomically_fails_when_parent_is_not_a_directory() {
        let test_dir = tempdir().unwrap();
        let blocked = test_dir.path().join("blocked");
        fs::write(&blocked, b"not a directory").unwrap();
        let path = blocked.join("data.bin");

        let error = write_atomically(&path, b"payload").unwrap_err();

        assert!(!error.is_empty());
        assert!(!path.exists());
    }

    #[test]
    fn save_encrypted_write_failure_does_not_create_a_vault() {
        let test_dir = tempdir().unwrap();
        let blocked = test_dir.path().join("blocked");
        fs::write(&blocked, b"not a directory").unwrap();
        let path = blocked.join(APP_NAME).join(DATA_FILE_NAME);
        let data = default_app_data();
        let mut session = None;

        let error = save_encrypted(&path, &data, "1234", &mut session).unwrap_err();

        assert!(!error.is_empty());
        assert!(!path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn save_encrypted_write_failure_leaves_existing_bytes_untouched() {
        use std::os::unix::fs::PermissionsExt;

        let test_dir = tempdir().unwrap();
        let path = test_dir.path().join(APP_NAME).join(DATA_FILE_NAME);
        let data = default_app_data();
        let pin = "1234";
        let mut session = None;
        save_encrypted(&path, &data, pin, &mut session).unwrap();
        let original = fs::read(&path).unwrap();

        let parent = path.parent().unwrap();
        let original_permissions = fs::metadata(parent).unwrap().permissions();
        let mut readonly = original_permissions.clone();
        readonly.set_mode(0o500);
        fs::set_permissions(parent, readonly).unwrap();
        let mut mutated = data;
        mutated.wallets[0].child_name = "Unsaved wallet".to_owned();
        let error = save_encrypted(&path, &mutated, pin, &mut session).unwrap_err();
        fs::set_permissions(parent, original_permissions).unwrap();

        assert!(!error.is_empty());
        assert_eq!(fs::read(&path).unwrap(), original);
        let (plaintext, _) = crate::crypto::decrypt(&original, pin).unwrap();
        let loaded = serde_json::from_slice::<AppData>(&plaintext).unwrap();
        assert_eq!(
            loaded.wallets[0].child_name,
            default_app_data().wallets[0].child_name
        );
    }
}
