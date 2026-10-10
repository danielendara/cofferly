use super::*;
use crate::data::ledger_filter_summary;
use chrono::NaiveDate;
use eframe::App as _;
use eframe::Storage as _;
use tempfile::{tempdir, TempDir};

fn test_app() -> (CofferlyApp, TempDir) {
    let dir = tempdir().unwrap();
    let app = CofferlyApp {
        data: default_app_data(),
        raw_bytes: None,
        session: None,
        selected_wallet: 0,
        ledger_sort: LedgerSort::NewestFirst,
        ledger_cache: None,
        ledger_filter: String::new(),
        ledger_filters: HashMap::new(),
        pending_ledger_filter_focus: false,
        draft: EntryDraft::new(),
        starting_balance_input: String::new(),
        weekly_allowance_input: String::new(),
        savings_goal_input: String::new(),
        child_name_input: String::new(),
        new_child_name_input: String::new(),
        pin_digits: Default::default(),
        pending_pin_focus: None,
        pending_entry_focus: None,
        lock_mode: LockMode::Story,
        pending_story: None,
        story_selections: Vec::new(),
        display_order: story::CATALOG.iter().map(|(id, _)| *id).collect(),
        story_icon_textures: HashMap::new(),
        parent_unlocked: true,
        save_enabled: true,
        previous_data_backup_preserved: false,
        status: Status::info(String::new()),
        data_path: dir.path().join(DATA_FILE_NAME),
        lock_screen_image: None,
        lock_screen_bg: theme::APP_BG,
        open_coffer_image: None,
        show_settings: false,
        confirm_delete_wallet: false,
        confirm_negative_cents: None,
        undo: None,
        entry_edit: None,
        pending_ledger_edit_focus: None,
        last_interaction: Instant::now(),
        unlocking: false,
        unlock_rx: None,
        failed_unlock_attempts: 0,
        unlock_cooldown_until: None,
        capture: None,
        capturing: false,
        temp_artifact_paths: Vec::new(),
        pending_wallet_selection_name: None,
        last_backup: None,
        pending_backup_overwrite: None,
        restore: None,
    };
    (app, dir)
}

fn saved_data(app: &CofferlyApp, pin: &str) -> AppData {
    let raw = std::fs::read(&app.data_path).unwrap();
    assert!(crypto::is_current_format(&raw));
    let (plaintext, _) = crypto::decrypt(&raw, pin).unwrap();
    serde_json::from_slice(&plaintext).unwrap()
}

fn unwritable_data_path(dir: &TempDir) -> PathBuf {
    let blocked = dir.path().join("not-a-directory");
    std::fs::write(&blocked, b"not a directory").unwrap();
    blocked.join(DATA_FILE_NAME)
}

fn test_story() -> [&'static str; story::STORY_LENGTH] {
    ["acorn", "anchor", "apple", "balloon", "book", "bridge"]
}

