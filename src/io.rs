use std::fs;
use std::io::Write;
use std::path::PathBuf;

use crate::data::{normalize_app_data, AppData, Wallet, DEFAULT_PARENT_PIN};
use crate::{
    AIRWALLET_LEGACY_APP_NAME, AIRWALLET_LEGACY_DATA_FILE_NAME, APP_NAME, ATLAS_LEGACY_APP_NAME,
    ATLAS_LEGACY_DATA_FILE_NAME, DATA_FILE_NAME, LEGACY_APP_NAME, LEGACY_DATA_FILE_NAME,
};

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

pub fn load_app_data_with_legacy(path: &PathBuf) -> Result<Option<AppData>, String> {
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
) -> Result<Option<AppData>, String> {
    if path.exists() {
        return load_app_data(path).map(Some);
    }

    for legacy_path in [
        atlas_generic_legacy_path,
        atlas_legacy_path,
        legacy_path,
        airwallet_legacy_path,
    ] {
        let Some(data) = load_legacy_app_data(legacy_path) else {
            continue;
        };

        let _ = save_app_data(path, &data);

        return Ok(Some(data));
    }

    Ok(None)
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

pub fn save_app_data(path: &PathBuf, data: &AppData) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }

    let contents = serde_json::to_string_pretty(data).map_err(|err| err.to_string())?;
    let parent = path
        .parent()
        .ok_or_else(|| format!("Could not find parent folder for {}", path.display()))?;
    let mut temp_file = tempfile::NamedTempFile::new_in(parent).map_err(|err| err.to_string())?;
    temp_file
        .write_all(contents.as_bytes())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::default_app_data;

    #[test]
    fn imports_legacy_data_when_new_data_does_not_exist() {
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
        let data = default_app_data();

        save_app_data(&legacy_path, &data).unwrap();

        let loaded = load_app_data_with_paths(
            &new_path,
            &atlas_generic_legacy_path,
            &atlas_legacy_path,
            &legacy_path,
            &airwallet_legacy_path,
        )
        .unwrap()
        .unwrap();

        assert_eq!(loaded.wallets.len(), data.wallets.len());
        assert!(new_path.exists());

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
        .unwrap()
        .unwrap();

        assert_eq!(loaded.wallets.len(), data.wallets.len());
        assert!(new_path.exists());

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
        .unwrap()
        .unwrap();

        assert_eq!(loaded.wallets.len(), data.wallets.len());
        assert!(new_path.exists());

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

        assert!(load_app_data_with_paths(
            &new_path,
            &atlas_generic_legacy_path,
            &atlas_legacy_path,
            &legacy_path,
            &airwallet_legacy_path
        )
        .unwrap()
        .is_none());

        let _ = fs::remove_dir_all(test_dir);
    }
}
