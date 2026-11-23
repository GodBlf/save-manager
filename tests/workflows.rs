use save_manager::{GameInput, Manager};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn game_settings_survive_reopening_and_roots_are_exclusive() {
    let area = tempfile::tempdir().unwrap();
    let state = area.path().join("state");
    let root = area.path().join("snapshots");
    fs::create_dir(&root).unwrap();
    let input = GameInput {
        name: "游戏 A".into(),
        root,
        paths: vec![area.path().join("saves"), area.path().join("A.ini")],
    };
    let mut manager = Manager::open(&state).unwrap();
    let game = manager.save_game(None, input.clone()).unwrap();
    assert!(manager.save_game(None, input.clone()).is_err());
    drop(manager);
    let manager = Manager::open(&state).unwrap();
    assert_eq!(manager.games()[0].id, game);
    assert_eq!(manager.games()[0].input, input);
}

struct GameArea {
    _temp: tempfile::TempDir,
    state: PathBuf,
    primary: PathBuf,
    input: GameInput,
}

impl GameArea {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("snapshots");
        let saves = temp.path().join("saves");
        let primary = temp.path().join("A.ini");
        fs::create_dir(&root).unwrap();
        fs::create_dir_all(saves.join("slot")).unwrap();
        fs::write(saves.join("slot/progress"), "old slot").unwrap();
        fs::write(&primary, "old config").unwrap();
        Self {
            state: temp.path().join("state"),
            input: GameInput {
                name: "游戏 A".into(),
                root,
                paths: vec![saves, primary.clone()],
            },
            primary,
            _temp: temp,
        }
    }

    fn open(&self) -> (Manager, uuid::Uuid) {
        let mut manager = Manager::open(&self.state).unwrap();
        let id = manager.save_game(None, self.input.clone()).unwrap();
        (manager, id)
    }
}

#[test]
fn creates_one_complete_snapshot_and_reuses_failed_number() {
    let area = GameArea::new();
    let (mut manager, game) = area.open();
    fs::remove_file(&area.primary).unwrap();
    let failure = manager.create_snapshot(game, "").unwrap_err();
    assert!(failure.to_string().contains("A.ini"));
    assert!(manager.snapshots(game).unwrap().is_empty());
    assert_eq!(fs::read_dir(&area.input.root).unwrap().count(), 0);
    fs::write(&area.primary, "old config").unwrap();
    assert!(manager.create_snapshot(game, "bad?name").is_err());
    manager.create_snapshot(game, "  A 重要  ").unwrap();
    let first = &manager.snapshots(game).unwrap()[0];
    assert_eq!(first.path, area.input.root.join("0-A 重要"));
    assert_eq!(
        fs::read_to_string(first.path.join("saves/slot/progress")).unwrap(),
        "old slot"
    );
    assert_eq!(
        fs::read_to_string(first.path.join("A.ini")).unwrap(),
        "old config"
    );
    manager.rename_snapshot(game, first, "A次要").unwrap();
    let renamed = &manager.snapshots(game).unwrap()[0];
    assert_eq!(renamed.path, area.input.root.join("0-A次要"));
    assert_eq!(renamed.id, first.id);
    manager.rename_snapshot(game, renamed, "").unwrap();
    assert!(area.input.root.join("0").is_dir());
    drop(manager);
    let mut manager = Manager::open(&area.state).unwrap();
    manager.create_snapshot(game, "").unwrap();
    assert_eq!(manager.snapshots(game).unwrap()[0].number, 1);
}