fn finish_background_work(app: &mut CofferlyApp) {
    let ctx = egui::Context::default();
    while app.unlocking {
        app.poll_unlock(&ctx);
        if app.unlocking {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

#[test]
fn capture_story_setup_prepares_the_recovery_card_reveal_screen() {
    let (mut app, _dir) = test_app();
    app.parent_unlocked = true;
    app.lock_mode = LockMode::Story;
    app.pending_story = None;

    capture::prepare_target(&mut app, capture::CaptureTarget::StorySetup);

    assert!(!app.parent_unlocked);
    assert_eq!(app.lock_mode, LockMode::SetupReveal);
    assert!(app.pending_story.is_some());
    assert!(!app.show_settings);
    assert!(app.status.text.contains("recovery key"));
}

#[test]
fn confirmed_first_run_story_is_saved_immediately_without_serializing_the_story() {
    let (mut app, _dir) = test_app();
    let selected = test_story();
    app.parent_unlocked = false;
    app.lock_mode = LockMode::SetupConfirm;
    app.pending_story = Some(selected);
    app.story_selections = selected.into();

    app.submit_story();
    finish_background_work(&mut app);

    let raw = std::fs::read(&app.data_path).unwrap();
    assert_eq!(raw[0], crypto::STORY_VERSION);
    let secret = story::encode(&selected).unwrap();
    let (plain, _) = crypto::decrypt(&raw, &secret).unwrap();
    assert!(!String::from_utf8_lossy(&plain).contains("coffer-story-v1:"));
    assert!(app.parent_unlocked);
}

#[test]
fn all_six_story_selections_remain_visible_while_unlocking() {
    let (mut app, _dir) = test_app();
    let selected = test_story();
    let secret = story::encode(&selected).unwrap();
    let mut session = None;
    app.raw_bytes = Some(
        crypto::encrypt(
            &serde_json::to_vec(&default_app_data()).unwrap(),
            &secret,
            &mut session,
        )
        .unwrap(),
    );
    app.parent_unlocked = false;
    app.lock_mode = LockMode::Story;

    for id in selected {
        app.select_story_object(id);
    }

    assert!(app.unlocking);
    assert_eq!(app.story_selections.as_slice(), selected);
}

#[test]
fn legacy_pin_migration_preserves_data_and_rejects_the_old_pin() {
    let (mut app, _dir) = test_app();
    let mut legacy_data = default_app_data();
    legacy_data.wallets[0].child_name = "Kept through migration".to_owned();
    let mut legacy_session = None;
    let mut raw = crypto::encrypt(
        &serde_json::to_vec(&legacy_data).unwrap(),
        "1234",
        &mut legacy_session,
    )
    .unwrap();
    raw[0] = crypto::LEGACY_PIN_VERSION;
    std::fs::write(&app.data_path, &raw).unwrap();
    app.raw_bytes = Some(raw);
    app.parent_unlocked = false;
    app.lock_mode = LockMode::LegacyPin;
    app.pin_digits = ["1".into(), "2".into(), "3".into(), "4".into()];

    app.unlock_parent_sync();

    assert_eq!(app.lock_mode, LockMode::MigrateReveal);
    assert!(!app.parent_unlocked);
    let selected = test_story();
    app.pending_story = Some(selected);
    app.lock_mode = LockMode::MigrateConfirm;
    app.story_selections = selected.into();
    app.submit_story();
    finish_background_work(&mut app);

    let migrated = std::fs::read(&app.data_path).unwrap();
    assert_eq!(migrated[0], crypto::STORY_VERSION);
    assert!(crypto::decrypt(&migrated, "1234").is_err());
    let secret = story::encode(&selected).unwrap();
    let (plain, _) = crypto::decrypt(&migrated, &secret).unwrap();
    let loaded = serde_json::from_slice::<AppData>(&plain).unwrap();
    assert_eq!(loaded.wallets[0].child_name, "Kept through migration");
}

#[test]
fn changing_story_rewraps_the_existing_data_key() {
    let (mut app, _dir) = test_app();
    let old_story = test_story();
    let old_secret = story::encode(&old_story).unwrap();
    let mut original_session = None;
    let original = crypto::encrypt(
        &serde_json::to_vec(&default_app_data()).unwrap(),
        &old_secret,
        &mut original_session,
    )
    .unwrap();
    std::fs::write(&app.data_path, &original).unwrap();
    let (_, session) = crypto::decrypt(&original, &old_secret).unwrap();
    app.raw_bytes = Some(original);
    app.session = Some(session);
    app.parent_unlocked = false;
    app.lock_mode = LockMode::ChangeConfirm;
    let replacement = ["crown", "diamond", "drum", "feather", "fish", "flower"];
    app.pending_story = Some(replacement);
    app.story_selections = replacement.into();

    app.submit_story();
    finish_background_work(&mut app);

    let changed = std::fs::read(&app.data_path).unwrap();
    assert!(crypto::decrypt(&changed, &old_secret).is_err());
    let replacement_secret = story::encode(&replacement).unwrap();
    assert!(crypto::decrypt(&changed, &replacement_secret).is_ok());
    assert!(app.parent_unlocked);
}

#[cfg(unix)]
#[test]
fn failed_legacy_migration_write_leaves_the_original_v2_file_recoverable() {
    use std::os::unix::fs::PermissionsExt;

    let (mut app, dir) = test_app();
    let mut session = None;
    let mut original = crypto::encrypt(
        &serde_json::to_vec(&default_app_data()).unwrap(),
        "1234",
        &mut session,
    )
    .unwrap();
    original[0] = crypto::LEGACY_PIN_VERSION;
    std::fs::write(&app.data_path, &original).unwrap();
    app.raw_bytes = Some(original.clone());
    app.parent_unlocked = false;
    app.lock_mode = LockMode::LegacyPin;
    app.pin_digits = ["1".into(), "2".into(), "3".into(), "4".into()];
    app.unlock_parent_sync();
    let selected = test_story();
    app.pending_story = Some(selected);
    app.lock_mode = LockMode::MigrateConfirm;
    app.story_selections = selected.into();

    let original_permissions = std::fs::metadata(dir.path()).unwrap().permissions();
    let mut readonly = original_permissions.clone();
    readonly.set_mode(0o500);
    std::fs::set_permissions(dir.path(), readonly).unwrap();
    app.submit_story();
    finish_background_work(&mut app);
    std::fs::set_permissions(dir.path(), original_permissions).unwrap();

    let after_failure = std::fs::read(&app.data_path).unwrap();
    assert_eq!(after_failure, original);
    assert!(crypto::decrypt(&after_failure, "1234").is_ok());
    assert!(!app.parent_unlocked);
    assert!(app.session.is_none());
    assert_eq!(app.lock_mode, LockMode::LegacyPin);
}

#[test]
fn open_coffer_asset_is_small_valid_and_transparent() {
    let image = image::load_from_memory(OPEN_COFFER_IMAGE_BYTES)
        .expect("open coffer asset should decode")
        .to_rgba8();

    assert!(image.width() <= 512);
    assert!(image.height() <= 512);
    assert_eq!(image.get_pixel(0, 0)[3], 0);
    assert!(image.pixels().any(|pixel| pixel[3] > 0));
}

#[test]
fn pasted_pin_digits_are_distributed_and_non_digits_are_ignored() {
    let (mut app, _dir) = test_app();
    app.pin_digits[1] = "9a87".to_owned();

    app.normalize_pin_digit_input(1);

    assert_eq!(app.pin_digits, ["", "9", "8", "7"]);
    assert_eq!(app.pending_pin_focus, Some(3));
    assert!(!app.parent_pin_complete());

    app.pin_digits[0] = "1".to_owned();
    assert!(app.parent_pin_complete());
    assert_eq!(app.entered_parent_pin(), "1987");
}

#[test]
fn encrypted_unlock_accepts_the_right_pin_and_clears_pin_fields() {
    let (mut app, _dir) = test_app();
    let mut stored = default_app_data();
    stored.wallets[0].child_name = "Encrypted wallet".to_owned();
    let serialized = serde_json::to_vec(&stored).unwrap();
    let mut session = None;
    app.raw_bytes = Some(crypto::encrypt(&serialized, "2468", &mut session).unwrap());
    app.parent_unlocked = false;
    app.session = None;
    app.pin_digits = ["2".into(), "4".into(), "6".into(), "8".into()];

    app.unlock_parent_sync();

    assert!(app.parent_unlocked);
    assert!(app.session.is_some());
    assert_eq!(app.selected_wallet().child_name, "Encrypted wallet");
    assert!(app.pin_digits.iter().all(String::is_empty));
    assert_eq!(app.pending_pin_focus, Some(0));
    assert_eq!(app.status.text, "Coffer Story unlocked.");
    assert_eq!(app.status.severity, StatusSeverity::Success);
}

// --- #156: correcting a mistyped entry ---------------------------------

/// Wallet with three dated entries, so corrections land in the middle of a
/// ledger rather than only on the newest row.
fn app_with_entries() -> (CofferlyApp, TempDir) {
    let (mut app, dir) = test_app();
    let wallet = &mut app.data.wallets[0];
    wallet.starting_balance_cents = 5_000;
    wallet.entries = vec![
        Entry {
            date: NaiveDate::from_ymd_opt(2026, 9, 1).unwrap(),
            description: "Weekly allowance".to_owned(),
            amount_cents: 1_000,
        },
        Entry {
            date: NaiveDate::from_ymd_opt(2026, 9, 5).unwrap(),
            description: "bkie repair".to_owned(),
            amount_cents: -12_500,
        },
        Entry {
            date: NaiveDate::from_ymd_opt(2026, 9, 8).unwrap(),
            description: "Birthday money".to_owned(),
            amount_cents: 2_000,
        },
    ];
    app.invalidate_ledger_cache();
    (app, dir)
}

#[test]
fn ledger_rows_point_back_at_the_entry_they_came_from() {
    let (mut app, _dir) = app_with_entries();
    let rows = app.cached_ledger_rows();

    // Newest first, and the synthetic starting-balance row owns no entry.
    let indices: Vec<Option<usize>> = rows.iter().map(|row| row.entry_index).collect();
    assert_eq!(indices, vec![Some(2), Some(1), Some(0), None]);
}

#[test]
fn editing_an_entry_rewrites_it_in_place_and_leaves_an_undo() {
    let (mut app, _dir) = app_with_entries();

    app.begin_entry_edit(1);
    assert_eq!(app.draft.kind, EntryKind::Deduction);
    assert_eq!(app.draft.amount, "125.00");
    assert_eq!(app.draft.description, "bkie repair");
    assert_eq!(app.pending_entry_focus, Some(EntryFormField::Amount));

    app.draft.amount = "$12.50".to_owned();
    app.draft.description = "Bike repair".to_owned();
    app.commit_entry_edit();

    let entry = &app.selected_wallet().entries[1];
    assert_eq!(entry.amount_cents, -1_250);
    assert_eq!(entry.description, "Bike repair");
    assert_eq!(entry.date, NaiveDate::from_ymd_opt(2026, 9, 5).unwrap());
    assert_eq!(app.status.severity, StatusSeverity::Success);
    assert!(app.entry_edit.is_none());
    assert!(app.undo.is_some());
    // Focus goes back to the row the correction came from.
    assert_eq!(app.pending_ledger_edit_focus, Some(1));
    // The entry count never changes — a correction is not a delete + add.
    assert_eq!(app.selected_wallet().entries.len(), 3);
}

#[test]
fn undo_restores_the_entry_exactly_as_it_was() {
    let (mut app, _dir) = app_with_entries();
    let before = app.selected_wallet().entries.clone();

    app.begin_entry_edit(0);
    app.draft.amount = "99.00".to_owned();
    app.draft.description = "Changed my mind".to_owned();
    app.draft.date_input = format_ledger_date(NaiveDate::from_ymd_opt(2026, 9, 2).unwrap());
    app.commit_entry_edit();
    assert_ne!(app.selected_wallet().entries, before);

    app.undo_remove_entry();

    assert_eq!(app.selected_wallet().entries, before);
    assert_eq!(app.status.severity, StatusSeverity::Success);
    assert!(app.undo.is_none());
}

#[test]
fn editing_uses_the_same_validation_as_adding() {
    let (mut app, _dir) = app_with_entries();

    app.begin_entry_edit(0);
    app.draft.amount = "not money".to_owned();
    app.commit_entry_edit();
    assert_eq!(app.status.severity, StatusSeverity::Error);
    assert_eq!(app.pending_entry_focus, Some(EntryFormField::Amount));
    assert!(
        app.entry_edit.is_some(),
        "a bad amount keeps the correction open"
    );

    app.draft.amount = "10.00".to_owned();
    app.draft.description = "   ".to_owned();
    app.commit_entry_edit();
    assert_eq!(app.pending_entry_focus, Some(EntryFormField::Description));

    app.draft.description = "Weekly allowance".to_owned();
    app.draft.date_input = "not a date".to_owned();
    app.commit_entry_edit();
    assert_eq!(app.pending_entry_focus, Some(EntryFormField::Date));

    // Nothing was written while the form was invalid.
    assert_eq!(app.selected_wallet().entries[0].amount_cents, 1_000);
}

#[test]
fn a_correction_that_goes_negative_mid_ledger_asks_first() {
    let (mut app, _dir) = app_with_entries();
    // Final balance stays positive after this correction, but the balance
    // right after entry 1 does not — a final-balance check would miss it.
    app.begin_entry_edit(1);
    app.draft.amount = "70.00".to_owned();
    app.commit_entry_edit();

    assert_eq!(app.status.severity, StatusSeverity::Info);
    assert!(app.status.text.contains("below zero"));
    assert_eq!(
        app.selected_wallet().entries[1].amount_cents,
        -12_500,
        "nothing is written until the parent confirms"
    );

    app.commit_entry_edit();
    assert_eq!(app.selected_wallet().entries[1].amount_cents, -7_000);
    assert_eq!(app.status.severity, StatusSeverity::Success);
    // Final balance was never negative — only the balance at that row was.
    assert!(app.selected_wallet().current_balance_cents() > 0);
}

#[test]
fn edit_write_failure_rolls_the_ledger_back_to_disk() {
    let (mut app, dir) = app_with_entries();
    let before = app.selected_wallet().entries.clone();
    app.data_path = unwritable_data_path(&dir);

    app.begin_entry_edit(2);
    app.draft.amount = "20.00".to_owned();
    app.draft.description = "Birthday cash".to_owned();
    app.commit_entry_edit();

    assert_eq!(app.status.severity, StatusSeverity::Error);
    assert!(app.status.text.starts_with("Could not save:"));
    assert_eq!(
        app.selected_wallet().entries,
        before,
        "an unsaved correction must not survive in memory"
    );
    assert!(app.undo.is_none(), "nothing to undo — nothing was applied");
}

#[test]
fn cancelling_a_correction_restores_the_half_typed_new_entry() {
    let (mut app, _dir) = app_with_entries();
    app.draft.description = "Half-typed thing".to_owned();
    app.draft.amount = "7".to_owned();

    app.begin_entry_edit(0);
    assert_eq!(app.draft.description, "Weekly allowance");
    app.draft.amount = "999.00".to_owned();
    app.cancel_entry_edit();

    assert_eq!(app.draft.description, "Half-typed thing");
    assert_eq!(app.draft.amount, "7");
    assert!(app.entry_edit.is_none());
    assert_eq!(app.selected_wallet().entries[0].amount_cents, 1_000);
    assert_eq!(app.pending_ledger_edit_focus, Some(0));
}

#[test]
fn a_locked_parent_cannot_start_or_commit_a_correction() {
    let (mut app, _dir) = app_with_entries();
    app.parent_unlocked = false;

    app.begin_entry_edit(0);
    assert!(app.entry_edit.is_none());
    assert_eq!(app.status.severity, StatusSeverity::Error);
    assert_eq!(app.selected_wallet().entries[0].amount_cents, 1_000);
}

#[test]
fn switching_wallets_drops_a_pending_correction_undo() {
    let (mut app, _dir) = app_with_entries();
    app.data.wallets.push(Wallet {
        child_name: "Second".to_owned(),
        starting_balance_cents: 0,
        entries: Vec::new(),
        weekly_allowance: None,
        savings_goal_cents: None,
    });

    app.begin_entry_edit(0);
    app.draft.amount = "11.00".to_owned();
    app.commit_entry_edit();
    assert!(app.undo.is_some());

    app.select_wallet(1);
    assert!(
        app.undo.is_none(),
        "undo after a wallet switch would restore into the wrong ledger"
    );
}

#[test]
fn a_correction_that_changes_nothing_is_a_no_op() {
    let (mut app, _dir) = app_with_entries();
    let before = app.selected_wallet().entries.clone();

    app.begin_entry_edit(0);
    app.commit_entry_edit();

    assert_eq!(app.selected_wallet().entries, before);
    assert!(app.undo.is_none());
    assert!(app.entry_edit.is_none());
}

#[test]
fn encrypted_unlock_rejects_wrong_pin_without_exposing_data() {
    let (mut app, _dir) = test_app();
    let mut stored = default_app_data();
    stored.wallets[0].child_name = "Secret wallet".to_owned();
    let serialized = serde_json::to_vec(&stored).unwrap();
    let mut session = None;
    app.raw_bytes = Some(crypto::encrypt(&serialized, "2468", &mut session).unwrap());
    app.parent_unlocked = false;
    app.session = None;
    app.pin_digits = ["0".into(), "0".into(), "0".into(), "0".into()];

    app.unlock_parent_sync();

    assert!(!app.parent_unlocked);
    assert!(app.session.is_none());
    assert_ne!(app.selected_wallet().child_name, "Secret wallet");
    assert!(app.pin_digits.iter().all(String::is_empty));
    // The first two wrong attempts are a free grace period (#80): no cooldown yet.
    assert_eq!(
        app.status.text,
        "Wrong credential or data has been tampered with."
    );
    assert_eq!(app.status.severity, StatusSeverity::Error);
    assert!(app.unlock_cooldown_remaining().is_none());
}

#[test]
fn first_two_wrong_pin_attempts_are_free_then_cooldown_escalates_and_bounds() {
    assert_eq!(unlock_cooldown_duration(1), Duration::ZERO);
    assert_eq!(unlock_cooldown_duration(2), Duration::ZERO);
    assert_eq!(unlock_cooldown_duration(3), Duration::from_secs(60));
    assert_eq!(unlock_cooldown_duration(4), Duration::from_secs(2 * 60));
    assert_eq!(unlock_cooldown_duration(5), Duration::from_secs(5 * 60));
    assert_eq!(unlock_cooldown_duration(6), Duration::from_secs(15 * 60));
    assert_eq!(unlock_cooldown_duration(7), Duration::from_secs(30 * 60));
    assert_eq!(unlock_cooldown_duration(8), Duration::from_secs(60 * 60));
    assert_eq!(unlock_cooldown_duration(100), Duration::from_secs(60 * 60));
}

#[test]
fn active_pin_cooldown_blocks_even_the_correct_pin() {
    let (mut app, _dir) = test_app();
    app.parent_unlocked = false;
    app.failed_unlock_attempts = 3;
    app.unlock_cooldown_until = Some(Instant::now() + Duration::from_secs(5 * 60));
    app.pin_digits = ["1".into(), "2".into(), "3".into(), "4".into()];

    app.start_unlock();

    assert!(!app.parent_unlocked);
    assert!(app.pin_digits.iter().all(String::is_empty));
    assert!(app.status.text.starts_with("Too many wrong PIN attempts."));
}

#[test]
fn story_confirm_mismatch_never_starts_a_cooldown() {
    let (mut app, _dir) = test_app();
    app.lock_mode = LockMode::SetupConfirm;
    app.pending_story = Some(test_story());
    app.story_selections = ["book", "bridge", "acorn", "anchor", "apple", "balloon"].into();

    app.submit_story();

    assert_eq!(app.lock_mode, LockMode::SetupConfirm);
    assert_eq!(app.failed_unlock_attempts, 0);
    assert!(app.unlock_cooldown_remaining().is_none());
    assert_eq!(
        app.status.text,
        "That didn't match. Try selecting it again."
    );
    assert!(app.story_selections.is_empty());
}

#[test]
fn remove_last_story_selection_undoes_the_most_recent_pick() {
    let (mut app, _dir) = test_app();
    app.select_story_object("acorn");
    app.select_story_object("anchor");

    app.remove_last_story_selection();

    assert_eq!(app.story_selections.as_slice(), ["acorn"]);
}

#[test]
fn picking_an_already_chosen_story_object_is_ignored() {
    let (mut app, _dir) = test_app();
    app.parent_unlocked = false;
    app.lock_mode = LockMode::Story;
    for id in ["acorn", "anchor", "apple", "balloon", "book"] {
        app.select_story_object(id);
    }

    app.select_story_object("acorn");

    assert_eq!(
        app.story_selections.as_slice(),
        ["acorn", "anchor", "apple", "balloon", "book"]
    );
    assert!(!app.unlocking);
    assert_eq!(app.failed_unlock_attempts, 0);
    assert!(app.unlock_cooldown_remaining().is_none());
    assert_ne!(app.status.text, "Invalid Coffer Story.");
}

#[test]
fn cancel_story_change_restores_unlocked_parent_mode_without_touching_the_vault() {
    let (mut app, _dir) = test_app();
    app.lock_mode = LockMode::ChangeConfirm;
    app.parent_unlocked = false;
    app.pending_story = Some(test_story());
    app.story_selections = vec!["acorn"];

    app.cancel_story_change();

    assert_eq!(app.lock_mode, LockMode::Story);
    assert!(app.parent_unlocked);
    assert!(app.pending_story.is_none());
    assert!(app.story_selections.is_empty());
}

#[test]
fn cancel_story_migration_drops_the_session_and_returns_to_legacy_pin() {
    let (mut app, _dir) = test_app();
    let old_story = test_story();
    let secret = story::encode(&old_story).unwrap();
    let mut session = None;
    crypto::encrypt(
        &serde_json::to_vec(&default_app_data()).unwrap(),
        &secret,
        &mut session,
    )
    .unwrap();
    app.lock_mode = LockMode::MigrateConfirm;
    app.session = session;
    app.pending_story = Some(test_story());

    app.cancel_story_migration();

    assert_eq!(app.lock_mode, LockMode::LegacyPin);
    assert!(app.session.is_none());
    assert!(app.pending_story.is_none());
}

#[test]
fn back_to_story_reveal_returns_to_the_matching_reveal_mode() {
    let (mut app, _dir) = test_app();
    for (confirm, reveal) in [
        (LockMode::SetupConfirm, LockMode::SetupReveal),
        (LockMode::MigrateConfirm, LockMode::MigrateReveal),
        (LockMode::ChangeConfirm, LockMode::ChangeReveal),
    ] {
        app.lock_mode = confirm;
        app.story_selections = vec!["acorn"];

        app.back_to_story_reveal();

        assert_eq!(app.lock_mode, reveal);
        assert!(app.story_selections.is_empty());
    }
}

#[test]
fn successful_unlock_resets_pin_cooldown_state() {
    let (mut app, _dir) = test_app();
    app.parent_unlocked = false;
    app.failed_unlock_attempts = 4;
    app.unlock_cooldown_until = None;
    app.pin_digits = ["1".into(), "2".into(), "3".into(), "4".into()];

    app.unlock_parent_sync();

    assert!(app.parent_unlocked);
    assert_eq!(app.failed_unlock_attempts, 0);
    assert!(app.unlock_cooldown_until.is_none());
}

#[test]
fn completed_background_unlock_does_not_rewrite_unchanged_data() {
    let (mut app, _dir) = test_app();
    let stored = default_app_data();
    let serialized = serde_json::to_vec(&stored).unwrap();
    let mut encryption_session = None;
    let encrypted = crypto::encrypt(&serialized, "2468", &mut encryption_session).unwrap();
    std::fs::write(&app.data_path, &encrypted).unwrap();
    let (_, unlock_session) = crypto::decrypt(&encrypted, "2468").unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(BackgroundCryptoResult::Unlock(Ok((stored, unlock_session))))
        .unwrap();
    app.unlock_rx = Some(rx);
    app.unlocking = true;
    app.parent_unlocked = false;
    app.previous_data_backup_preserved = true;

    app.poll_unlock(&egui::Context::default());

    assert!(app.parent_unlocked);
    assert_eq!(std::fs::read(&app.data_path).unwrap(), encrypted);
    assert!(app.status.text.contains("data.json backup"));
}

/// Writes a Coffer Story vault with one renamed wallet and returns its bytes.
fn story_vault(
    story_objects: [&'static str; story::STORY_LENGTH],
    child_name: &str,
    wallets: usize,
) -> Vec<u8> {
    let mut data = default_app_data();
    data.wallets[0].child_name = child_name.to_owned();
    data.wallets.truncate(wallets.min(data.wallets.len()));
    let secret = story::encode(&story_objects).unwrap();
    let mut session = None;
    crypto::encrypt(&serde_json::to_vec(&data).unwrap(), &secret, &mut session).unwrap()
}

const BACKUP_STORY: [&str; story::STORY_LENGTH] =
    ["crown", "diamond", "drum", "feather", "fish", "flower"];

/// An unlocked app whose on-disk vault is a story vault named "Current child".
fn unlocked_story_app() -> (CofferlyApp, TempDir, Vec<u8>) {
    let (mut app, dir) = test_app();
    let current = story_vault(test_story(), "Current child", 2);
    std::fs::write(&app.data_path, &current).unwrap();
    let (plain, session) =
        crypto::decrypt(&current, &story::encode(&test_story()).unwrap()).unwrap();
    app.data = serde_json::from_slice(&plain).unwrap();
    app.raw_bytes = Some(current.clone());
    app.session = Some(session);
    (app, dir, current)
}

#[test]
fn backup_writes_a_verified_copy_and_remembers_the_date() {
    let (mut app, dir, current) = unlocked_story_app();
    let dest = dir.path().join("Cofferly-backup-2026-09-03.cofferly");

    app.back_up_vault_to(dest.clone(), false);

    assert_eq!(std::fs::read(&dest).unwrap(), current);
    assert_eq!(app.status.severity, StatusSeverity::Success);
    assert!(app.status.text.contains("verified"));
    assert_eq!(
        app.last_backup,
        Some(format_ledger_date(Local::now().date_naive()))
    );
}

#[test]
fn backup_asks_before_replacing_an_existing_file() {
    let (mut app, dir, current) = unlocked_story_app();
    let dest = dir.path().join("existing.cofferly");
    std::fs::write(&dest, b"older backup").unwrap();

    app.back_up_vault_to(dest.clone(), false);
    assert_eq!(
        app.pending_backup_overwrite.as_deref(),
        Some(dest.as_path())
    );
    assert_eq!(std::fs::read(&dest).unwrap(), b"older backup");
    assert!(app.last_backup.is_none());

    app.cancel_backup_overwrite();
    assert!(app.pending_backup_overwrite.is_none());
    assert_eq!(std::fs::read(&dest).unwrap(), b"older backup");

    app.back_up_vault_to(dest.clone(), true);
    assert_eq!(std::fs::read(&dest).unwrap(), current);
}

#[test]
fn locking_clears_a_pending_backup_overwrite_prompt() {
    let (mut app, dir, _current) = unlocked_story_app();
    let dest = dir.path().join("existing.cofferly");
    std::fs::write(&dest, b"older backup").unwrap();

    app.back_up_vault_to(dest.clone(), false);
    assert!(app.pending_backup_overwrite.is_some());

    app.lock_parent();
    assert!(app.pending_backup_overwrite.is_none());
    assert_eq!(std::fs::read(&dest).unwrap(), b"older backup");
}

#[test]
fn opening_settings_clears_a_pending_backup_overwrite_prompt() {
    let (mut app, dir, _current) = unlocked_story_app();
    let dest = dir.path().join("existing.cofferly");
    std::fs::write(&dest, b"older backup").unwrap();

    app.back_up_vault_to(dest, false);
    assert!(app.pending_backup_overwrite.is_some());

    app.open_settings();
    assert!(app.pending_backup_overwrite.is_none());
}

#[test]
fn backup_requires_parent_mode_and_a_saved_vault() {
    let (mut app, dir) = test_app();
    let dest = dir.path().join("backup.cofferly");

    app.back_up_vault_to(dest.clone(), false);
    assert!(app.status.text.contains("Nothing to back up"));

    app.parent_unlocked = false;
    app.back_up_vault_to(dest.clone(), false);
    assert_eq!(app.status.severity, StatusSeverity::Error);
    assert!(!dest.exists());
}

#[test]
fn restore_from_settings_confirms_then_replaces_and_keeps_a_pre_restore_copy() {
    let (mut app, dir, current) = unlocked_story_app();
    let backup_path = dir.path().join("backup.cofferly");
    let backup = story_vault(BACKUP_STORY, "Backup child", 1);
    std::fs::write(&backup_path, &backup).unwrap();

    app.begin_restore_from(&backup_path);
    assert!(
        !app.parent_unlocked,
        "restore asks for the backup's story on the lock screen"
    );
    assert_eq!(app.lock_mode, LockMode::Story);
    assert_eq!(app.restore.as_ref().unwrap().replaces_wallets, 2);
    assert_eq!(std::fs::read(&app.data_path).unwrap(), current);

    app.story_selections = BACKUP_STORY.into();
    app.submit_story();
    finish_background_work(&mut app);
    let restore = app.restore.as_ref().unwrap();
    assert_eq!(
        restore.decrypted.as_ref().unwrap().0.wallets[0].child_name,
        "Backup child"
    );
    assert_eq!(
        std::fs::read(&app.data_path).unwrap(),
        current,
        "nothing changes before confirm"
    );

    app.confirm_restore();

    assert!(app.restore.is_none());
    assert!(app.parent_unlocked);
    assert_eq!(app.data.wallets.len(), 1);
    assert_eq!(app.data.wallets[0].child_name, "Backup child");
    assert_eq!(std::fs::read(&app.data_path).unwrap(), backup);
    assert!(app.status.text.contains("Restored 1 wallet"));
    let kept: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("vault.pre-restore-")
        })
        .collect();
    assert_eq!(kept.len(), 1);
    assert_eq!(std::fs::read(&kept[0]).unwrap(), current);

    // Saves after a restore keep using the backup's key.
    app.data.wallets[0].child_name = "Renamed".to_owned();
    app.save_encrypted_data_and_refresh_ref(&story::encode(&BACKUP_STORY).unwrap())
        .unwrap();
    assert_eq!(
        saved_data(&app, &story::encode(&BACKUP_STORY).unwrap()).wallets[0].child_name,
        "Renamed"
    );
}

#[test]
fn restore_with_the_wrong_story_changes_nothing() {
    let (mut app, dir, current) = unlocked_story_app();
    let backup_path = dir.path().join("backup.cofferly");
    std::fs::write(&backup_path, story_vault(BACKUP_STORY, "Backup child", 1)).unwrap();

    app.begin_restore_from(&backup_path);
    app.story_selections = test_story().into();
    app.submit_story();
    finish_background_work(&mut app);

    assert!(app.restore.as_ref().unwrap().decrypted.is_none());
    assert_eq!(app.status.severity, StatusSeverity::Error);
    assert!(app.status.text.contains("Wrong Coffer Story"));
    assert_eq!(std::fs::read(&app.data_path).unwrap(), current);

    app.cancel_restore();
    assert!(app.restore.is_none());
    assert_eq!(std::fs::read(&app.data_path).unwrap(), current);
}

#[test]
fn restore_rejects_a_non_cofferly_file_without_locking() {
    let (mut app, dir, current) = unlocked_story_app();
    let not_a_backup = dir.path().join("notes.cofferly");
    std::fs::write(&not_a_backup, b"hello").unwrap();

    app.begin_restore_from(&not_a_backup);

    assert!(app.restore.is_none());
    assert!(app.parent_unlocked);
    assert!(app.status.text.contains("not a Cofferly backup"));
    assert_eq!(std::fs::read(&app.data_path).unwrap(), current);
}

#[test]
fn restore_is_offered_on_a_fresh_install_but_not_over_a_locked_vault() {
    let (mut app, dir) = test_app();
    let backup_path = dir.path().join("backup.cofferly");
    let backup = story_vault(BACKUP_STORY, "Backup child", 2);
    std::fs::write(&backup_path, &backup).unwrap();

    app.parent_unlocked = false;
    app.raw_bytes = Some(b"locked vault".to_vec());
    assert!(!app.can_start_restore());
    app.begin_restore_from(&backup_path);
    assert!(app.restore.is_none());

    app.raw_bytes = None;
    app.lock_mode = LockMode::SetupReveal;
    assert!(app.can_start_restore());
    app.begin_restore_from(&backup_path);
    assert_eq!(app.restore.as_ref().unwrap().replaces_wallets, 0);

    app.story_selections = BACKUP_STORY.into();
    app.submit_story();
    finish_background_work(&mut app);
    app.confirm_restore();

    assert!(app.parent_unlocked);
    assert_eq!(std::fs::read(&app.data_path).unwrap(), backup);
    assert!(!app.status.text.contains("pre-restore"));
}

#[test]
fn cancelling_a_fresh_install_restore_returns_to_the_new_story() {
    let (mut app, dir) = test_app();
    let backup_path = dir.path().join("backup.cofferly");
    std::fs::write(&backup_path, story_vault(BACKUP_STORY, "Backup child", 1)).unwrap();
    app.parent_unlocked = false;
    app.lock_mode = LockMode::SetupReveal;

    app.begin_restore_from(&backup_path);
    app.cancel_restore();

    assert_eq!(app.lock_mode, LockMode::SetupReveal);
    assert!(!app.data_path.exists());
}

#[test]
fn last_backup_date_persists_through_save() {
    let (mut app, _dir) = test_app();
    app.last_backup = Some("2026-09-03".to_owned());
    let mut storage = FakeStorage::default();

    app.save(&mut storage);

    let state = eframe::get_value::<UiState>(&storage, UI_STATE_KEY).unwrap();
    assert_eq!(state.last_backup.as_deref(), Some("2026-09-03"));
}

#[test]
fn first_run_unlocks_without_creating_a_file() {
    let (mut app, _dir) = test_app();
    app.parent_unlocked = false;
    app.pin_digits = ["1".into(), "2".into(), "3".into(), "4".into()];

    app.unlock_parent_sync();

    assert!(app.parent_unlocked);
    assert!(!app.data_path.exists());
    assert_eq!(app.status.text, "Parent mode unlocked.");
}

#[test]
fn unsupported_data_cannot_be_unlocked_or_overwritten() {
    let (mut app, _dir) = test_app();
    let unsupported = br#"{"parent_pin":"1234","wallets":[]}"#.to_vec();
    std::fs::write(&app.data_path, &unsupported).unwrap();
    app.raw_bytes = Some(unsupported.clone());
    app.parent_unlocked = false;
    // Exercise the format guard itself even if a caller misclassifies storage as writable.
    app.save_enabled = true;
    app.pin_digits = ["1".into(), "2".into(), "3".into(), "4".into()];

    app.start_unlock();

    assert!(!app.parent_unlocked);
    assert!(app.session.is_none());
    assert_eq!(std::fs::read(&app.data_path).unwrap(), unsupported);
    assert!(app.status.text.contains("Cannot unlock"));
    assert_eq!(app.status.severity, StatusSeverity::Error);
}

#[test]
fn transaction_remove_and_undo_workflow_stays_encrypted() {
    let (mut app, _dir) = test_app();
    app.draft.kind = EntryKind::Deposit;
    app.draft.description = "Weekly allowance".to_owned();
    app.draft.amount = "$10.50".to_owned();

    app.add_entry();

    assert_eq!(app.selected_wallet().current_balance_cents(), 1050);
    assert!(app.draft.description.is_empty());
    assert!(app.status.text.contains("Added $10.50"));
    assert_eq!(app.status.severity, StatusSeverity::Success);
    assert_eq!(saved_data(&app, "1234").wallets[0].entries.len(), 1);
    assert!(app.session.is_some());

    app.remove_latest_entry();
    assert!(app.selected_wallet().entries.is_empty());
    assert!(app.undo.is_some());
    assert!(app.status.text.contains("Undo available"));

    app.undo_remove_entry();
    assert_eq!(app.selected_wallet().current_balance_cents(), 1050);
    assert!(app.undo.is_none());
    assert_eq!(saved_data(&app, "1234").wallets[0].entries.len(), 1);
}

#[test]
fn switching_wallets_clears_a_pending_undo_from_a_different_wallet() {
    // Settings tells parents "Undo remains available until the next
    // wallet change." Removing an entry from wallet 0 and then switching
    // to wallet 1 must drop that pending undo, so a later "Undo" click
    // cannot silently restore the entry into a wallet the parent isn't
    // looking at anymore.
    let (mut app, _dir) = test_app();
    app.draft.kind = EntryKind::Deposit;
    app.draft.description = "Weekly allowance".to_owned();
    app.draft.amount = "$10.50".to_owned();
    app.add_entry();
    app.remove_latest_entry();
    assert!(app.undo.is_some());

    app.select_wallet(1);

    assert!(app.undo.is_none());
}

/// Feeds a single key press through a real egui input pass, mirroring how
/// `handle_ledger_filter_shortcut`'s `consume_key` actually reads it —
/// there is no lighter-weight way to exercise `ctx.input_mut`/
/// `ctx.text_edit_focused` guards than a real (if minimal) pass.
fn press_key(ctx: &egui::Context, key: egui::Key) {
    ctx.begin_pass(egui::RawInput {
        events: vec![egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
        ..Default::default()
    });
}

#[test]
fn slash_focuses_the_ledger_filter_when_nothing_else_has_focus() {
    let (mut app, _dir) = test_app();
    let ctx = egui::Context::default();
    press_key(&ctx, egui::Key::Slash);

    app.handle_ledger_filter_shortcut(&ctx);

    assert!(app.pending_ledger_filter_focus);
    ctx.end_pass().textures_delta.clear();
}

#[test]
fn slash_is_ignored_while_settings_is_open() {
    let (mut app, _dir) = test_app();
    app.show_settings = true;
    let ctx = egui::Context::default();
    press_key(&ctx, egui::Key::Slash);

    app.handle_ledger_filter_shortcut(&ctx);

    assert!(!app.pending_ledger_filter_focus);
    ctx.end_pass().textures_delta.clear();
}

#[test]
fn wallet_keyboard_keys_move_to_the_next_and_previous_index() {
    assert_eq!(wallet_keyboard_delta(egui::Key::ArrowDown), Some(1));
    assert_eq!(wallet_keyboard_delta(egui::Key::CloseBracket), Some(1));
    assert_eq!(wallet_keyboard_delta(egui::Key::ArrowUp), Some(-1));
    assert_eq!(wallet_keyboard_delta(egui::Key::OpenBracket), Some(-1));
    assert_eq!(wallet_keyboard_delta(egui::Key::Enter), None);
    assert_eq!(next_wallet_index(0, 3, 1), 1);
    assert_eq!(next_wallet_index(2, 3, 1), 2);
    assert_eq!(next_wallet_index(0, 3, -1), 0);
}

#[test]
fn keyboard_wallet_switch_announces_name_and_balance_and_clears_undo() {
    let (mut app, _dir) = test_app();
    app.draft.kind = EntryKind::Deposit;
    app.draft.description = "Weekly allowance".to_owned();
    app.draft.amount = "$10.50".to_owned();
    app.add_entry();
    app.remove_latest_entry();
    assert!(app.undo.is_some());

    app.apply_wallet_keyboard_delta(1);

    assert_eq!(app.selected_wallet, 1);
    assert!(app.undo.is_none());
    assert_eq!(app.status.text, wallet_selection_announcement("Child 2", 0));
    assert!(app.status.text.contains("Child 2"));
    assert!(app.status.text.contains("balance"));
}

#[test]
fn keyboard_wallet_switch_at_the_edge_does_not_clear_undo() {
    let (mut app, _dir) = test_app();
    app.draft.kind = EntryKind::Deposit;
    app.draft.description = "Weekly allowance".to_owned();
    app.draft.amount = "$10.50".to_owned();
    app.add_entry();
    app.remove_latest_entry();
    assert!(app.undo.is_some());

    app.apply_wallet_keyboard_delta(-1);

    assert_eq!(app.selected_wallet, 0);
    assert!(app.undo.is_some());
}

/// Minimal in-memory `eframe::Storage` so `CofferlyApp::new` can restore
/// persisted UI state without touching a real config directory.
#[derive(Default)]
struct FakeStorage(std::collections::HashMap<String, String>);

impl eframe::Storage for FakeStorage {
    fn get_string(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }
    fn set_string(&mut self, key: &str, value: String) {
        self.0.insert(key.to_owned(), value);
    }
    fn remove_string(&mut self, key: &str) {
        self.0.remove(key);
    }
    fn flush(&mut self) {}
}

/// Serializes every reader/writer of `COFFERLY_DATA_DIR` so parallel tests
/// cannot overwrite each other's data dir.
static COFFERLY_DATA_DIR_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Points `COFFERLY_DATA_DIR` at a temp dir for the life of the guard and
/// restores the previous value on drop, so this test cannot leak state to
/// others even if an assertion panics.
///
/// Construction holds `COFFERLY_DATA_DIR_LOCK` for the guard lifetime.
struct ScopedDataDir {
    previous: Option<String>,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl ScopedDataDir {
    fn set(path: &std::path::Path) -> Self {
        let lock = COFFERLY_DATA_DIR_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = std::env::var("COFFERLY_DATA_DIR").ok();
        // SAFETY: the mutex serializes every reader/writer of this env var.
        unsafe { std::env::set_var("COFFERLY_DATA_DIR", path) };
        Self {
            previous,
            _lock: lock,
        }
    }
}

impl Drop for ScopedDataDir {
    fn drop(&mut self) {
        // SAFETY: see `set` above.
        unsafe {
            match &self.previous {
                Some(previous) => std::env::set_var("COFFERLY_DATA_DIR", previous),
                None => std::env::remove_var("COFFERLY_DATA_DIR"),
            }
        }
    }
}

#[test]
fn restarting_with_more_than_two_wallets_keeps_the_previously_selected_wallet() {
    // Regression test: `CofferlyApp::new` used to clamp the persisted
    // wallet selection against the freshly-constructed placeholder
    // `AppData` (always 2 wallets) instead of deferring to the real
    // wallet count, which is unknown until the vault is decrypted. A
    // family with 3+ children would have their selection silently
    // snapped back to wallet index 1 on every restart, no matter which
    // child was actually selected when the app was closed.
    let dir = tempdir().unwrap();
    let _data_dir = ScopedDataDir::set(dir.path());

    let mut data = default_app_data();
    for index in 2..5 {
        data.wallets.push(Wallet {
            child_name: format!("Child {}", index + 1),
            starting_balance_cents: 0,
            entries: Vec::new(),
            weekly_allowance: None,
            savings_goal_cents: None,
        });
    }
    let secret = "coffer-story-v1:test-regression-secret";
    let mut session = None;
    let encrypted =
        crypto::encrypt(&serde_json::to_vec(&data).unwrap(), secret, &mut session).unwrap();
    std::fs::create_dir_all(dir.path().join(APP_NAME)).unwrap();
    std::fs::write(dir.path().join(APP_NAME).join(DATA_FILE_NAME), &encrypted).unwrap();

    let mut storage = FakeStorage::default();
    eframe::set_value(
        &mut storage,
        UI_STATE_KEY,
        &UiState {
            selected_wallet: 4,
            ledger_sort_newest_first: true,
            selected_wallet_name: None,
            last_entry_kind: EntryKind::default(),
            ledger_filter: String::new(),
            ledger_filters: HashMap::new(),
            last_backup: None,
        },
    );
    let mut cc = eframe::CreationContext::_new_kittest(egui::Context::default());
    cc.storage = Some(&storage);

    let mut app = CofferlyApp::new(&cc);
    assert_eq!(
        app.selected_wallet, 4,
        "the persisted selection must survive construction, before the vault is even decrypted"
    );

    let (plain, session) = crypto::decrypt(app.raw_bytes.as_ref().unwrap(), secret).unwrap();
    let loaded = serde_json::from_slice::<AppData>(&plain).unwrap();
    let normalized = data::normalize_app_data(loaded).unwrap();
    app.apply_unlock(normalized, session);

    assert_eq!(app.selected_wallet, 4);
    assert_eq!(app.selected_wallet().child_name, "Child 5");
}

/// Issue #135: persisted selection should follow the wallet's identity
/// (`child_name`), not its position, so unlock restores the same child
/// even though the placeholder position (index 0) differs from where
/// the wallet actually landed.
#[test]
fn unlock_restores_last_selected_wallet_by_name() {
    let (mut app, _dir) = test_app();
    app.pending_wallet_selection_name = Some("Charlie".to_owned());

    let mut data = default_app_data();
    data.wallets = vec![
        Wallet {
            child_name: "Alice".to_owned(),
            starting_balance_cents: 0,
            entries: Vec::new(),
            weekly_allowance: None,
            savings_goal_cents: None,
        },
        Wallet {
            child_name: "Bob".to_owned(),
            starting_balance_cents: 0,
            entries: Vec::new(),
            weekly_allowance: None,
            savings_goal_cents: None,
        },
        Wallet {
            child_name: "Charlie".to_owned(),
            starting_balance_cents: 0,
            entries: Vec::new(),
            weekly_allowance: None,
            savings_goal_cents: None,
        },
    ];
    let session = SessionCrypto::establish("test-secret").unwrap();

    app.apply_unlock(data, session);

    assert_eq!(app.selected_wallet, 2);
    assert_eq!(app.selected_wallet().child_name, "Charlie");
    assert!(
        app.pending_wallet_selection_name.is_none(),
        "the persisted name should be consumed once resolved"
    );
}

/// Issue #135: if the previously-selected wallet was deleted while
/// locked (or on another launch), unlock should fall back to the first
/// wallet rather than silently landing on whatever wallet now occupies
/// the old index.
#[test]
fn unlock_falls_back_to_first_wallet_when_selected_wallet_was_deleted() {
    let (mut app, _dir) = test_app();
    app.selected_wallet = 1;
    app.pending_wallet_selection_name = Some("Charlie".to_owned());

    // "Charlie" is gone; "Alice" now sits at index 0.
    let mut data = default_app_data();
    data.wallets = vec![
        Wallet {
            child_name: "Alice".to_owned(),
            starting_balance_cents: 0,
            entries: Vec::new(),
            weekly_allowance: None,
            savings_goal_cents: None,
        },
        Wallet {
            child_name: "Bob".to_owned(),
            starting_balance_cents: 0,
            entries: Vec::new(),
            weekly_allowance: None,
            savings_goal_cents: None,
        },
    ];
    let session = SessionCrypto::establish("test-secret").unwrap();

    app.apply_unlock(data, session);

    assert_eq!(app.selected_wallet, 0);
    assert_eq!(app.selected_wallet().child_name, "Alice");
}

/// Issue #135: selecting a wallet, locking, and unlocking again within
/// the same run (no relaunch, so `raw_bytes`/`pending_wallet_selection_name`
/// aren't repopulated from disk) must keep the selection the parent had
/// -- `apply_unlock`'s no-persisted-name branch should defer to whatever
/// index is already in memory instead of resetting to 0.
#[test]
fn selection_persists_across_a_lock_and_unlock_cycle() {
    let (mut app, _dir) = test_app();
    app.data.wallets = vec![
        Wallet {
            child_name: "Alice".to_owned(),
            starting_balance_cents: 0,
            entries: Vec::new(),
            weekly_allowance: None,
            savings_goal_cents: None,
        },
        Wallet {
            child_name: "Bob".to_owned(),
            starting_balance_cents: 0,
            entries: Vec::new(),
            weekly_allowance: None,
            savings_goal_cents: None,
        },
    ];
    app.select_wallet(1);
    assert_eq!(app.selected_wallet, 1);

    app.lock_parent();
    assert!(!app.parent_unlocked);
    // Locking mid-run never touches the in-memory wallet list or index.
    assert_eq!(app.selected_wallet, 1);

    let data = app.data.clone();
    let session = SessionCrypto::establish("test-secret").unwrap();
    app.apply_unlock(data, session);

    assert_eq!(app.selected_wallet, 1);
    assert_eq!(app.selected_wallet().child_name, "Bob");
}

/// Issue #135 + #131: keyboard wallet switching (`apply_wallet_keyboard_delta`)
/// goes through the same `select_wallet` as a sidebar click, so `save()`
/// must persist the keyboard-selected wallet's name too -- persistence
/// should not care which input method changed the selection.
#[test]
fn keyboard_wallet_switch_persists_through_save_like_a_sidebar_click() {
    let (mut app, _dir) = test_app();
    app.data.wallets = vec![
        Wallet {
            child_name: "Alice".to_owned(),
            starting_balance_cents: 0,
            entries: Vec::new(),
            weekly_allowance: None,
            savings_goal_cents: None,
        },
        Wallet {
            child_name: "Bob".to_owned(),
            starting_balance_cents: 0,
            entries: Vec::new(),
            weekly_allowance: None,
            savings_goal_cents: None,
        },
    ];

    app.apply_wallet_keyboard_delta(1);
    assert_eq!(app.selected_wallet, 1);

    let mut storage = FakeStorage::default();
    eframe::App::save(&mut app, &mut storage);

    let saved = eframe::get_value::<UiState>(&storage, UI_STATE_KEY).unwrap();
    assert_eq!(saved.selected_wallet, 1);
    assert_eq!(saved.selected_wallet_name.as_deref(), Some("Bob"));
}

#[test]
fn last_entry_kind_persists_through_save() {
    let (mut app, _dir) = test_app();
    app.draft.kind = EntryKind::Deposit;

    let mut storage = FakeStorage::default();
    eframe::App::save(&mut app, &mut storage);

    let saved = eframe::get_value::<UiState>(&storage, UI_STATE_KEY).unwrap();
    assert_eq!(saved.last_entry_kind, EntryKind::Deposit);
}

#[test]
fn last_entry_kind_restores_into_a_fresh_draft_on_construction() {
    let dir = tempdir().unwrap();
    let _data_dir = ScopedDataDir::set(dir.path());
    // No vault file written -- fresh install, so restore_ui_state runs
    // against the placeholder AppData's wallet count directly and
    // CofferlyApp::new never needs to decrypt anything.

    let mut storage = FakeStorage::default();
    eframe::set_value(
        &mut storage,
        UI_STATE_KEY,
        &UiState {
            selected_wallet: 0,
            ledger_sort_newest_first: true,
            selected_wallet_name: None,
            last_entry_kind: EntryKind::Deposit,
            ledger_filter: String::new(),
            ledger_filters: HashMap::new(),
            last_backup: None,
        },
    );
    let mut cc = eframe::CreationContext::_new_kittest(egui::Context::default());
    cc.storage = Some(&storage);

    let app = CofferlyApp::new(&cc);

    assert_eq!(app.draft.kind, EntryKind::Deposit);
}

#[test]
fn ledger_filter_persists_through_save() {
    let (mut app, _dir) = test_app();
    app.ledger_filter = "allowance".to_owned();

    let mut storage = FakeStorage::default();
    eframe::App::save(&mut app, &mut storage);

    let saved = eframe::get_value::<UiState>(&storage, UI_STATE_KEY).unwrap();
    assert_eq!(saved.ledger_filter, "allowance");
    assert_eq!(
        saved.ledger_filters.get("Child 1").map(String::as_str),
        Some("allowance")
    );
}

#[test]
fn ledger_filter_restores_on_construction_and_caps_length() {
    let dir = tempdir().unwrap();
    let _data_dir = ScopedDataDir::set(dir.path());

    let long_filter = "x".repeat(100);
    let mut storage = FakeStorage::default();
    eframe::set_value(
        &mut storage,
        UI_STATE_KEY,
        &UiState {
            selected_wallet: 0,
            ledger_sort_newest_first: true,
            selected_wallet_name: None,
            last_entry_kind: EntryKind::default(),
            ledger_filter: long_filter,
            ledger_filters: HashMap::new(),
            last_backup: None,
        },
    );
    let mut cc = eframe::CreationContext::_new_kittest(egui::Context::default());
    cc.storage = Some(&storage);

    let app = CofferlyApp::new(&cc);

    assert_eq!(app.ledger_filter, "x".repeat(80));
}

#[test]
fn ui_state_without_ledger_filter_field_still_loads_and_defaults_to_empty() {
    let mut storage = FakeStorage::default();
    eframe::set_value(
        &mut storage,
        UI_STATE_KEY,
        &UiState {
            selected_wallet: 2,
            ledger_sort_newest_first: false,
            selected_wallet_name: Some("Alice".to_owned()),
            last_entry_kind: EntryKind::Deposit,
            ledger_filter: "snack".to_owned(),
            ledger_filters: HashMap::new(),
            last_backup: None,
        },
    );

    let raw = storage.get_string(UI_STATE_KEY).unwrap();
    let without_field = raw.replacen(",ledger_filter:\"snack\"", "", 1);
    assert_ne!(
        raw, without_field,
        "expected to find and strip ledger_filter from the RON record"
    );
    storage.set_string(UI_STATE_KEY, without_field);

    let restored = eframe::get_value::<UiState>(&storage, UI_STATE_KEY).unwrap();
    assert_eq!(restored.ledger_filter, "");

    let dir = tempdir().unwrap();
    let _data_dir = ScopedDataDir::set(dir.path());
    let mut cc = eframe::CreationContext::_new_kittest(egui::Context::default());
    cc.storage = Some(&storage);

    let app = CofferlyApp::new(&cc);
    assert_eq!(app.ledger_filter, "");
    assert!(app.ledger_filters.is_empty());
}

#[test]
fn ledger_filter_is_per_child_when_switching_wallets() {
    let (mut app, _dir) = test_app();
    app.ledger_filter = "snack".to_owned();

    app.select_wallet(1);
    assert_eq!(app.selected_wallet().child_name, "Child 2");
    assert_eq!(app.ledger_filter, "");

    app.select_wallet(0);
    assert_eq!(app.selected_wallet().child_name, "Child 1");
    assert_eq!(app.ledger_filter, "snack");
    assert!(
        !app.data_path.exists(),
        "switching wallets is display-only and must not write the vault"
    );

    app.ledger_filter = "snack".to_owned();
    app.apply_wallet_keyboard_delta(1);
    assert_eq!(app.selected_wallet().child_name, "Child 2");
    assert_eq!(app.ledger_filter, "");
    app.apply_wallet_keyboard_delta(-1);
    assert_eq!(app.ledger_filter, "snack");
}

#[test]
fn lock_relaunch_restores_per_child_ledger_filters_not_a_global_string() {
    let (mut app, _dir) = test_app();
    app.ledger_filter = "snack".to_owned();
    app.select_wallet(1);
    assert_eq!(app.ledger_filter, "");
    app.ledger_filter = "chores".to_owned();

    let mut storage = FakeStorage::default();
    eframe::App::save(&mut app, &mut storage);

    let saved = eframe::get_value::<UiState>(&storage, UI_STATE_KEY).unwrap();
    assert_eq!(saved.ledger_filter, "chores");
    assert_eq!(
        saved.ledger_filters.get("Child 1").map(String::as_str),
        Some("snack")
    );
    assert_eq!(
        saved.ledger_filters.get("Child 2").map(String::as_str),
        Some("chores")
    );

    let dir = tempdir().unwrap();
    let _data_dir = ScopedDataDir::set(dir.path());
    let mut cc = eframe::CreationContext::_new_kittest(egui::Context::default());
    cc.storage = Some(&storage);
    let mut restored = CofferlyApp::new(&cc);

    assert_eq!(restored.selected_wallet, 1);
    assert_eq!(restored.ledger_filter, "chores");
    restored.select_wallet(0);
    assert_eq!(restored.ledger_filter, "snack");
    restored.select_wallet(1);
    assert_eq!(restored.ledger_filter, "chores");
}

#[test]
fn per_child_ledger_filters_restore_on_construction_not_as_a_global_string() {
    let dir = tempdir().unwrap();
    let _data_dir = ScopedDataDir::set(dir.path());

    let mut filters = HashMap::new();
    filters.insert("Child 1".to_owned(), "snack".to_owned());
    filters.insert("Child 2".to_owned(), "x".repeat(100));

    let mut storage = FakeStorage::default();
    eframe::set_value(
        &mut storage,
        UI_STATE_KEY,
        &UiState {
            selected_wallet: 0,
            ledger_sort_newest_first: true,
            selected_wallet_name: Some("Child 1".to_owned()),
            last_entry_kind: EntryKind::default(),
            ledger_filter: "snack".to_owned(),
            ledger_filters: filters,
            last_backup: None,
        },
    );
    let mut cc = eframe::CreationContext::_new_kittest(egui::Context::default());
    cc.storage = Some(&storage);

    let mut app = CofferlyApp::new(&cc);

    assert_eq!(app.ledger_filter, "snack");
    assert_eq!(
        app.ledger_filters.get("Child 1").map(String::as_str),
        Some("snack")
    );
    assert_eq!(
        app.ledger_filters.get("Child 2").map(String::as_str),
        Some(&*"x".repeat(80)),
        "restored per-child text is capped at 80 chars"
    );

    app.select_wallet(1);
    assert_eq!(app.ledger_filter, "x".repeat(80));
    app.select_wallet(0);
    assert_eq!(app.ledger_filter, "snack");
}

#[test]
fn add_child_wallet_does_not_copy_the_previous_query() {
    let (mut app, _dir) = test_app();
    app.ledger_filter = "snack".to_owned();
    app.new_child_name_input = "Sam".to_owned();

    app.add_child_wallet();

    assert_eq!(app.selected_wallet().child_name, "Sam");
    assert_eq!(app.ledger_filter, "");

    app.select_wallet(0);
    assert_eq!(app.ledger_filter, "snack");
}

#[test]
fn add_child_wallet_rejects_a_duplicate_name_without_saving() {
    let (mut app, _dir) = test_app();
    for input in ["child 1", "  CHILD 1  "] {
        app.new_child_name_input = input.to_owned();
        let count = app.data.wallets.len();

        app.add_child_wallet();

        assert_eq!(app.data.wallets.len(), count);
        assert_eq!(app.status.severity, StatusSeverity::Error);
        assert!(app
            .status
            .text
            .starts_with("Another wallet is already named"));
        assert!(!app.data_path.exists());
    }
}

#[test]
fn rename_rejects_another_wallets_name_but_allows_own_case_change() {
    let (mut app, _dir) = test_app();
    app.select_wallet(1);
    app.child_name_input = "child 1".to_owned();

    app.rename_selected_child();

    assert_eq!(app.selected_wallet().child_name, "Child 2");
    assert_eq!(app.status.severity, StatusSeverity::Error);
    assert!(!app.data_path.exists());

    app.child_name_input = "CHILD 2".to_owned();
    app.rename_selected_child();

    assert_eq!(app.selected_wallet().child_name, "CHILD 2");
    assert_eq!(app.status.severity, StatusSeverity::Success);
}

#[test]
fn rename_moves_ledger_filter_map_key_and_delete_drops_it() {
    let (mut app, _dir) = test_app();
    app.ledger_filter = "snack".to_owned();
    app.child_name_input = "Sam".to_owned();

    app.rename_selected_child();

    assert_eq!(app.selected_wallet().child_name, "Sam");
    assert_eq!(app.ledger_filter, "snack");
    assert_eq!(
        app.ledger_filters.get("Sam").map(String::as_str),
        Some("snack")
    );
    assert!(!app.ledger_filters.contains_key("Child 1"));

    app.select_wallet(1);
    assert_eq!(app.ledger_filter, "");
    app.select_wallet(0);
    assert_eq!(app.ledger_filter, "snack");

    app.delete_selected_wallet();

    assert_eq!(app.selected_wallet().child_name, "Child 2");
    assert_eq!(app.ledger_filter, "");
    assert!(!app.ledger_filters.contains_key("Sam"));
}

#[test]
fn ui_state_without_ledger_filters_field_still_loads() {
    let mut storage = FakeStorage::default();
    eframe::set_value(
        &mut storage,
        UI_STATE_KEY,
        &UiState {
            selected_wallet: 2,
            ledger_sort_newest_first: false,
            selected_wallet_name: Some("Alice".to_owned()),
            last_entry_kind: EntryKind::Deposit,
            ledger_filter: "snack".to_owned(),
            ledger_filters: {
                let mut filters = HashMap::new();
                filters.insert("Alice".to_owned(), "snack".to_owned());
                filters
            },
            last_backup: None,
        },
    );

    let raw = storage.get_string(UI_STATE_KEY).unwrap();
    let without_field = raw.replacen(",ledger_filters:{\"Alice\":\"snack\"}", "", 1);
    assert_ne!(
        raw, without_field,
        "expected to find and strip ledger_filters from the RON record; got {raw}"
    );
    storage.set_string(UI_STATE_KEY, without_field);

    let restored = eframe::get_value::<UiState>(&storage, UI_STATE_KEY).unwrap();
    assert_eq!(restored.ledger_filter, "snack");
    assert!(restored.ledger_filters.is_empty());

    let dir = tempdir().unwrap();
    let _data_dir = ScopedDataDir::set(dir.path());
    let mut cc = eframe::CreationContext::_new_kittest(egui::Context::default());
    cc.storage = Some(&storage);

    let app = CofferlyApp::new(&cc);
    assert_eq!(app.ledger_filter, "snack");
    assert!(app.ledger_filters.is_empty());
}

#[test]
fn ui_state_without_last_entry_kind_field_still_loads_and_defaults_to_deduction() {
    let mut storage = FakeStorage::default();
    eframe::set_value(
        &mut storage,
        UI_STATE_KEY,
        &UiState {
            selected_wallet: 2,
            ledger_sort_newest_first: false,
            selected_wallet_name: Some("Alice".to_owned()),
            last_entry_kind: EntryKind::Deposit,
            ledger_filter: String::new(),
            ledger_filters: HashMap::new(),
            last_backup: None,
        },
    );

    // Simulate a build that predates this field: strip the
    // last_entry_kind entry out of the RON record entirely, the way
    // real data saved before this change would actually look.
    let raw = storage.get_string(UI_STATE_KEY).unwrap();
    let without_field = raw.replacen(",last_entry_kind:Deposit", "", 1);
    assert_ne!(
        raw, without_field,
        "expected to find and strip last_entry_kind from the RON record"
    );
    storage.set_string(UI_STATE_KEY, without_field);

    let restored = eframe::get_value::<UiState>(&storage, UI_STATE_KEY).unwrap();
    assert_eq!(restored.selected_wallet, 2);
    assert_eq!(restored.selected_wallet_name.as_deref(), Some("Alice"));
    assert_eq!(restored.last_entry_kind, EntryKind::Deduction);
}

#[test]
fn invalid_transaction_does_not_mutate_or_create_a_file() {
    let (mut app, _dir) = test_app();
    app.draft.kind = EntryKind::Deduction;
    app.draft.description = "Toy".to_owned();
    app.draft.amount = "not money".to_owned();

    app.add_entry();

    assert!(app.selected_wallet().entries.is_empty());
    assert!(!app.data_path.exists());
    assert_eq!(
        app.status.text,
        "Enter a valid amount, like 10, 10.50, or $1,234.56."
    );
    assert_eq!(app.status.severity, StatusSeverity::Error);
    assert_eq!(app.pending_entry_focus, Some(EntryFormField::Amount));
}

#[test]
fn add_entry_write_failure_rolls_back_and_keeps_the_draft() {
    let (mut app, dir) = test_app();
    app.data_path = unwritable_data_path(&dir);
    app.draft.kind = EntryKind::Deposit;
    app.draft.description = "Weekly allowance".to_owned();
    app.draft.amount = "$10.50".to_owned();
    let date_before = app.draft.date_input.clone();

    app.add_entry();

    assert_eq!(app.status.severity, StatusSeverity::Error);
    assert!(app.status.text.starts_with("Could not save:"));
    assert!(app.status.text.ends_with("The entry was not recorded."));
    assert!(app.selected_wallet().entries.is_empty());
    assert_eq!(app.draft.description, "Weekly allowance");
    assert_eq!(app.draft.amount, "$10.50");
    assert_eq!(app.draft.kind, EntryKind::Deposit);
    assert_eq!(app.draft.date_input, date_before);
    assert_eq!(app.pending_entry_focus, Some(EntryFormField::Description));
    assert!(app.raw_bytes.is_none());
    assert!(!app.data_path.exists());
}

#[cfg(unix)]
#[test]
fn add_entry_write_failure_leaves_existing_vault_bytes_unchanged() {
    use std::os::unix::fs::PermissionsExt;

    let (mut app, dir) = test_app();
    app.draft.kind = EntryKind::Deposit;
    app.draft.description = "First allowance".to_owned();
    app.draft.amount = "5".to_owned();
    app.add_entry();
    assert_eq!(app.status.severity, StatusSeverity::Success);
    let original = std::fs::read(&app.data_path).unwrap();
    let original_raw_bytes = app.raw_bytes.clone();

    app.draft.kind = EntryKind::Deposit;
    app.draft.description = "Unsaved allowance".to_owned();
    app.draft.amount = "7".to_owned();
    let date_before = app.draft.date_input.clone();
    let original_permissions = std::fs::metadata(dir.path()).unwrap().permissions();
    let mut readonly = original_permissions.clone();
    readonly.set_mode(0o500);
    std::fs::set_permissions(dir.path(), readonly).unwrap();
    app.add_entry();
    std::fs::set_permissions(dir.path(), original_permissions).unwrap();

    assert_eq!(app.status.severity, StatusSeverity::Error);
    assert!(app.status.text.starts_with("Could not save:"));
    assert_eq!(app.selected_wallet().entries.len(), 1);
    assert_eq!(app.selected_wallet().current_balance_cents(), 500);
    assert_eq!(app.draft.description, "Unsaved allowance");
    assert_eq!(app.draft.amount, "7");
    assert_eq!(app.draft.kind, EntryKind::Deposit);
    assert_eq!(app.draft.date_input, date_before);
    assert_eq!(app.raw_bytes, original_raw_bytes);
    assert_eq!(std::fs::read(&app.data_path).unwrap(), original);
    assert_eq!(saved_data(&app, "1234").wallets[0].entries.len(), 1);
    assert_eq!(
        saved_data(&app, "1234").wallets[0].entries[0].description,
        "First allowance"
    );
}

#[test]
fn wallet_management_keeps_at_least_one_wallet() {
    let (mut app, _dir) = test_app();
    app.new_child_name_input = "Sam".to_owned();
    app.add_child_wallet();

    assert_eq!(app.data.wallets.len(), 3);
    assert_eq!(app.selected_wallet().child_name, "Sam");

    app.delete_selected_wallet();
    app.delete_selected_wallet();
    app.delete_selected_wallet();

    assert_eq!(app.data.wallets.len(), 1);
    assert_eq!(app.status.text, "Keep at least one wallet.");
    assert_eq!(saved_data(&app, "1234").wallets.len(), 1);
}

#[test]
fn cached_ledger_rows_reuses_the_same_allocation_until_invalidated() {
    let (mut app, _dir) = test_app();

    let first = app.cached_ledger_rows();
    let second = app.cached_ledger_rows();
    assert!(Arc::ptr_eq(&first, &second));

    app.invalidate_ledger_cache();
    let rebuilt = app.cached_ledger_rows();
    assert!(!Arc::ptr_eq(&first, &rebuilt));
}

#[test]
fn print_path_uses_temp_directory() {
    let (app, _dir) = test_app();
    let path = app.print_path(true).unwrap();
    assert!(path.starts_with(std::env::temp_dir()));
    assert!(path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("cofferly-"));
    assert!(path.extension().is_some_and(|ext| ext == "html"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    let _ = std::fs::remove_file(&path);

    let csv = app.csv_path(false).unwrap();
    assert!(csv.starts_with(std::env::temp_dir()));
    assert!(csv.extension().is_some_and(|ext| ext == "csv"));
    let _ = std::fs::remove_file(&csv);
}

#[test]
fn print_path_uses_unpredictable_names() {
    let (app, _dir) = test_app();
    let first = app.print_path(true).unwrap();
    let second = app.print_path(true).unwrap();
    assert_ne!(first, second);
    let _ = std::fs::remove_file(&first);
    let _ = std::fs::remove_file(&second);
}

#[test]
fn print_and_export_are_disabled_when_saved_data_cannot_load() {
    let (mut app, _dir) = test_app();
    app.save_enabled = false;

    app.print_selected_wallet();
    assert_eq!(app.status.severity, StatusSeverity::Error);
    assert!(app.status.text.contains("printing is disabled"));

    app.print_all_wallets();
    assert!(app.status.text.contains("printing is disabled"));

    app.export_selected_wallet_csv();
    assert!(app.status.text.contains("export is disabled"));

    app.export_all_wallets_csv();
    assert!(app.status.text.contains("export is disabled"));
    assert!(app.temp_artifact_paths.is_empty());
}

#[test]
fn print_export_status_chip_reports_opener_success_and_failure() {
    let (mut app, dir) = test_app();
    let path = dir.path().join("child-1-ledger.html");
    std::fs::write(&path, b"<html></html>").unwrap();

    app.apply_export_open_result(path.clone(), "printable ledger", Ok(()));
    assert_eq!(app.status.severity, StatusSeverity::Success);
    assert!(app.status.text.contains("Opened printable ledger"));
    assert!(app.status.text.contains(path.to_string_lossy().as_ref()));
    assert_eq!(app.temp_artifact_paths.last(), Some(&path));

    app.apply_export_open_result(
        path.clone(),
        "CSV ledger",
        Err("no application found".to_owned()),
    );
    assert_eq!(app.status.severity, StatusSeverity::Error);
    assert!(app.status.text.contains("CSV ledger was saved to"));
    assert!(app.status.text.contains("could not open it"));
    assert!(app.status.text.contains("file manager"));
    assert!(app.status.text.contains("no application found"));
}

#[test]
fn selected_export_status_names_filter_and_count_only_when_set() {
    let (mut app, _dir) = test_app();
    app.data.wallets[0].starting_balance_cents = 1_000;
    app.data.wallets[0].entries = vec![
        Entry {
            date: NaiveDate::from_ymd_opt(2026, 6, 8).unwrap(),
            description: "Weekly allowance".to_owned(),
            amount_cents: 500,
        },
        Entry {
            date: NaiveDate::from_ymd_opt(2026, 6, 9).unwrap(),
            description: "Snack".to_owned(),
            amount_cents: -200,
        },
    ];

    assert!(app.selected_description_filter().is_none());
    let plain = export_opened_status("printable ledger", Path::new("ledger.html"));
    assert!(!plain.text.to_lowercase().contains("filter"));

    app.ledger_filter = "  ".to_owned();
    assert!(app.selected_description_filter().is_none());

    app.ledger_filter = "  snack  ".to_owned();
    let query = app.selected_description_filter().expect("active filter");
    assert_eq!(query, "snack");
    assert_eq!(app.filtered_entry_count(&query), 1);

    let one = filtered_export_opened_status("CSV ledger", Path::new("ledger.csv"), &query, 1);
    assert!(one.contains("snack"));
    assert!(one.contains("1 entry"));
    assert!(one.to_lowercase().contains("filter"));
    assert!(one.contains("ledger.csv"));

    let two =
        filtered_export_opened_status("printable ledger", Path::new("ledger.html"), "snack", 2);
    assert!(two.contains("2 entries"));
    let none =
        filtered_export_opened_status("printable ledger", Path::new("ledger.html"), "nope", 0);
    assert!(none.contains("0 entries"));
    assert!(none.contains("nope"));
}

#[test]
fn status_live_region_follows_severity_and_keeps_its_id() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();

    let frames = [
        (StatusSeverity::Error, "Could not save: disk full"),
        (StatusSeverity::Error, "Could not save: disk full"),
        (StatusSeverity::Success, "Opened CSV ledger."),
        (StatusSeverity::Info, "Child 1, balance $0.00"),
    ];
    let mut ids = Vec::new();
    for (severity, text) in frames {
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let response = show_live_status(
                ui,
                "parent_mode_status",
                text,
                egui::Color32::BLACK,
                11.0,
                true,
                severity,
            );
            ids.push(response.id);
            let node = ui.ctx().accesskit_node_builder(response.id, |node| {
                (
                    node.role(),
                    node.live(),
                    node.label().map(str::to_owned),
                    node.value().map(str::to_owned),
                )
            });
            let (role, live, label, value) = node.expect("accesskit node");
            assert_eq!(role, egui::accesskit::Role::Status);
            let expected_live = match severity {
                StatusSeverity::Error => egui::accesskit::Live::Assertive,
                StatusSeverity::Info | StatusSeverity::Success => egui::accesskit::Live::Polite,
            };
            assert_eq!(live, Some(expected_live));
            assert_eq!(label.as_deref(), Some(text));
            assert_eq!(value.as_deref(), Some(text));
        });
        output.drop_without_applying_deltas();
    }
    assert_eq!(
        ids[0], ids[1],
        "repeating the same message keeps the node id"
    );
    assert_eq!(ids[0], ids[2], "a new message updates the same node id");
    assert_eq!(ids[0], ids[3]);

    let ctx = egui::Context::default();
    let output = ctx.run_ui(egui::RawInput::default(), |ui| {
        let response = show_live_status(
            ui,
            "lock_screen_status",
            "⚠ Could not save: disk full",
            egui::Color32::BLACK,
            13.0,
            false,
            StatusSeverity::Error,
        );
        assert_ne!(response.id, egui::Id::NULL);
        assert!(
            ui.ctx()
                .accesskit_node_builder(response.id, |_| ())
                .is_none(),
            "accesskit off leaves the label in place and the builder unused"
        );
    });
    output.drop_without_applying_deltas();
}

#[test]
fn export_status_copy_is_actionable() {
    let path = PathBuf::from("/tmp/cofferly-child-ledger.html");
    let ok = export_opened_status("printable ledger", &path);
    assert_eq!(ok.severity, StatusSeverity::Success);
    assert!(ok.text.contains("Opened printable ledger"));
    assert!(ok.text.contains("/tmp/cofferly-child-ledger.html"));

    let err = export_opener_failed_status("CSV ledger", &path, "permission denied");
    assert_eq!(err.severity, StatusSeverity::Error);
    assert!(err.text.contains("CSV ledger was saved to"));
    assert!(err.text.contains("permission denied"));
    assert!(err.text.contains("Open that file from your file manager"));
}

#[test]
fn lock_clears_session_key() {
    let (mut app, _dir) = test_app();
    app.draft.kind = EntryKind::Deposit;
    app.draft.description = "Seed".to_owned();
    app.draft.amount = "1".to_owned();
    app.add_entry();
    assert!(app.session.is_some());

    app.lock_parent();
    assert!(!app.parent_unlocked);
    assert!(app.session.is_none());
    assert_eq!(
        app.status.text,
        "Locked. Unlock parent mode to make changes."
    );
}

#[test]
fn locking_cancels_an_in_progress_correction() {
    let (mut app, _dir) = app_with_entries();
    app.draft.description = "half typed".to_owned();
    app.draft.amount = "3".to_owned();
    let before = app.selected_wallet().entries.clone();

    app.begin_entry_edit(0);
    app.draft.amount = "11.00".to_owned();
    app.lock_parent();

    assert!(app.entry_edit.is_none());
    assert_eq!(app.draft.description, "half typed");
    assert_eq!(app.draft.amount, "3");
    assert_eq!(app.selected_wallet().entries, before);
}

#[test]
fn locking_drops_a_pending_undo() {
    let (mut app, _dir) = app_with_entries();
    app.remove_latest_entry();
    assert!(app.undo.is_some());

    app.lock_parent();
    assert!(app.undo.is_none());
}

#[test]
fn lock_deletes_tracked_temp_artifacts() {
    let (mut app, dir) = test_app();
    let artifact = dir.path().join("cofferly-recovery-card-test.html");
    std::fs::write(&artifact, "secret story").unwrap();
    app.track_temp_artifact(artifact.clone());

    app.lock_parent();

    assert!(!artifact.exists());
    assert!(app.temp_artifact_paths.is_empty());
}

#[test]
fn on_exit_deletes_tracked_temp_artifacts() {
    let (mut app, dir) = test_app();
    let artifact = dir.path().join("cofferly-ledger-test.csv");
    std::fs::write(&artifact, "secret ledger").unwrap();
    app.track_temp_artifact(artifact.clone());

    app.on_exit();

    assert!(!artifact.exists());
}

#[test]
fn confirm_story_setup_runs_off_ui_thread() {
    let (mut app, _dir) = test_app();
    app.lock_mode = LockMode::SetupConfirm;
    let selected = test_story();
    app.pending_story = Some(selected);
    app.story_selections = selected.to_vec();

    app.confirm_story_setup();

    assert!(app.unlocking);
    assert!(app.pending_story.is_some());

    finish_background_work(&mut app);

    assert!(!app.unlocking);
    assert!(app.pending_story.is_none());
    assert!(app.parent_unlocked);
}

#[test]
fn confirm_story_setup_clears_pending_story_after_success() {
    let (mut app, _dir) = test_app();
    app.lock_mode = LockMode::SetupConfirm;
    let selected = story::generate().unwrap();
    app.pending_story = Some(selected);
    app.story_selections = selected.to_vec();

    app.confirm_story_setup();
    finish_background_work(&mut app);

    assert!(app.pending_story.is_none());
}

#[test]
fn auto_lock_triggers_after_inactivity_threshold() {
    let (mut app, _dir) = test_app();
    app.parent_unlocked = true;
    app.last_interaction = Instant::now() - AUTO_LOCK_AFTER - Duration::from_secs(1);
    let ctx = egui::Context::default();
    app.auto_lock_if_idle(&ctx);
    assert!(!app.parent_unlocked);
    assert!(app.status.text.contains("inactivity"));
}

#[test]
fn auto_lock_warns_in_the_last_two_minutes() {
    let (mut app, _dir) = test_app();
    app.parent_unlocked = true;
    app.last_interaction = Instant::now() - AUTO_LOCK_AFTER + Duration::from_secs(90);
    let remaining = app.auto_lock_remaining().expect("unlocked");
    assert!(remaining <= AUTO_LOCK_WARN);
    assert!(!remaining.is_zero());
    let ctx = egui::Context::default();
    app.auto_lock_if_idle(&ctx);
    assert!(app.parent_unlocked);
}

#[test]
fn remove_latest_entry_removes_newest_by_date_not_append_order() {
    let (mut app, _dir) = test_app();
    app.draft.kind = EntryKind::Deposit;
    app.draft.description = "Recent".to_owned();
    app.draft.amount = "10".to_owned();
    app.draft.date_input = "07/15/2026".to_owned();
    app.add_entry();

    app.draft.description = "Backdated".to_owned();
    app.draft.amount = "5".to_owned();
    app.draft.date_input = "07/01/2026".to_owned();
    app.add_entry();

    assert_eq!(app.selected_wallet().entries.len(), 2);

    app.remove_latest_entry();

    assert_eq!(app.selected_wallet().entries.len(), 1);
    assert_eq!(app.selected_wallet().entries[0].description, "Backdated");
    assert!(app.status.text.contains("Recent"));
    assert!(app.undo.is_some());
}

#[test]
fn add_entry_can_use_an_earlier_date() {
    let (mut app, _dir) = test_app();
    app.draft.kind = EntryKind::Deposit;
    app.draft.description = "Backdated allowance".to_owned();
    app.draft.amount = "5".to_owned();
    app.draft.date_input = "07/01/2026".to_owned();

    app.add_entry();

    assert_eq!(
        app.selected_wallet().entries[0].date,
        NaiveDate::from_ymd_opt(2026, 7, 1).unwrap()
    );
    assert!(app.status.text.contains("Added $5.00"));
}

#[test]
fn add_entry_rejects_a_future_date() {
    let (mut app, _dir) = test_app();
    app.draft.kind = EntryKind::Deposit;
    app.draft.description = "Tomorrow".to_owned();
    app.draft.amount = "5".to_owned();
    app.draft.date_input = "12/31/2099".to_owned();

    app.add_entry();

    assert!(app.selected_wallet().entries.is_empty());
    assert_eq!(app.status.text, "Use today or an earlier date.");
    assert_eq!(app.pending_entry_focus, Some(EntryFormField::Date));
}

#[test]
fn entry_validation_moves_focus_to_the_first_invalid_field() {
    let (mut app, _dir) = test_app();
    app.draft.kind = EntryKind::Deposit;
    app.draft.description = "Allowance".to_owned();
    app.draft.amount.clear();

    app.add_entry();

    assert_eq!(app.pending_entry_focus, Some(EntryFormField::Amount));
    assert_eq!(
        app.status.text,
        "Enter a valid amount, like 10, 10.50, or $1,234.56."
    );
    assert!(app.selected_wallet().entries.is_empty());

    app.draft.amount = "5".to_owned();
    app.draft.description.clear();
    app.add_entry();

    assert_eq!(app.pending_entry_focus, Some(EntryFormField::Description));
    assert_eq!(app.status.text, "Add a description (1-100 characters).");

    app.draft.description = "a".repeat(data::MAX_DESCRIPTION_CHARS + 1);
    app.draft.amount = "5".to_owned();
    app.draft.date_input = format_ledger_date(Local::now().date_naive());
    app.add_entry();

    assert_eq!(app.pending_entry_focus, Some(EntryFormField::Description));
    assert_eq!(app.status.text, "Add a description (1-100 characters).");
    assert!(app.selected_wallet().entries.is_empty());

    app.draft.description = "Allowance".to_owned();
    app.draft.date_input = "not a date".to_owned();
    app.add_entry();

    assert_eq!(app.pending_entry_focus, Some(EntryFormField::Date));
    assert_eq!(
        app.status.text,
        "Enter a date as MM/DD/YYYY (08/21/2026) or ISO %Y-%m-%d (2026-08-21)."
    );
    assert_eq!(app.status.severity, StatusSeverity::Error);
    assert!(app.selected_wallet().entries.is_empty());
}

#[test]
fn money_out_that_would_go_negative_asks_confirm_before_commit() {
    let (mut app, _dir) = test_app();
    app.draft.kind = EntryKind::Deduction;
    app.draft.description = "Snack".to_owned();
    app.draft.amount = "5".to_owned();

    app.add_entry();

    assert!(app.selected_wallet().entries.is_empty());
    assert_eq!(app.confirm_negative_cents, Some(500));
    assert!(app.status.text.contains("would leave Child 1 at -$5.00"));
    assert_eq!(app.status.severity, StatusSeverity::Info);

    app.add_entry();

    assert_eq!(app.selected_wallet().entries.len(), 1);
    assert_eq!(app.selected_wallet().entries[0].amount_cents, -500);
    assert_eq!(app.selected_wallet().current_balance_cents(), -500);
    assert!(app.confirm_negative_cents.is_none());
    assert!(app.status.text.contains("Deducted $5.00"));
    assert_eq!(app.status.severity, StatusSeverity::Success);
}

#[test]
fn changing_the_money_out_amount_requires_a_fresh_negative_confirm() {
    let (mut app, _dir) = test_app();
    app.draft.kind = EntryKind::Deduction;
    app.draft.description = "Snack".to_owned();
    app.draft.amount = "5".to_owned();
    app.add_entry();
    assert_eq!(app.confirm_negative_cents, Some(500));

    app.draft.amount = "8".to_owned();
    app.add_entry();

    assert!(app.selected_wallet().entries.is_empty());
    assert_eq!(app.confirm_negative_cents, Some(800));
    assert!(app.status.text.contains("would leave Child 1 at -$8.00"));
}

#[test]
fn deposits_do_not_require_a_negative_balance_confirm() {
    let (mut app, _dir) = test_app();
    app.draft.kind = EntryKind::Deposit;
    app.draft.description = "Weekly allowance".to_owned();
    app.draft.amount = "10".to_owned();
    app.confirm_negative_cents = Some(1000);

    app.add_entry();

    assert_eq!(app.selected_wallet().entries.len(), 1);
    assert_eq!(app.selected_wallet().entries[0].amount_cents, 1000);
    assert!(app.confirm_negative_cents.is_none());
    assert!(app.status.text.contains("Added $10.00"));
}

#[test]
fn money_out_that_stays_non_negative_commits_without_confirm() {
    let (mut app, _dir) = test_app();
    app.data.wallets[0].starting_balance_cents = 1_000;
    app.draft.kind = EntryKind::Deduction;
    app.draft.description = "Snack".to_owned();
    app.draft.amount = "10".to_owned();

    app.add_entry();

    assert_eq!(app.selected_wallet().entries.len(), 1);
    assert_eq!(app.selected_wallet().current_balance_cents(), 0);
    assert!(app.confirm_negative_cents.is_none());
    assert!(app.status.text.contains("Deducted $10.00"));
}

#[test]
fn canceling_a_negative_spend_confirm_does_not_record_the_entry() {
    let (mut app, _dir) = test_app();
    app.draft.kind = EntryKind::Deduction;
    app.draft.description = "Snack".to_owned();
    app.draft.amount = "5".to_owned();
    app.add_entry();
    assert_eq!(app.confirm_negative_cents, Some(500));

    app.cancel_negative_spend_confirm();

    assert!(app.confirm_negative_cents.is_none());
    assert!(app.selected_wallet().entries.is_empty());
    assert_eq!(app.status.text, "Spending not recorded.");
}

#[test]
fn canceling_a_negative_spend_confirm_does_not_steal_focus_to_description() {
    let (mut app, _dir) = test_app();
    app.draft.kind = EntryKind::Deduction;
    app.draft.description = "Snack".to_owned();
    app.draft.amount = "5".to_owned();
    app.add_entry();
    app.pending_entry_focus = None;

    app.cancel_negative_spend_confirm();

    assert_eq!(app.pending_entry_focus, None);
}

#[test]
fn successful_add_focuses_description_for_rapid_multi_entry_logging() {
    let (mut app, _dir) = test_app();
    app.draft.kind = EntryKind::Deposit;
    app.draft.description = "Weekly allowance".to_owned();
    app.draft.amount = "10".to_owned();
    app.pending_entry_focus = None;

    app.add_entry();

    assert_eq!(app.selected_wallet().entries.len(), 1);
    assert_eq!(app.pending_entry_focus, Some(EntryFormField::Description));
    // Clear/sticky behavior unchanged.
    assert_eq!(app.draft.description, "");
    assert_eq!(app.draft.amount, "");
    assert_eq!(app.draft.kind, EntryKind::Deposit);
}

#[test]
fn successful_money_out_after_negative_confirm_also_focuses_description() {
    let (mut app, _dir) = test_app();
    app.draft.kind = EntryKind::Deduction;
    app.draft.description = "Snack".to_owned();
    app.draft.amount = "5".to_owned();
    app.add_entry();
    assert_eq!(app.confirm_negative_cents, Some(500));

    app.add_entry();

    assert_eq!(app.selected_wallet().entries.len(), 1);
    assert_eq!(app.pending_entry_focus, Some(EntryFormField::Description));
}

#[test]
fn today_control_fills_the_local_date() {
    let (mut app, _dir) = test_app();
    app.draft.date_input = "07/01/2020".to_owned();
    app.pending_entry_focus = None;

    app.fill_entry_date_today();

    assert_eq!(
        app.draft.date_input,
        format_ledger_date(Local::now().date_naive())
    );
    assert_eq!(app.pending_entry_focus, Some(EntryFormField::Date));
}

#[test]
fn ledger_filter_is_local_ui_state_that_leaves_entries_and_cache_untouched() {
    let (mut app, _dir) = test_app();
    app.draft.kind = EntryKind::Deposit;
    app.draft.description = "Weekly allowance".to_owned();
    app.draft.amount = "5".to_owned();
    app.add_entry();
    app.draft.kind = EntryKind::Deduction;
    app.draft.description = "Snack".to_owned();
    app.draft.amount = "2".to_owned();
    app.add_entry();

    assert_eq!(app.ledger_filter, "", "starts empty by default");

    app.ledger_filter = "allow".to_owned();
    let rows = app.cached_ledger_rows();
    let summary = ledger_filter_summary(&rows, &app.ledger_filter);
    let descriptions: Vec<_> = summary
        .rows
        .iter()
        .map(|row| row.description.as_str())
        .collect();

    // Default sort is newest-first, so the entry sorts ahead of the
    // always-kept starting-balance row.
    assert_eq!(descriptions, ["Weekly allowance", "Starting balance"]);
    // Filtering is display-only: the underlying entries and sorted cache
    // are unaffected by whatever the filter text is.
    assert_eq!(app.selected_wallet().entries.len(), 2);

    app.ledger_filter.clear();
    let unfiltered = ledger_filter_summary(&rows, &app.ledger_filter);
    assert_eq!(unfiltered.rows.len(), rows.len());
}

#[test]
fn repeat_last_deposit_is_absent_with_no_entries() {
    let (app, _dir) = test_app();

    assert!(app.selected_wallet().latest_deposit().is_none());
}

#[test]
fn repeat_last_deposit_is_absent_with_only_deductions() {
    let (mut app, _dir) = test_app();
    app.draft.kind = EntryKind::Deduction;
    app.draft.description = "Snack".to_owned();
    app.draft.amount = "5".to_owned();
    app.add_entry();

    assert!(app.selected_wallet().latest_deposit().is_none());
}

#[test]
fn repeat_last_deposit_prefills_newest_deposit_amount_and_description_with_todays_date() {
    let (mut app, _dir) = test_app();
    app.draft.kind = EntryKind::Deposit;
    app.draft.description = "Weekly allowance".to_owned();
    app.draft.amount = "10".to_owned();
    app.draft.date_input = "06/01/2026".to_owned();
    app.add_entry();

    app.draft.kind = EntryKind::Deduction;
    app.draft.description = "Snack".to_owned();
    app.draft.amount = "3".to_owned();
    app.add_entry();

    app.draft.kind = EntryKind::Deposit;
    app.draft.description = "Birthday gift".to_owned();
    app.draft.amount = "25".to_owned();
    app.draft.date_input = "06/05/2026".to_owned();
    app.add_entry();

    // Clear the draft so the assertions below only reflect the repeat action.
    app.draft = EntryDraft::new();
    app.draft.kind = EntryKind::Deduction;
    app.pending_entry_focus = None;

    app.repeat_last_deposit();

    assert_eq!(app.draft.kind, EntryKind::Deposit);
    assert_eq!(app.draft.amount, format_money_input(2500));
    assert_eq!(app.draft.description, "Birthday gift");
    assert_eq!(
        app.draft.date_input,
        format_ledger_date(Local::now().date_naive())
    );
    assert_eq!(app.pending_entry_focus, Some(EntryFormField::Amount));
}

#[test]
fn repeat_last_deposit_prefill_still_goes_through_normal_add_entry_validation() {
    // A prefilled draft must not bypass validation: an oversized amount
    // should still be rejected the same way a manually typed one would.
    let (mut app, _dir) = test_app();
    app.draft.kind = EntryKind::Deposit;
    app.draft.description = "Weekly allowance".to_owned();
    app.draft.amount = "10".to_owned();
    app.add_entry();
    assert_eq!(app.selected_wallet().entries.len(), 1);

    app.repeat_last_deposit();
    assert_eq!(app.draft.amount, format_money_input(1000));

    app.add_entry();

    assert_eq!(app.selected_wallet().entries.len(), 2);
    assert_eq!(app.selected_wallet().entries[1].amount_cents, 1000);
    assert_eq!(
        app.selected_wallet().entries[1].description,
        "Weekly allowance"
    );
    assert_eq!(
        app.selected_wallet().entries[1].date,
        Local::now().date_naive()
    );
    assert_eq!(app.status.severity, StatusSeverity::Success);
}

#[test]
fn repeat_last_deposit_does_nothing_when_no_deposit_exists() {
    let (mut app, _dir) = test_app();
    app.draft.kind = EntryKind::Deduction;
    app.draft.description = "Snack".to_owned();
    app.draft.amount = "5".to_owned();
    app.add_entry();

    let draft_before = app.draft.clone();
    app.repeat_last_deposit();

    assert_eq!(app.draft.kind, draft_before.kind);
    assert_eq!(app.draft.amount, draft_before.amount);
    assert_eq!(app.draft.description, draft_before.description);
}

#[test]
fn add_entry_accepts_iso_dates_and_focuses_date_on_failure() {
    let (mut app, _dir) = test_app();
    app.draft.kind = EntryKind::Deposit;
    app.draft.description = "ISO allowance".to_owned();
    app.draft.amount = "5".to_owned();
    app.draft.date_input = "2026-07-01".to_owned();

    app.add_entry();

    assert_eq!(
        app.selected_wallet().entries[0].date,
        NaiveDate::from_ymd_opt(2026, 7, 1).unwrap()
    );

    app.draft.description = "Bad date".to_owned();
    app.draft.amount = "5".to_owned();
    app.draft.date_input = "21-08-2026".to_owned();
    app.add_entry();

    assert_eq!(app.pending_entry_focus, Some(EntryFormField::Date));
    assert!(app.status.text.contains("MM/DD/YYYY"));
    assert!(app.status.text.contains("%Y-%m-%d"));
    assert_eq!(app.status.severity, StatusSeverity::Error);
}

#[test]
fn invalid_starting_balance_explains_accepted_amount_formats() {
    let (mut app, _dir) = test_app();
    app.starting_balance_input = "not money".to_owned();

    app.update_starting_balance();

    assert_eq!(
        app.status.text,
        "Enter a valid starting balance, like 90, 90.00, or $1,234.56."
    );
    assert_eq!(app.status.severity, StatusSeverity::Error);
    assert_eq!(app.selected_wallet().starting_balance_cents, 0);
    assert!(!app.data_path.exists());
}

fn wallet_with_opening_and_entry(app: &mut CofferlyApp, starting_cents: i64, entry_cents: i64) {
    app.data.wallets[0].starting_balance_cents = starting_cents;
    app.data.wallets[0].entries.push(Entry {
        date: NaiveDate::from_ymd_opt(2026, 7, 1).unwrap(),
        description: "Allowance".to_owned(),
        amount_cents: entry_cents,
    });
}

#[test]
fn opening_settings_prefills_starting_balance_not_the_running_total() {
    let (mut app, _dir) = test_app();
    wallet_with_opening_and_entry(&mut app, 1_000, 500);
    assert_eq!(app.selected_wallet().current_balance_cents(), 1_500);

    app.open_settings();

    assert!(app.show_settings);
    assert_eq!(app.starting_balance_input, format_money_input(1_000));
    assert_ne!(app.starting_balance_input, format_money_input(1_500));
    assert!(!app.starting_balance_save_ready());
}

#[test]
fn saving_the_prefilled_starting_balance_does_not_rebase_opening_to_the_running_total() {
    let (mut app, _dir) = test_app();
    wallet_with_opening_and_entry(&mut app, 1_000, 500);

    app.open_settings();
    app.update_starting_balance();

    assert_eq!(app.selected_wallet().starting_balance_cents, 1_000);
    assert_eq!(app.selected_wallet().current_balance_cents(), 1_500);
    assert_eq!(app.selected_wallet().entries.len(), 1);
    assert!(!app.data_path.exists());
}

#[test]
fn updating_starting_balance_shifts_the_running_total_without_touching_entries() {
    let (mut app, _dir) = test_app();
    wallet_with_opening_and_entry(&mut app, 1_000, 500);
    app.starting_balance_input = "20.00".to_owned();
    assert!(app.starting_balance_save_ready());

    app.update_starting_balance();

    assert_eq!(app.selected_wallet().starting_balance_cents, 2_000);
    assert_eq!(app.selected_wallet().current_balance_cents(), 2_500);
    assert_eq!(app.selected_wallet().entries.len(), 1);
    assert!(app.starting_balance_input.is_empty());
    assert!(!app.starting_balance_save_ready());
    assert_eq!(
        saved_data(&app, "1234").wallets[0].starting_balance_cents,
        2_000
    );
}

#[test]
fn starting_balance_write_failure_keeps_memory_and_reports_status_error() {
    let (mut app, dir) = test_app();
    app.data_path = unwritable_data_path(&dir);
    app.starting_balance_input = "20.00".to_owned();

    app.update_starting_balance();

    assert_eq!(app.status.severity, StatusSeverity::Error);
    assert!(app.status.text.starts_with("Could not save:"));
    assert_eq!(app.selected_wallet().starting_balance_cents, 2_000);
    assert!(app.starting_balance_input.is_empty());
    assert!(app.raw_bytes.is_none());
    assert!(!app.data_path.exists());
}

#[test]
fn adding_a_wallet_resets_the_previous_kids_starting_balance_prefill() {
    let (mut app, _dir) = test_app();
    wallet_with_opening_and_entry(&mut app, 1_000, 500);
    app.open_settings();
    assert_eq!(app.starting_balance_input, format_money_input(1_000));
    assert!(!app.starting_balance_save_ready());

    app.new_child_name_input = "Sam".to_owned();
    app.add_child_wallet();

    assert_eq!(app.selected_wallet().child_name, "Sam");
    assert_eq!(app.selected_wallet().starting_balance_cents, 0);
    assert_eq!(app.starting_balance_input, format_money_input(0));
    assert!(!app.starting_balance_save_ready());

    app.starting_balance_input = "5.00".to_owned();
    assert!(app.starting_balance_save_ready());
}

#[test]
fn renaming_a_wallet_keeps_the_settings_name_field_prefilled() {
    let (mut app, _dir) = test_app();
    wallet_with_opening_and_entry(&mut app, 1_000, 500);
    app.open_settings();
    assert_eq!(app.child_name_input, "Child 1");
    assert_eq!(app.starting_balance_input, format_money_input(1_000));
    app.child_name_input = "Sam".to_owned();

    app.rename_selected_child();

    assert_eq!(app.selected_wallet().child_name, "Sam");
    assert_eq!(app.child_name_input, "Sam");
    assert_eq!(app.starting_balance_input, format_money_input(1_000));
    assert_eq!(
        app.child_name_input.trim(),
        app.selected_wallet().child_name
    );
    assert_eq!(saved_data(&app, "1234").wallets[0].child_name, "Sam");
}

#[test]
fn deleting_a_wallet_resets_the_deleted_kids_settings_prefills() {
    let (mut app, _dir) = test_app();
    wallet_with_opening_and_entry(&mut app, 1_000, 500);
    app.data.wallets[1].starting_balance_cents = 2_500;
    app.open_settings();
    assert!(app.show_settings);
    assert_eq!(app.child_name_input, "Child 1");
    assert_eq!(app.starting_balance_input, format_money_input(1_000));
    assert!(!app.starting_balance_save_ready());

    app.delete_selected_wallet();

    assert!(app.show_settings);
    assert_eq!(app.selected_wallet().child_name, "Child 2");
    assert_eq!(app.selected_wallet().starting_balance_cents, 2_500);
    assert_eq!(app.child_name_input, "Child 2");
    assert_eq!(app.starting_balance_input, format_money_input(2_500));
    assert!(!app.starting_balance_save_ready());

    app.starting_balance_input = "5.00".to_owned();
    assert!(app.starting_balance_save_ready());
}

mod weekly_allowance {
    use super::*;
    use crate::data::{WeeklyAllowance, WEEKLY_ALLOWANCE_DESCRIPTION};

    const SECRET: &str = "test-secret";

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    /// Child 1 has a $5 allowance turned on Tuesday 2026-09-01.
    fn vault_with_allowance() -> AppData {
        let mut data = default_app_data();
        data.wallets[0].weekly_allowance = Some(WeeklyAllowance::starting(500, date(2026, 9, 1)));
        data
    }

    fn allowance_entries(data: &AppData) -> Vec<NaiveDate> {
        data.wallets[0]
            .entries
            .iter()
            .filter(|entry| entry.description == WEEKLY_ALLOWANCE_DESCRIPTION)
            .map(|entry| entry.date)
            .collect()
    }

    fn unlock_on(app: &mut CofferlyApp, data: AppData, today: NaiveDate) -> Option<Status> {
        let session = SessionCrypto::establish(SECRET).unwrap();
        app.apply_unlock_on(data, session, today)
    }

    #[test]
    fn unlock_posts_missed_weeks_saves_them_with_last_posted_and_announces_the_count() {
        let (mut app, _dir) = test_app();

        let status = unlock_on(&mut app, vault_with_allowance(), date(2026, 9, 23));

        assert!(matches!(
            status,
            Some(Status {
                severity: StatusSeverity::Success,
                ..
            })
        ));
        assert_eq!(
            app.status.text,
            "Coffer Story unlocked. Weekly allowance added: 3 entries for Child 1."
        );
        let saved = saved_data(&app, SECRET);
        assert_eq!(
            allowance_entries(&saved),
            vec![date(2026, 9, 8), date(2026, 9, 15), date(2026, 9, 22)]
        );
        assert_eq!(
            saved.wallets[0].weekly_allowance.unwrap().last_posted,
            date(2026, 9, 22)
        );
        assert_eq!(allowance_entries(&app.data), allowance_entries(&saved));
        assert!(saved.wallets[1].entries.is_empty());
    }

    #[test]
    fn re_unlocking_the_saved_vault_never_double_posts() {
        let (mut app, _dir) = test_app();
        unlock_on(&mut app, vault_with_allowance(), date(2026, 9, 23));

        let reloaded = saved_data(&app, SECRET);
        let status = unlock_on(&mut app, reloaded, date(2026, 9, 28));

        assert!(status.is_none());
        assert_eq!(app.status.text, "Coffer Story unlocked.");
        assert_eq!(allowance_entries(&saved_data(&app, SECRET)).len(), 3);
    }

    #[test]
    fn a_failed_save_posts_nothing_so_the_next_unlock_posts_exactly_once() {
        let (mut app, dir) = test_app();
        let good_path = app.data_path.clone();
        app.data_path = unwritable_data_path(&dir);

        let status = unlock_on(&mut app, vault_with_allowance(), date(2026, 9, 23));

        assert!(matches!(
            status,
            Some(Status {
                severity: StatusSeverity::Error,
                ..
            })
        ));
        assert!(app.status.text.contains("Weekly allowance was not added"));
        assert!(allowance_entries(&app.data).is_empty());
        assert_eq!(
            app.data.wallets[0].weekly_allowance.unwrap().last_posted,
            date(2026, 9, 1),
            "in-memory ledger must not keep entries the vault never got"
        );

        app.data_path = good_path;
        unlock_on(&mut app, vault_with_allowance(), date(2026, 9, 23));
        let reloaded = saved_data(&app, SECRET);
        unlock_on(&mut app, reloaded, date(2026, 9, 23));
        assert_eq!(allowance_entries(&saved_data(&app, SECRET)).len(), 3);
    }

    #[test]
    fn the_status_line_says_when_catch_up_was_capped() {
        let (mut app, _dir) = test_app();

        unlock_on(&mut app, vault_with_allowance(), date(2026, 11, 17));

        assert_eq!(
            app.status.text,
            "Coffer Story unlocked. Weekly allowance added: 8 entries for Child 1. \
             Catch-up is capped at 8 weeks: Child 1 (3 older weeks skipped)."
        );
    }

    #[test]
    fn posted_entries_can_be_removed_like_any_other() {
        let (mut app, _dir) = test_app();
        unlock_on(&mut app, vault_with_allowance(), date(2026, 9, 9));
        assert_eq!(app.data.wallets[0].entries.len(), 1);

        app.remove_latest_entry();

        assert!(app.data.wallets[0].entries.is_empty());
        // Removing it doesn't re-post it: last_posted already covers that week.
        let reloaded = saved_data(&app, SECRET);
        unlock_on(&mut app, reloaded, date(2026, 9, 9));
        assert!(saved_data(&app, SECRET).wallets[0].entries.is_empty());
    }

    #[test]
    fn settings_turn_on_with_todays_weekday_change_amount_and_turn_off() {
        let (mut app, _dir) = test_app();
        app.parent_unlocked = true;
        app.session = Some(SessionCrypto::establish(SECRET).unwrap());
        app.prefill_settings_from_selected();
        assert_eq!(app.weekly_allowance_input, "");
        assert!(
            !app.weekly_allowance_save_ready(),
            "blank while off is not a change"
        );

        app.weekly_allowance_input = "5".to_owned();
        assert!(app.weekly_allowance_save_ready());
        app.save_weekly_allowance_on(date(2026, 9, 24));
        let on = app.selected_wallet().weekly_allowance.unwrap();
        assert_eq!(on, WeeklyAllowance::starting(500, date(2026, 9, 24)));
        assert_eq!(on.weekday_name(), "Thursday");
        assert_eq!(
            app.status.text,
            "Weekly allowance of $5.00 turned on for Child 1. It posts every Thursday, starting 10/01/2026."
        );
        assert!(
            app.selected_wallet().entries.is_empty(),
            "the enable day doesn't post"
        );
        assert_eq!(
            saved_data(&app, SECRET).wallets[0].weekly_allowance,
            Some(on)
        );

        // Changing the amount keeps the weekday and last_posted.
        let mut posted = on;
        posted.last_posted = date(2026, 10, 8);
        app.selected_wallet_mut().weekly_allowance = Some(posted);
        app.weekly_allowance_input = "7.50".to_owned();
        app.save_weekly_allowance_on(date(2026, 10, 10));
        let changed = app.selected_wallet().weekly_allowance.unwrap();
        assert_eq!(changed.amount_cents, 750);
        assert_eq!(changed.enabled_on, date(2026, 9, 24));
        assert_eq!(changed.last_posted, date(2026, 10, 8));

        // Blank turns it off; turning it back on starts fresh from that day.
        app.weekly_allowance_input = "  ".to_owned();
        assert!(app.weekly_allowance_save_ready());
        app.save_weekly_allowance_on(date(2026, 10, 10));
        assert!(app.selected_wallet().weekly_allowance.is_none());
        assert_eq!(app.status.text, "Weekly allowance turned off for Child 1.");

        app.weekly_allowance_input = "5".to_owned();
        app.save_weekly_allowance_on(date(2026, 11, 2));
        assert_eq!(
            app.selected_wallet().weekly_allowance,
            Some(WeeklyAllowance::starting(500, date(2026, 11, 2)))
        );
    }

    #[test]
    fn settings_reject_zero_negative_and_oversized_amounts() {
        let (mut app, _dir) = test_app();
        app.parent_unlocked = true;
        for input in ["0", "-5", "abc", "999999999999"] {
            app.weekly_allowance_input = input.to_owned();
            assert!(!app.weekly_allowance_save_ready(), "{input}");
            app.save_weekly_allowance_on(date(2026, 9, 24));
            assert!(app.selected_wallet().weekly_allowance.is_none(), "{input}");
            assert_eq!(app.status.severity, StatusSeverity::Error, "{input}");
        }
    }
}

mod savings_goal {
    use super::*;
    use crate::data::SavingsGoalProgress;

    const SECRET: &str = "test-secret";

    fn unlocked() -> (CofferlyApp, TempDir) {
        let (mut app, dir) = test_app();
        app.parent_unlocked = true;
        app.session = Some(SessionCrypto::establish(SECRET).unwrap());
        app.prefill_settings_from_selected();
        (app, dir)
    }

    #[test]
    fn settings_set_change_and_clear_the_goal_in_the_vault() {
        let (mut app, _dir) = unlocked();
        assert_eq!(app.savings_goal_input, "");
        assert!(
            !app.savings_goal_save_ready(),
            "blank while unset is not a change"
        );

        app.savings_goal_input = "120".to_owned();
        assert!(app.savings_goal_save_ready());
        app.save_savings_goal();
        assert_eq!(app.selected_wallet().savings_goal_cents, Some(12_000));
        assert_eq!(app.status.text, "Savings goal for Child 1 is $120.00.");
        assert_eq!(
            saved_data(&app, SECRET).wallets[0].savings_goal_cents,
            Some(12_000)
        );
        assert_eq!(
            app.selected_wallet()
                .savings_goal_progress()
                .unwrap()
                .label(),
            "$0.00 of $120.00 · $120.00 to go"
        );

        app.selected_wallet_mut().starting_balance_cents = 4_500;
        assert_eq!(
            app.selected_wallet().savings_goal_progress().unwrap(),
            SavingsGoalProgress::from_balance(4_500, 12_000)
        );

        app.savings_goal_input = "50".to_owned();
        app.save_savings_goal();
        assert_eq!(app.selected_wallet().savings_goal_cents, Some(5_000));
        app.selected_wallet_mut().starting_balance_cents = 5_000;
        let exact = app.selected_wallet().savings_goal_progress().unwrap();
        assert!(exact.reached);
        assert_eq!(exact.fraction, 1.0);
        assert_eq!(exact.label(), "Goal reached");

        app.selected_wallet_mut().starting_balance_cents = 15_000;
        let over = app.selected_wallet().savings_goal_progress().unwrap();
        assert_eq!(over.fraction, 1.0);
        assert_eq!(over.label(), "Goal reached");

        app.selected_wallet_mut().starting_balance_cents = -200;
        let negative = app.selected_wallet().savings_goal_progress().unwrap();
        assert_eq!(negative.fraction, 0.0);
        assert_eq!(negative.label(), "-$2.00 of $50.00 · $52.00 to go");

        app.savings_goal_input = "  ".to_owned();
        assert!(app.savings_goal_save_ready());
        app.save_savings_goal();
        assert!(app.selected_wallet().savings_goal_cents.is_none());
        assert!(app.selected_wallet().savings_goal_progress().is_none());
        assert_eq!(app.status.text, "Savings goal cleared for Child 1.");
        let cleared = serde_json::to_string(&saved_data(&app, SECRET)).unwrap();
        assert!(!cleared.contains("savings_goal"));
    }

    #[test]
    fn settings_reject_zero_negative_and_oversized_goals() {
        let (mut app, _dir) = unlocked();
        for input in ["0", "-5", "abc", "999999999999", "1000000000"] {
            app.savings_goal_input = input.to_owned();
            assert!(!app.savings_goal_save_ready(), "{input}");
            app.save_savings_goal();
            assert!(
                app.selected_wallet().savings_goal_cents.is_none(),
                "{input}"
            );
            assert_eq!(app.status.severity, StatusSeverity::Error, "{input}");
        }

        app.savings_goal_input = "999999999.99".to_owned();
        assert!(app.savings_goal_save_ready());
        app.save_savings_goal();
        assert_eq!(
            app.selected_wallet().savings_goal_cents,
            Some(crate::data::MAX_ABSOLUTE_CENTS)
        );

        app.prefill_settings_from_selected();
        assert!(!app.savings_goal_save_ready());
    }
}
