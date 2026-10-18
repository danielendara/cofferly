//! Headless UI journeys: drive the real `eframe::App::update` through
//! AccessKit with `egui_kittest`, finding widgets only by role and accessible
//! label (or key events). The app is built by the same constructor `main` uses,
//! against an empty temp data dir (the first-run path); fixtures only.
use super::*;
use crate::test_support::ScopedDataDir;
use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use egui_kittest::Harness;
use std::time::{Duration, Instant};
use tempfile::TempDir;

/// Accessible name of the empty description field (it embeds the length).
const DESCRIPTION_EMPTY: &str = "What was it for? 0 of 100 characters";

type AppHarness = Harness<'static, CofferlyApp>;

/// Builds the app via `CofferlyApp::new` (as `main` does). `COFFERLY_DATA_DIR`
/// must already point at a temp dir (hold a `ScopedDataDir` for the journey).
fn harness() -> AppHarness {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1080.0, 720.0))
        .build_eframe(|cc| CofferlyApp::new(cc));
    harness.run();
    harness
}

/// Steps frames until `done` holds. `Harness::run` panics while background
/// key derivation keeps requesting repaints, so poll with a timeout instead.
fn step_until(harness: &mut AppHarness, what: &str, done: impl Fn(&AppHarness) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !done(harness) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        harness.step();
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn has_label(harness: &AppHarness, label: &str) -> bool {
    harness.query_by_label(label).is_some()
}

fn click(harness: &mut AppHarness, label: &str) {
    println!("  click  {label:?}");
    harness.get_by_label(label).click();
    // Let the click land and the resulting screen lay out before the next query.
    for _ in 0..4 {
        harness.step();
    }
}

/// Activates a button from the keyboard: focus it, then press Enter.
fn press_enter_on(harness: &mut AppHarness, label: &str) {
    println!("  key    Enter on {label:?}");
    harness.get_by_label(label).focus();
    harness.step();
    harness.key_press(egui::Key::Enter);
    for _ in 0..4 {
        harness.step();
    }
}

fn click_story(harness: &mut AppHarness, story: [&'static str; story::STORY_LENGTH]) {
    for id in story {
        let name = story::label(id).unwrap();
        click(harness, &format!("Coffer Story object: {name}"));
    }
}

fn type_into(harness: &mut AppHarness, role: Role, label: &str, text: &str) {
    println!("  type   {label:?} <- {text:?}");
    let field = harness.get_by_role_and_label(role, label);
    field.focus();
    field.type_text(text);
    harness.step();
    harness.step();
}

/// Completes first-run Coffer Story setup; returns the generated story.
fn first_run_setup(harness: &mut AppHarness) -> [&'static str; story::STORY_LENGTH] {
    assert!(has_label(harness, "Your new Coffer Story"));
    let story = harness.state().pending_story.expect("generated story");
    click(harness, "I wrote it down — continue");
    assert!(has_label(harness, "Confirm your Coffer Story"));
    click_story(harness, story);
    step_until(harness, "setup to unlock the app", |h| {
        has_label(h, "Record money in or money out")
    });
    story
}

fn vault_path(dir: &TempDir) -> std::path::PathBuf {
    dir.path().join("Cofferly").join("vault.cofferly")
}

/// Records one $5.00 spend through the entry form (confirming the overdraft
/// prompt, since the fixture wallet starts empty).
fn record_snack_spend(harness: &mut AppHarness) {
    type_into(harness, Role::TextInput, DESCRIPTION_EMPTY, "Snack");
    type_into(harness, Role::TextInput, "Amount", "5.00");
    click(harness, "Record spending");
    click(harness, "Record spending anyway");
}

fn unlock_with(harness: &mut AppHarness, story: [&'static str; story::STORY_LENGTH]) {
    assert!(has_label(harness, "Welcome back"));
    click_story(harness, story);
    step_until(harness, "unlock", |h| has_label(h, "Lock"));
}

fn label_containing(harness: &AppHarness, text: &str) -> bool {
    harness.query_all_by_label_contains(text).next().is_some()
}

/// The first button whose accessible label starts with `prefix` (labels such
/// as "Edit entry <date>: <description>" embed values the test does not pin).
fn button_with_prefix(harness: &AppHarness, prefix: &str) -> Option<String> {
    harness
        .query_all_by_role(Role::Button)
        .filter_map(|node| node.accesskit_node().label())
        .find(|label| label.starts_with(prefix))
}

#[test]
fn journey_first_run_setup_then_record_a_spend() {
    let dir = TempDir::new().unwrap();
    let _data_dir = ScopedDataDir::set(dir.path());
    let mut h = harness();
    assert!(
        !vault_path(&dir).exists(),
        "first run starts without a vault"
    );

    first_run_setup(&mut h);
    assert!(
        vault_path(&dir).exists(),
        "setup writes the vault in the temp dir"
    );

    record_snack_spend(&mut h);
    assert!(
        button_with_prefix(&h, "Edit entry").is_some_and(|label| label.ends_with(": Snack")),
        "ledger row (its Edit button) for the new entry"
    );
    assert!(label_containing(&h, "Deducted $5.00 for Child 1: Snack."));
    assert!(has_label(&h, "Child 1, balance -$5.00"));
    assert_eq!(h.state().data.wallets[0].entries.len(), 1);
    assert!(vault_path(&dir).exists());
}

fn click_edit_for_snack(harness: &mut AppHarness) {
    let label = button_with_prefix(harness, "Edit entry").expect("ledger row Edit button");
    click(harness, &label);
}

/// Replaces the amount field's text via keys only (select all, then type).
fn replace_amount(harness: &mut AppHarness, text: &str) {
    println!("  keys   Amount <- Ctrl+A, {text:?}");
    let field = harness.get_by_role_and_label(Role::TextInput, "Amount");
    field.focus();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.step();
    harness
        .get_by_role_and_label(Role::TextInput, "Amount")
        .type_text(text);
    harness.step();
    harness.step();
}

#[test]
fn journey_lock_ends_an_in_progress_correction_and_drops_the_undo() {
    let dir = TempDir::new().unwrap();
    let _data_dir = ScopedDataDir::set(dir.path());
    let mut h = harness();
    let story = first_run_setup(&mut h);
    record_snack_spend(&mut h);

    // Save one correction so an Undo really exists to be dropped.
    click_edit_for_snack(&mut h);
    replace_amount(&mut h, "6.00");
    click(&mut h, "Save correction");
    click(&mut h, "Save correction anyway"); // the wallet is still overdrawn
    assert!(!has_label(&h, "Cancel correction"));
    click(&mut h, "Settings");
    assert!(
        button_with_prefix(&h, "Undo").is_some(),
        "a saved correction offers Undo in Settings"
    );
    press_enter_on(&mut h, "Close");

    // Start another correction, then lock and unlock again.
    click_edit_for_snack(&mut h);
    assert!(has_label(&h, "Cancel correction"));
    click(&mut h, "Lock");
    unlock_with(&mut h, story);

    assert!(
        !has_label(&h, "Cancel correction"),
        "lock ended the correction"
    );
    assert!(has_label(&h, "Record money in or money out"));
    click(&mut h, "Settings");
    assert!(
        button_with_prefix(&h, "Undo").is_none(),
        "lock dropped the pending Undo"
    );
}

#[test]
fn journey_failed_save_rolls_back_the_entry_and_keeps_the_draft() {
    let dir = TempDir::new().unwrap();
    let _data_dir = ScopedDataDir::set(dir.path());
    let mut h = harness();
    first_run_setup(&mut h);

    // Make the vault path unwritable on every OS: move the data dir aside and
    // put a regular file where the directory was (parent-is-a-file).
    let data_dir = dir.path().join("Cofferly");
    let saved_dir = dir.path().join("Cofferly.saved");
    let saved_vault = saved_dir.join("vault.cofferly");
    std::fs::rename(&data_dir, &saved_dir).unwrap();
    std::fs::write(&data_dir, b"not a directory").unwrap();
    let vault_before = std::fs::read(&saved_vault).unwrap();

    type_into(&mut h, Role::TextInput, DESCRIPTION_EMPTY, "Snack");
    type_into(&mut h, Role::TextInput, "Amount", "5.00");
    click(&mut h, "Record spending");
    click(&mut h, "Record spending anyway");

    assert!(label_containing(&h, "Could not save:"));
    assert!(label_containing(&h, "The entry was not recorded."));
    assert!(
        button_with_prefix(&h, "Edit entry").is_none(),
        "no ledger row was added"
    );
    assert!(has_label(&h, "Child 1, balance $0.00"));
    assert!(
        h.query_by_role_and_label(Role::TextInput, "What was it for? 5 of 100 characters")
            .is_some(),
        "description draft is still in the form"
    );
    assert_eq!(
        h.get_by_role_and_label(Role::TextInput, "Amount")
            .value()
            .as_deref(),
        Some("5.00"),
        "amount draft is still in the form"
    );
    assert_eq!(std::fs::read(&saved_vault).unwrap(), vault_before);
}