fn copy_directory(source: &Path, destination: &Path) {
    fs::create_dir(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_directory(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[test]
fn scans_only_current_root_and_resumes_numbering_when_readding_game() {
    let area = GameArea::new();
    let (mut manager, game) = area.open();
    manager.create_snapshot(game, "").unwrap();
    manager.create_snapshot(game, "").unwrap();
    let previous = manager.snapshots(game).unwrap();
    let new_root = area._temp.path().join("copied");
    fs::create_dir(&new_root).unwrap();
    copy_directory(&previous[1].path, &new_root.join("0"));
    fs::create_dir(new_root.join("2-普通目录")).unwrap();
    let new_input = GameInput {
        root: new_root.clone(),
        ..area.input.clone()
    };
    manager.save_game(Some(game), new_input.clone()).unwrap();
    assert_eq!(
        manager
            .snapshots(game)
            .unwrap()
            .iter()
            .map(|s| s.number)
            .collect::<Vec<_>>(),
        vec![0]
    );
    assert!(previous.iter().all(|s| s.path.exists()));
    manager.create_snapshot(game, "").unwrap();
    assert!(new_root.join("3").is_dir());
    fs::rename(new_root.join("0"), area._temp.path().join("moved")).unwrap();
    assert_eq!(
        manager
            .snapshots(game)
            .unwrap()
            .iter()
            .map(|s| s.number)
            .collect::<Vec<_>>(),
        vec![3]
    );
    manager.remove_game(game).unwrap();
    assert!(new_root.join("3").is_dir());
    fs::create_dir(new_root.join("6-普通目录")).unwrap();
    let readded = manager.save_game(None, new_input).unwrap();
    assert_eq!(manager.snapshots(readded).unwrap()[0].number, 3);
    manager.create_snapshot(readded, "").unwrap();
    assert!(new_root.join("7").is_dir());
}

#[test]
fn rejects_nested_paths_and_colliding_windows_names() {
    let area = GameArea::new();
    let (mut manager, game) = area.open();
    let mut input = area.input.clone();
    input.paths.push(input.paths[0].join("slot"));
    assert!(
        manager
            .save_game(Some(game), input)
            .unwrap_err()
            .to_string()
            .contains("slot")
    );
    let mut input = area.input.clone();
    input.paths.push(area._temp.path().join("different/a.INI"));
    assert!(
        manager
            .save_game(Some(game), input)
            .unwrap_err()
            .to_string()
            .contains("A.ini")
    );
    let mut input = area.input.clone();
    input
        .paths
        .push(area._temp.path().join(".SAVE-MANAGER.JSON"));
    assert!(manager.save_game(Some(game), input).is_err());
}

#[test]
fn restores_original_paths_and_completely_replaces_directory() {
    let area = GameArea::new();
    let (mut manager, game) = area.open();
    manager.create_snapshot(game, "").unwrap();
    let snapshot = manager.snapshots(game).unwrap().remove(0);
    let new_path = area._temp.path().join("other.ini");
    let input = GameInput {
        paths: vec![new_path.clone()],
        ..area.input.clone()
    };
    manager.save_game(Some(game), input).unwrap();
    fs::write(area.input.paths[0].join("extra"), "extra").unwrap();
    fs::write(&area.primary, "new config").unwrap();
    let confirmed = manager.restore_targets(game, &snapshot).unwrap();
    assert_eq!(confirmed, area.input.paths);
    assert!(manager.restore_snapshot(game, &snapshot, &[]).is_err());
    manager
        .restore_snapshot(game, &snapshot, &confirmed)
        .unwrap();
    assert!(!new_path.exists());
    assert!(!area.input.paths[0].join("extra").exists());
    assert_eq!(fs::read_to_string(&area.primary).unwrap(), "old config");
    assert!(manager.pending_recovery(game).unwrap().is_none());
}

#[cfg(windows)]
#[test]
fn restores_previous_targets_when_a_later_target_is_locked() {
    use std::os::windows::fs::OpenOptionsExt;

    let area = GameArea::new();
    let (mut manager, game) = area.open();
    manager.create_snapshot(game, "").unwrap();
    let snapshot = manager.snapshots(game).unwrap().remove(0);
    let progress = area.input.paths[0].join("slot/progress");
    fs::write(&progress, "current progress").unwrap();
    fs::write(&area.primary, "current config").unwrap();
    let locked = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&area.primary)
        .unwrap();
    let confirmed = manager.restore_targets(game, &snapshot).unwrap();
    let failure = manager
        .restore_snapshot(game, &snapshot, &confirmed)
        .unwrap_err();
    assert!(failure.to_string().contains("A.ini"));
    assert_eq!(fs::read_to_string(&progress).unwrap(), "current progress");
    assert!(manager.pending_recovery(game).unwrap().is_none());
    drop(locked);
    assert_eq!(fs::read_to_string(&area.primary).unwrap(), "current config");
}

// These fixtures start the application with on-disk state from a stopped
// process. The assertions use Manager and the resulting save files.
#[test]
fn startup_cleans_an_unpublished_snapshot_even_after_root_was_offline() {
    let area = GameArea::new();
    let (manager, game) = area.open();
    let id = uuid::Uuid::new_v4();
    let staging_name = format!(".save-manager-create-{id}");
    let staging = area.input.root.join(&staging_name);
    fs::create_dir(&staging).unwrap();
    fs::write(staging.join("incomplete"), "staged").unwrap();
    write_fixture(
        area.state.join(format!("create-{game}.json")),
        json!({
            "root": area.input.root, "id": id, "number": 0, "directory": "0",
        }),
    );
    drop(manager);

    let offline = area._temp.path().join("offline");
    fs::rename(&area.input.root, &offline).unwrap();
    let manager = Manager::open(&area.state).unwrap();
    assert!(!manager.notices().is_empty());
    drop(manager);

    fs::rename(offline, &area.input.root).unwrap();
    let mut manager = Manager::open(&area.state).unwrap();
    assert!(manager.notices().is_empty());
    assert!(!staging.exists());
    assert!(manager.snapshots(game).unwrap().is_empty());
    manager.create_snapshot(game, "").unwrap();
    assert_eq!(manager.snapshots(game).unwrap()[0].number, 0);
}

#[test]
fn startup_advances_number_for_a_snapshot_published_before_interruption() {
    let area = GameArea::new();
    let (manager, game) = area.open();
    let id = uuid::Uuid::new_v4();
    let published = area.input.root.join("0");
    fs::create_dir(&published).unwrap();
    copy_directory(&area.input.paths[0], &published.join("saves"));
    fs::copy(&area.primary, published.join("A.ini")).unwrap();
    write_fixture(
        published.join(".save-manager.json"),
        json!({
            "version": 1, "id": id, "number": 0, "created": 1, "paths": area.input.paths,
        }),
    );
    write_fixture(
        area.state.join(format!("create-{game}.json")),
        json!({
            "root": area.input.root, "id": id, "number": 0, "directory": "0",
        }),
    );
    drop(manager);

    let mut manager = Manager::open(&area.state).unwrap();
    assert!(manager.notices().is_empty());
    assert_eq!(manager.snapshots(game).unwrap()[0].id, id);
    fs::rename(published, area._temp.path().join("moved")).unwrap();
    manager.create_snapshot(game, "").unwrap();
    assert_eq!(manager.snapshots(game).unwrap()[0].number, 1);
}

#[test]
fn restart_preserves_a_modified_target_and_can_retry_the_remaining_restore() {
    let area = GameArea::new();
    let (mut manager, game) = area.open();
    manager.create_snapshot(game, "").unwrap();
    let original = manager.snapshots(game).unwrap().remove(0);
    let work = area._temp.path().join("restore-work");
    let second_work = area._temp.path().join("second-work");
    fs::create_dir(&work).unwrap();
    fs::create_dir(&second_work).unwrap();
    fs::write(work.join("original"), "previous config").unwrap();
    fs::write(&area.primary, "changed after restart").unwrap();
    let second = area._temp.path().join("other.ini");
    fs::write(second_work.join("original"), "previous other config").unwrap();
    write_fixture(
        area.state.join(format!("restore-{game}.json")),
        json!({
            "phase": "Applying",
            "entries": [
                {"target": second, "work": second_work, "had_original": true, "incoming_fingerprint": null},
                {"target": area.primary, "work": work, "had_original": true, "incoming_fingerprint": null}
            ],
            "missing_parents": [],
        }),
    );
    drop(manager);

    let mut manager = Manager::open(&area.state).unwrap();
    let recovery = manager.pending_recovery(game).unwrap().unwrap();
    assert_eq!(recovery.targets, vec![second.clone(), area.primary.clone()]);
    assert!(manager.restore_targets(game, &original).is_err());
    assert!(manager.create_snapshot(game, "").is_err());
    let error = manager.recover_restore(game).unwrap_err();
    assert!(
        error
            .to_string()
            .contains(&work.join("original").display().to_string())
    );
    assert_eq!(
        fs::read_to_string(&second).unwrap(),
        "previous other config"
    );
    assert_eq!(
        fs::read_to_string(&area.primary).unwrap(),
        "changed after restart"
    );
    fs::rename(
        &area.primary,
        area._temp.path().join("manually-preserved.ini"),
    )
    .unwrap();
    manager.recover_restore(game).unwrap();
    assert_eq!(
        fs::read_to_string(&area.primary).unwrap(),
        "previous config"
    );
    assert!(manager.pending_recovery(game).unwrap().is_none());
    manager.create_snapshot(game, "").unwrap();
}

fn write_fixture(path: PathBuf, value: serde_json::Value) {
    fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
}

#[cfg(windows)]
#[test]
#[ignore = "Uses the actual Windows Recycle Bin for a disposable snapshot"]
fn recycles_snapshot_and_keeps_its_number_consumed() {
    let area = GameArea::new();
    let (mut manager, game) = area.open();
    manager.create_snapshot(game, "回收站验证").unwrap();
    let snapshot = manager.snapshots(game).unwrap().remove(0);
    manager.delete_snapshot(game, &snapshot).unwrap();
    assert!(!snapshot.path.exists());
    assert!(manager.snapshots(game).unwrap().is_empty());
    manager.create_snapshot(game, "").unwrap();
    assert_eq!(manager.snapshots(game).unwrap()[0].number, 1);
}

#[cfg(windows)]
#[test]
#[ignore = "Opens actual Explorer windows for a disposable test directory"]
fn opens_snapshot_folder_and_selects_the_tracked_file_in_explorer() {
    let area = GameArea::new();
    let (mut manager, game) = area.open();
    manager.create_snapshot(game, "").unwrap();
    let snapshot = manager.snapshots(game).unwrap().remove(0);
    save_manager::platform::open_folder(&snapshot.path).unwrap();
    save_manager::platform::select_file(&area.primary).unwrap();
}
