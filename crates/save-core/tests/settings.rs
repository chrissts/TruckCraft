use std::{fs, path::PathBuf};
use ts_save_core::{
    Error,
    session::{Game, SaveOutcome, SaveSession},
    settings::{self, SettingsFile},
};

const PROFILE: &str = "SiiNunit\n{\nuser_profile : profile.test {\n profile_name: \"Example\"\n future_field: 42\n}\n}\n";
const INFO: &str =
    "SiiNunit\n{\nsave_container : info.test {\n version: 105\n name: \"Example\"\n}\n}\n";
const GAME: &str = "SiiNunit\n{\neconomy : economy.test {\n player: player.test\n bank: bank.test\n}\nplayer : player.test {\n future_field: 123\n}\nbank : bank.test {\n money_account: 100\n}\n}\n";

fn fixture() -> (tempfile::TempDir, PathBuf, SaveSession) {
    let temporary = tempfile::tempdir().unwrap();
    let profile = temporary.path().join("profile");
    let save = profile.join("save/1");
    fs::create_dir_all(&save).unwrap();
    fs::write(profile.join("profile.sii"), PROFILE).unwrap();
    fs::write(save.join("info.sii"), INFO).unwrap();
    fs::write(save.join("game.sii"), GAME).unwrap();
    fs::write(profile.join("config.cfg"), b"uset example \"old\"\r\n").unwrap();
    let session = SaveSession::load(save.join("game.sii"), Game::Ets2).unwrap();
    (temporary, profile, session)
}
fn files() -> Vec<SettingsFile> {
    vec![
        SettingsFile {
            name: "config.cfg".into(),
            bytes: b"uset example \"new\"\r\n".to_vec(),
        },
        SettingsFile {
            name: "controls.sii".into(),
            bytes: vec![0, 255, 42, 13, 10],
        },
    ]
}

#[test]
fn bundles_preserve_bytes_and_reject_cross_game_paths_duplicates_and_invalid_data() {
    for game in [Game::Ets2, Game::Ats] {
        let text = settings::export(game, &files()).unwrap();
        assert_eq!(settings::import(game, &text).unwrap(), files());
        assert!(
            settings::import(
                if game == Game::Ets2 {
                    Game::Ats
                } else {
                    Game::Ets2
                },
                &text
            )
            .is_err()
        );
        for bad in [
            text.replace("config.cfg", "../config.cfg"),
            text.replace("controls.sii", "config.cfg"),
            text.replace("\"truckcraft_settings\":1", "\"truckcraft_settings\":2"),
            text.replace("00ff", "zzff"),
        ] {
            assert!(settings::import(game, &bad).is_err());
        }
    }
    for name in [
        "profile.sii",
        "game.sii",
        "../config.cfg",
        "C:/config.cfg",
        "gearbox_../evil.sii",
        "controls.exe",
        "gearbox_a.sii:stream",
    ] {
        assert!(!settings::valid_name(name));
    }
    assert!(
        settings::export(
            Game::Ets2,
            &[SettingsFile {
                name: "config.cfg".into(),
                bytes: vec![0; settings::MAX_FILE + 1]
            }]
        )
        .is_err()
    );
}

#[test]
fn settings_and_save_edits_share_history_and_one_recoverable_commit() {
    let (_temporary, profile, mut session) = fixture();
    let original = fs::read(profile.join("config.cfg")).unwrap();
    session.import_settings(&files()).unwrap();
    assert!(session.is_dirty());
    assert!(session.can_save());
    assert!(
        !session.data.is_dirty(),
        "settings must not force unrelated save documents dirty"
    );
    assert_eq!(fs::read(profile.join("config.cfg")).unwrap(), original);
    assert!(!profile.join("controls.sii").exists());
    session
        .edit(|data| data.game.set("bank.test", "money_account", "200"))
        .unwrap();
    assert!(session.undo());
    assert_eq!(
        session.data.game.value("bank.test", "money_account"),
        Some("100")
    );
    assert!(session.is_dirty());
    assert!(session.undo());
    assert!(!session.is_dirty());
    assert!(session.redo());
    assert!(session.redo());
    let SaveOutcome::Saved {
        backup,
        changed_files,
    } = session.save().unwrap()
    else {
        panic!("expected write")
    };
    assert_eq!(changed_files, 3);
    assert_eq!(
        fs::read(backup.join("settings/config.cfg")).unwrap(),
        original
    );
    assert_eq!(fs::read(backup.join("game.sii")).unwrap(), GAME.as_bytes());
    assert_eq!(
        fs::read(backup.join("profile.sii")).unwrap(),
        PROFILE.as_bytes()
    );
    assert_eq!(
        fs::read_to_string(backup.join("settings-absent.json")).unwrap(),
        "[\"controls.sii\"]"
    );
    for file in files() {
        assert_eq!(fs::read(profile.join(file.name)).unwrap(), file.bytes);
    }
    assert!(!session.is_dirty());
    assert!(!session.can_undo());
    assert_eq!(session.settings_files().unwrap(), files());
}

#[test]
fn settings_external_changes_and_new_file_collisions_refuse_all_writes() {
    for new_file in [false, true] {
        let (_temporary, profile, mut session) = fixture();
        session.import_settings(&files()).unwrap();
        session
            .edit(|data| data.game.set("bank.test", "money_account", "200"))
            .unwrap();
        let path = profile.join(if new_file {
            "controls.sii"
        } else {
            "config.cfg"
        });
        let before = fs::metadata(profile.join("config.cfg"))
            .unwrap()
            .modified()
            .unwrap();
        fs::write(&path, b"external content").unwrap();
        if !new_file {
            fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_times(fs::FileTimes::new().set_modified(before))
                .unwrap();
        }
        assert!(matches!(session.save(), Err(Error::FileChanged)));
        assert_eq!(fs::read(&path).unwrap(), b"external content");
        assert_eq!(
            fs::read(session.save_folder().join("game.sii")).unwrap(),
            GAME.as_bytes()
        );
        assert!(!session.save_folder().join("tsset-backups").exists());
    }
}

#[test]
fn restore_is_staged_exact_undoable_and_retains_a_recovery_of_current_files() {
    let (_temporary, profile, mut session) = fixture();
    let original = fs::read(profile.join("config.cfg")).unwrap();
    session.import_settings(&files()).unwrap();
    session
        .edit(|data| data.game.set("bank.test", "money_account", "200"))
        .unwrap();
    let SaveOutcome::Saved { backup, .. } = session.save().unwrap() else {
        panic!()
    };
    assert_eq!(session.recovery_snapshots().unwrap(), vec![backup.clone()]);
    assert_eq!(session.restore_recovery(&backup).unwrap(), 3);
    assert!(session.is_dirty());
    assert_eq!(
        session.data.game.value("bank.test", "money_account"),
        Some("100")
    );
    assert_eq!(
        fs::read(profile.join("config.cfg")).unwrap(),
        files()[0].bytes
    );
    assert!(session.undo());
    assert!(!session.is_dirty());
    assert_eq!(
        session.data.game.value("bank.test", "money_account"),
        Some("200")
    );
    assert!(session.redo());
    let SaveOutcome::Saved { backup: second, .. } = session.save().unwrap() else {
        panic!()
    };
    assert_ne!(second, backup);
    assert_eq!(
        fs::read(second.join("settings/controls.sii")).unwrap(),
        files()[1].bytes
    );
    assert_eq!(fs::read(profile.join("config.cfg")).unwrap(), original);
    assert!(!profile.join("controls.sii").exists());
    assert_eq!(
        fs::read(session.save_folder().join("game.sii")).unwrap(),
        GAME.as_bytes()
    );
    session.restore_recovery(&second).unwrap();
    session.save().unwrap();
    assert_eq!(
        fs::read(profile.join("controls.sii")).unwrap(),
        files()[1].bytes
    );
}

#[test]
fn malformed_or_unrelated_recovery_does_not_discard_staged_data() {
    let (_temporary, _profile, mut session) = fixture();
    let other = tempfile::tempdir().unwrap();
    assert!(session.restore_recovery(other.path()).is_err());
    session
        .edit(|data| data.game.set("bank.test", "money_account", "200"))
        .unwrap();
    let SaveOutcome::Saved { backup, .. } = session.save().unwrap() else {
        panic!()
    };
    fs::write(backup.join("game.sii"), b"invalid").unwrap();
    assert!(session.restore_recovery(&backup).is_err());
    assert!(!session.is_dirty());
    assert!(!session.can_undo());
    session.import_settings(&files()).unwrap();
    assert!(session.restore_recovery(&backup).is_err());
    assert!(session.is_dirty());
    assert_eq!(session.settings_files().unwrap(), files());
}

#[cfg(windows)]
#[test]
fn locked_settings_roll_back_earlier_save_and_settings_writes() {
    use std::os::windows::fs::OpenOptionsExt;
    let (_temporary, profile, mut session) = fixture();
    fs::write(profile.join("controls.sii"), b"original controls").unwrap();
    let mut incoming = files();
    incoming.push(SettingsFile {
        name: "config_local.cfg".into(),
        bytes: b"new local config".to_vec(),
    });
    session.import_settings(&incoming).unwrap();
    session
        .edit(|data| data.game.set("bank.test", "money_account", "200"))
        .unwrap();
    let _lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(profile.join("controls.sii"))
        .unwrap();
    assert!(matches!(
        session.save(),
        Err(Error::Commit { restored: true, .. })
    ));
    assert!(session.reload_required());
    assert_eq!(
        fs::read(session.save_folder().join("game.sii")).unwrap(),
        GAME.as_bytes()
    );
    assert_eq!(
        fs::read(profile.join("config.cfg")).unwrap(),
        b"uset example \"old\"\r\n"
    );
    assert_eq!(
        fs::read(profile.join("controls.sii")).unwrap(),
        b"original controls"
    );
    assert!(
        !profile.join("config_local.cfg").exists(),
        "rollback must remove files absent before this transaction"
    );
}

#[cfg(windows)]
#[test]
fn settings_and_recovery_refuse_profile_junctions() {
    let (temporary, profile, mut session) = fixture();
    let outside = tempfile::tempdir().unwrap();
    let link = profile.join("controls.sii");
    // Directory junctions need no symlink privilege on Windows.
    let output = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(outside.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "mklink failed: {output:?}");
    assert!(session.import_settings(&files()).is_err());
    assert!(!session.is_dirty());
    assert!(fs::read_dir(outside.path()).unwrap().next().is_none());
    fs::remove_dir(&link).unwrap();
    let profile_link = temporary.path().join("linked-profile");
    let output = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&profile_link)
        .arg(&profile)
        .output()
        .unwrap();
    assert!(output.status.success(), "mklink failed: {output:?}");
    let mut linked = SaveSession::load(profile_link.join("save/1/game.sii"), Game::Ets2).unwrap();
    assert!(linked.import_settings(&files()).is_err());
    fs::remove_dir(&profile_link).unwrap();
}
