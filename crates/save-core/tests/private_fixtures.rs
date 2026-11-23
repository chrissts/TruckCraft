//! Opt-in integration tests. Fixtures stay outside the repository/distribution.
use std::{
    env, fs,
    path::{Path, PathBuf},
};
use ts_save_core::{
    cargo, coaches, company, convoy,
    document::Document,
    editor::{self, VehicleKind},
    events,
    session::{Game, SaveOutcome, SaveSession},
    value,
};

#[test]
#[ignore = "requires private ETS2/ATS fixture files"]
fn real_encoded_save_recovery_restores_original_bytes_and_settings() {
    for game in [Game::Ets2, Game::Ats] {
        let (_temporary, mut session) = clone_fixture(&root(), game);
        let paths = [
            session.profile_folder().join("profile.sii"),
            session.save_folder().join("info.sii"),
            session.save_folder().join("game.sii"),
        ];
        let originals: Vec<_> = paths.iter().map(|path| fs::read(path).unwrap()).collect();
        let bank = session.data.bank_id().unwrap().to_owned();
        session
            .edit(|data| data.game.set(&bank, "money_account", "987654321"))
            .unwrap();
        let controls = session.profile_folder().join("controls.sii");
        session
            .import_settings(&[ts_save_core::settings::SettingsFile {
                name: "controls.sii".into(),
                bytes: b"Invented controls fixture".to_vec(),
            }])
            .unwrap();
        let SaveOutcome::Saved { backup, .. } = session.save().unwrap() else {
            panic!("expected write")
        };
        let edited: Vec<_> = paths.iter().map(|path| fs::read(path).unwrap()).collect();
        session.restore_recovery(&backup).unwrap();
        assert!(session.undo());
        assert!(!session.is_dirty());
        assert!(session.redo());
        let SaveOutcome::Saved { backup: second, .. } = session.save().unwrap() else {
            panic!("expected restoration")
        };
        for (path, bytes) in paths.iter().zip(&originals) {
            assert_eq!(
                fs::read(path).unwrap(),
                *bytes,
                "raw encoded/plain bytes must be restored exactly"
            );
        }
        assert!(!controls.exists());
        for (name, bytes) in ["profile.sii", "info.sii", "game.sii"].iter().zip(&edited) {
            assert_eq!(fs::read(second.join(name)).unwrap(), *bytes);
        }
        assert_eq!(
            fs::read(second.join("settings/controls.sii")).unwrap(),
            b"Invented controls fixture"
        );
        SaveSession::load(&paths[2], game).unwrap();
    }
}

#[test]
#[ignore = "requires private ETS2/ATS fixture files"]
fn real_individual_condition_and_paint_preserve_other_units_and_reload() {
    for game in [Game::Ets2, Game::Ats] {
        let (_temporary, mut session) = active_fixture(&root(), game);
        let original_profile = session.data.profile.clone();
        let mut identity = ts_save_core::profile::identity(&session.data).unwrap();
        identity.face = if identity.face == 1 { 2 } else { 1 };
        identity.logo = "logo_0".into();
        identity.male = !identity.male;
        let kind = if game == Game::Ets2 {
            VehicleKind::Coach
        } else {
            VehicleKind::Truck
        };
        let id = editor::fleet(&session.data, kind)
            .unwrap()
            .into_iter()
            .find(|entity| {
                ts_save_core::vehicle::paint(&session.data, kind, &entity.id)
                    .is_ok_and(|paint| !paint.is_empty())
            })
            .unwrap()
            .id;
        let before = session.data.game.clone();
        let mut wear = ts_save_core::vehicle::condition(&session.data, kind, &id).unwrap();
        let mut paint = ts_save_core::vehicle::paint(&session.data, kind, &id).unwrap();
        wear[0].fraction = 0.125;
        paint[0].linear = [0.125, 0.25, 0.5];
        let parts = ts_save_core::vehicle::parts(&session.data, kind, &id).unwrap();
        session
            .edit(|data| {
                ts_save_core::vehicle::set_condition(data, kind, &id, &wear)?;
                ts_save_core::vehicle::set_paint(data, kind, &id, &paint)?;
                ts_save_core::profile::set_identity(data, &identity, &[1, 2], &[0])?;
                ts_save_core::vehicle::set_parts(data, kind, &id, &parts)
            })
            .unwrap();
        let changed = session.data.game.render().unwrap().into_owned();
        assert!(session.undo());
        assert_eq!(
            session.data.game.render().unwrap(),
            before.render().unwrap()
        );
        assert!(session.redo());
        assert_eq!(session.data.game.render().unwrap(), changed);
        let mut allowed: Vec<(&str, &str)> = wear
            .iter()
            .map(|wear| (id.as_str(), wear.field.as_str()))
            .collect();
        allowed.extend(
            paint
                .iter()
                .map(|color| (color.accessory.as_str(), color.field.as_str())),
        );
        assert_only_fields_changed(&before, &session.data.game, &allowed);
        session.save().unwrap();
        let reloaded = SaveSession::load(session.save_folder().join("game.sii"), game).unwrap();
        assert_eq!(
            ts_save_core::profile::identity(&reloaded.data).unwrap(),
            identity
        );
        assert_only_fields_changed(
            &original_profile,
            &reloaded.data.profile,
            &[
                (reloaded.data.profile_id().unwrap(), "face"),
                (reloaded.data.profile_id().unwrap(), "logo"),
                (reloaded.data.profile_id().unwrap(), "male"),
            ],
        );
        assert_eq!(
            ts_save_core::vehicle::condition(&reloaded.data, kind, &id).unwrap(),
            wear
        );
        assert_eq!(
            ts_save_core::vehicle::paint(&reloaded.data, kind, &id).unwrap(),
            paint
        );
        assert_eq!(
            ts_save_core::vehicle::parts(&reloaded.data, kind, &id).unwrap(),
            parts
        );
    }
}

#[test]
#[ignore = "requires private ETS2/ATS fixture files"]
fn real_profile_clone_and_rename_keep_original_game_bytes() {
    for game in [Game::Ets2, Game::Ats] {
        let (_temporary, session) = clone_fixture(&root(), game);
        let source = session.save_folder().parent().unwrap().parent().unwrap();
        let original_profile = fs::read(source.join("profile.sii")).unwrap();
        let original_game = fs::read(session.save_folder().join("game.sii")).unwrap();
        let cloned = ts_save_core::profile_files::copy_profile(
            source,
            "TruckCraft clone",
            ts_save_core::profile_files::Operation::Clone {
                autosave_only: false,
            },
        )
        .unwrap();
        let clone = SaveSession::load(cloned.path.join("save/1/game.sii"), game).unwrap();
        assert_eq!(clone.data.profile_name(), "TruckCraft clone");
        assert_eq!(
            fs::read(source.join("profile.sii")).unwrap(),
            original_profile
        );
        assert_eq!(
            fs::read(cloned.path.join("save/1/game.sii")).unwrap(),
            original_game
        );
        let renamed = ts_save_core::profile_files::copy_profile(
            &cloned.path,
            "TruckCraft renamed",
            ts_save_core::profile_files::Operation::Rename,
        )
        .unwrap();
        assert!(!cloned.path.exists());
        let renamed_session =
            SaveSession::load(renamed.path.join("save/1/game.sii"), game).unwrap();
        assert_eq!(renamed_session.data.profile_name(), "TruckCraft renamed");
        assert_eq!(
            fs::read(renamed.path.join("save/1/game.sii")).unwrap(),
            original_game
        );
        assert_eq!(
            fs::read(renamed.backup.unwrap().join("save/1/game.sii")).unwrap(),
            original_game
        );
    }
}

#[test]
#[ignore = "requires private ETS2/ATS fixture files"]
fn real_company_garage_driver_city_edit_save_and_reload() {
    for game in [Game::Ets2, Game::Ats] {
        let (_temporary, mut session) = clone_fixture(&root(), game);
        let economy = session.data.economy_id().unwrap().to_owned();
        let player = session.data.player_id().unwrap().to_owned();
        let original_staff = session.data.game.array(&player, "drivers").unwrap().len();
        let original_events = events::entries(&session.data.game, &economy)
            .unwrap()
            .1
            .len();
        let original_game = session.data.game.clone();
        let garage = company::garages(&session.data)
            .unwrap()
            .into_iter()
            .find(|garage| garage.status == 0)
            .unwrap();
        let city = garage.city.clone();
        let garage = garage.id;
        let driver = company::drivers(&session.data)
            .unwrap()
            .into_iter()
            .find(|driver| driver.status == company::DriverStatus::Available)
            .unwrap()
            .id;
        session
            .edit(|data| company::resize_garages(data, std::slice::from_ref(&garage), 2))
            .unwrap();
        session
            .edit(|data| company::hire(data, &driver, &garage))
            .unwrap();
        assert_eq!(
            session.data.game.array(&player, "drivers").unwrap().len(),
            original_staff + 1
        );
        assert_eq!(
            events::entries(&session.data.game, &economy)
                .unwrap()
                .1
                .len(),
            original_events + 1
        );
        session
            .edit(|data| company::set_driver(data, &driver, 25000, 63, [6; 5], None))
            .unwrap();
        session
            .edit(|data| company::maximise_hired_drivers(data).map(|_| ()))
            .unwrap();
        session
            .edit(|data| company::set_visited(data, std::slice::from_ref(&city), true))
            .unwrap();
        session.edit(|data| company::fire(data, &driver)).unwrap();
        assert_eq!(
            session.data.game.array(&player, "drivers").unwrap().len(),
            original_staff
        );
        assert_eq!(
            events::entries(&session.data.game, &economy)
                .unwrap()
                .1
                .len(),
            original_events
        );
        let hq = session.data.game.string(&player, "hq_city").unwrap();
        let original_hq = format!("garage.{hq}");
        session
            .edit(|data| ts_save_core::editor::move_headquarters(data, &city))
            .unwrap();
        assert_eq!(session.data.game.string(&player, "hq_city").unwrap(), city);
        session
            .edit(|data| ts_save_core::editor::move_headquarters(data, &hq))
            .unwrap();
        session
            .edit(|data| company::resize_garages(data, std::slice::from_ref(&garage), 0))
            .unwrap();
        let expected_touched = [
            garage.as_str(),
            driver.as_str(),
            economy.as_str(),
            player.as_str(),
            original_hq.as_str(),
        ];
        for unit in original_game.units() {
            if expected_touched.contains(&unit.id.as_ref()) {
                continue;
            }
            assert_eq!(
                session.data.game.fields(&unit.id),
                original_game.fields(&unit.id),
                "unrelated unit changed"
            );
        }
        session.save().unwrap();
        let reloaded = SaveSession::load(session.save_folder().join("game.sii"), game).unwrap();
        assert_eq!(
            reloaded
                .data
                .game
                .integer::<u32>(&driver, "experience_points")
                .unwrap(),
            ts_save_core::profile::xp_for_level(game, 150).unwrap()
        );
        assert_eq!(
            company::garages(&reloaded.data)
                .unwrap()
                .into_iter()
                .find(|entry| entry.id == garage)
                .unwrap()
                .status,
            0
        );
        assert!(
            company::cities(&reloaded.data)
                .unwrap()
                .iter()
                .any(|(entry, count)| entry == &city && *count > 0)
        );
    }
}

fn root() -> PathBuf {
    PathBuf::from(
        env::var_os("TSSET_FIXTURE_ROOT")
            .expect("set TSSET_FIXTURE_ROOT to a private fixture directory"),
    )
}

#[test]
#[ignore = "requires private ETS2/ATS fixture files"]
fn real_structured_plates_preserve_layout_and_roundtrip_edits() {
    use ts_save_core::{
        editor::{self, VehicleKind},
        plates::{Part, Plate},
    };
    for game in [Game::Ets2, Game::Ats] {
        let (_temporary, mut session) = active_fixture(&root(), game);
        let mut selected = None;
        for kind in [VehicleKind::Truck, VehicleKind::Trailer, VehicleKind::Coach] {
            if let Some(entity) = editor::fleet(&session.data, kind)
                .unwrap()
                .into_iter()
                .find(|entity| {
                    session
                        .data
                        .game
                        .value(&entity.id, "license_plate")
                        .is_some()
                })
            {
                selected = Some((kind, entity.id));
                break;
            }
        }
        let (kind, id) = selected.unwrap();
        let raw = session.data.game.string(&id, "license_plate").unwrap();
        let mut plate = Plate::parse(&raw).unwrap();
        assert_eq!(plate.render().unwrap(), raw);
        let country = plate.country.clone();
        if let Some(Part::Text(text)) = plate
            .parts
            .iter_mut()
            .find(|part| matches!(part, Part::Text(_)))
        {
            text.push('R');
        }
        plate.set_color([25, 80, 120, 255]).unwrap();
        let edited = plate.render().unwrap();
        let baseline = session.data.game.clone();
        session
            .edit(|data| editor::set_license_plate(data, kind, &id, &edited))
            .unwrap();
        assert_only_fields_changed(&baseline, &session.data.game, &[(&id, "license_plate")]);
        assert!(session.undo());
        assert_eq!(session.data.game.string(&id, "license_plate").unwrap(), raw);
        assert!(session.redo());
        session.save().unwrap();
        let reloaded = SaveSession::load(session.save_folder().join("game.sii"), game).unwrap();
        let saved =
            Plate::parse(&reloaded.data.game.string(&id, "license_plate").unwrap()).unwrap();
        assert_eq!(saved.country, country);
        assert_eq!(saved.first_color(), [25, 80, 120, 255]);
        assert_eq!(saved.render().unwrap(), edited);
    }
}

#[test]
#[ignore = "requires private ETS2/ATS fixture files"]
fn real_freight_offers_events_create_edit_remove_and_reload() {
    use ts_save_core::freight;
    for game in [Game::Ets2, Game::Ats] {
        let (_temporary, mut session) = clone_fixture(&root(), game);
        let companies = freight::companies(&session.data).unwrap();
        let all = companies
            .iter()
            .flat_map(|source| freight::offers(&session.data, source).unwrap())
            .collect::<Vec<_>>();
        // Select a normal template, never a positioned special-transport offer.
        let offer = all
            .iter()
            .find(|offer| {
                session
                    .data
                    .game
                    .array(&offer.id, "trailer_place")
                    .unwrap()
                    .is_empty()
            })
            .unwrap();
        let mut terms = offer.terms.clone();
        let now: u32 = session
            .data
            .game
            .integer(session.data.economy_id().unwrap(), "game_time")
            .unwrap();
        terms.expiration = now + 1000;
        let source = offer.source.clone();
        let template = offer.id.clone();
        let baseline = session.data.game.clone();
        let id = session
            .edit(|data| freight::create(data, &source, &template, &terms))
            .unwrap();
        assert!(session.undo());
        assert_eq!(
            session.data.game.render().unwrap(),
            baseline.render().unwrap()
        );
        assert!(session.redo());
        terms.expiration += 100;
        session
            .edit(|data| freight::update(data, &source, &id, &terms))
            .unwrap();
        session.save().unwrap();
        let mut reloaded = SaveSession::load(session.save_folder().join("game.sii"), game).unwrap();
        assert_eq!(
            freight::offers(&reloaded.data, &source)
                .unwrap()
                .iter()
                .find(|offer| offer.id == id)
                .unwrap()
                .terms,
            terms
        );
        reloaded
            .edit(|data| freight::remove(data, &source, std::slice::from_ref(&id)))
            .unwrap();
        assert_eq!(
            reloaded.data.game.array(&source, "job_offer").unwrap(),
            baseline.array(&source, "job_offer").unwrap()
        );
        reloaded.save().unwrap();
        let reloaded = SaveSession::load(reloaded.save_folder().join("game.sii"), game).unwrap();
        assert_eq!(
            reloaded.data.game.array(&source, "job_offer").unwrap(),
            baseline.array(&source, "job_offer").unwrap()
        );
    }
}
fn clone_fixture(root: &Path, game: Game) -> (tempfile::TempDir, SaveSession) {
    let source = root.join(game.name()).join("profile");
    let temporary = tempfile::tempdir().unwrap();
    let profile = temporary.path().join("profile");
    let save = profile.join("save/1");
    fs::create_dir_all(&save).unwrap();
    fs::copy(source.join("profile.sii"), profile.join("profile.sii")).unwrap();
    for name in ["game.sii", "info.sii"] {
        fs::copy(source.join("save/1").join(name), save.join(name)).unwrap();
    }
    let session = SaveSession::load(save.join("game.sii"), game).unwrap();
    (temporary, session)
}
fn active_fixture(root: &Path, game: Game) -> (tempfile::TempDir, SaveSession) {
    let (temporary, session) = clone_fixture(root, game);
    if convoy::active_owner(&session.data).unwrap().is_some() {
        return (temporary, session);
    }
    assert!(convoy::capture(&session.data, "No active vehicle").is_err());
    assert_eq!(game, Game::Ets2, "an active ATS fixture is required");
    let source = PathBuf::from(
        env::var_os("TSSET_COACH_FIXTURE")
            .expect("set TSSET_COACH_FIXTURE for the active ETS2 journey"),
    );
    let save = temporary.path().join("profile/save/1");
    for name in ["game.sii", "info.sii"] {
        fs::copy(source.join(name), save.join(name)).unwrap();
    }
    (
        temporary,
        SaveSession::load(save.join("game.sii"), game).unwrap(),
    )
}
fn assert_only_fields_changed(before: &Document, after: &Document, allowed: &[(&str, &str)]) {
    assert_eq!(before.unit_count(), after.unit_count());
    for unit in before.units() {
        assert_eq!(after.unit(&unit.id).unwrap().kind, unit.kind);
        let fields = before.fields(&unit.id);
        assert_eq!(fields.len(), after.fields(&unit.id).len());
        for (field, raw) in fields {
            if allowed.contains(&(unit.id.as_ref(), field)) {
                continue;
            }
            assert_eq!(
                after.value(&unit.id, field),
                Some(raw),
                "unrequested scalar changed"
            );
            if let Ok(array) = before.array(&unit.id, field) {
                assert_eq!(
                    array,
                    after.array(&unit.id, field).unwrap(),
                    "unrequested array changed"
                );
            }
        }
    }
}

#[test]
#[ignore = "requires private ETS2/ATS fixture files"]
fn real_save_load_edit_backup_reload_and_unchanged_data() {
    for game in [Game::Ets2, Game::Ats] {
        let (_temporary, mut session) = clone_fixture(&root(), game);
        let baseline = session.data.game.clone();
        assert_eq!(baseline.render().unwrap(), baseline.render().unwrap());
        let economy = session.data.economy_id().unwrap().to_owned();
        let bank = session.data.bank_id().unwrap().to_owned();
        let xp = baseline
            .integer::<u32>(&economy, "experience_points")
            .unwrap();
        let money = baseline.integer::<i64>(&bank, "money_account").unwrap();
        session
            .edit(|data| {
                data.game
                    .set(&economy, "experience_points", (xp + 7).to_string())?;
                data.game
                    .set(&bank, "money_account", (money + 1).to_string())
            })
            .unwrap();
        assert_only_fields_changed(
            &baseline,
            &session.data.game,
            &[(&economy, "experience_points"), (&bank, "money_account")],
        );
        let raw_game = fs::read(session.save_folder().join("game.sii")).unwrap();
        let raw_profile = fs::read(
            session
                .save_folder()
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("profile.sii"),
        )
        .unwrap();
        let SaveOutcome::Saved {
            backup,
            changed_files,
        } = session.save().unwrap()
        else {
            panic!("save did not commit")
        };
        assert_eq!(changed_files, 1);
        assert_eq!(fs::read(backup.join("game.sii")).unwrap(), raw_game);
        assert_eq!(fs::read(backup.join("profile.sii")).unwrap(), raw_profile);
        assert_only_fields_changed(
            &baseline,
            &session.data.game,
            &[(&economy, "experience_points"), (&bank, "money_account")],
        );
        assert_eq!(
            session
                .data
                .game
                .integer::<u32>(&economy, "experience_points")
                .unwrap(),
            xp + 7
        );
        assert_eq!(
            session
                .data
                .game
                .integer::<i64>(&bank, "money_account")
                .unwrap(),
            money + 1
        );
    }
}

#[test]
#[ignore = "requires private ETS2/ATS fixture files"]
fn real_cargo_convoy_roundtrip_and_rejected_edits() {
    for game in [Game::Ets2, Game::Ats] {
        let (_temporary, mut session) = active_fixture(&root(), game);
        let company = cargo::companies(&session.data).into_iter().next().unwrap();
        let clock = session
            .data
            .game
            .integer::<u32>(session.data.economy_id().unwrap(), "game_time")
            .unwrap();
        session
            .edit(|data| {
                cargo::regenerate(
                    data,
                    &company,
                    cargo::Parameters {
                        count: 10,
                        valid_minutes: 180,
                        spacing_minutes: 60,
                    },
                )
            })
            .unwrap();
        let seeds = session
            .data
            .game
            .array(&company, "cargo_offer_seeds")
            .unwrap();
        assert_eq!(seeds.len(), 10);
        for (i, seed) in seeds.iter().enumerate() {
            let seed = seed.parse::<u32>().unwrap();
            assert!((clock + 180 + i as u32 * 60..=clock + 240 + i as u32 * 60).contains(&seed));
        }
        let baseline = session.data.game.render().unwrap().into_owned();
        assert!(
            session
                .edit(|data| cargo::replace(data, &company, &[1; 65]))
                .is_err()
        );
        assert_eq!(session.data.game.render().unwrap(), baseline);
        let pack = convoy::capture(&session.data, "Example position").unwrap();
        let json = convoy::export(&pack).unwrap();
        assert_eq!(convoy::import(&json, game).unwrap(), pack);
        assert!(
            convoy::import(
                &json,
                if game == Game::Ats {
                    Game::Ets2
                } else {
                    Game::Ats
                }
            )
            .is_err()
        );
        session
            .edit(|data| convoy::apply_route(data, &pack))
            .unwrap();
        let output = session.data.game.render().unwrap().into_owned();
        let reloaded = Document::parse(output).unwrap();
        let mut data = session.data.clone();
        data.game = reloaded;
        let captured = convoy::capture(&data, "Example position").unwrap();
        assert_eq!(captured.ahead, pack.ahead);
        assert_eq!(captured.behind, pack.behind);
        assert_eq!(captured.avoid, pack.avoid);
        let mut moved = pack.clone();
        moved.positions[0].placement = "(1, 2, 3) (1; 0, 0, 0)".into();
        session
            .edit(|data| convoy::apply_position(data, &moved, 0))
            .unwrap();
        assert_eq!(
            convoy::capture(&session.data, "Moved").unwrap().positions[0].placement,
            moved.positions[0].placement
        );
        moved.positions[0]
            .trailer_placements
            .get_or_insert_with(Vec::new)
            .push("(0, 0, 0) (1; 0, 0, 0)".into());
        let before = session.data.game.render().unwrap().into_owned();
        assert!(
            session
                .edit(|data| convoy::apply_position(data, &moved, 0))
                .is_err()
        );
        assert_eq!(session.data.game.render().unwrap(), before);
        let mut empty = pack;
        empty.ahead.clear();
        empty.behind.clear();
        empty.avoid.clear();
        session
            .edit(|data| convoy::apply_route(data, &empty))
            .unwrap();
        assert!(
            convoy::capture(&session.data, "Empty")
                .unwrap()
                .ahead
                .is_empty()
        );
        session.save().unwrap();
    }
}

#[test]
#[ignore = "requires a private active-coach fixture"]
fn real_coach_journey_controls_and_dlc_gate() {
    let root = root();
    let (temporary, initial) = clone_fixture(&root, Game::Ets2);
    drop(initial);
    let source =
        PathBuf::from(env::var_os("TSSET_COACH_FIXTURE").expect("set TSSET_COACH_FIXTURE"));
    let save = temporary.path().join("profile/save/1");
    for name in ["game.sii", "info.sii"] {
        fs::copy(source.join(name), save.join(name)).unwrap();
    }
    let mut session = SaveSession::load(save.join("game.sii"), Game::Ets2).unwrap();
    let state = coaches::detect(&session.data).unwrap();
    let coaches::CoachState::Ready {
        stages, passengers, ..
    } = state
    else {
        panic!("active coach fixture required")
    };
    assert_eq!(stages.len(), 2);
    assert_eq!(passengers.len(), 24);
    let coach = editor::fleet(&session.data, VehicleKind::Coach).unwrap()[0]
        .id
        .clone();
    session
        .edit(|data| {
            coaches::set_progress(data, 1234, [1, 2, 3, 2, 1, 3])?;
            editor::set_fuel(data, VehicleKind::Coach, &coach, 50.0)?;
            editor::repair(data, VehicleKind::Coach, &coach, true)?;
            coaches::set_satisfaction(data, 75.0)?;
            coaches::extend_trip(data, 15)
        })
        .unwrap();
    assert_eq!(editor::fuel(&session.data, &coach).unwrap(), 50.0);
    for passenger in &passengers {
        assert_eq!(
            value::float(session.data.game.value(passenger, "satisfaction").unwrap()).unwrap(),
            0.75
        );
    }
    session.save().unwrap();
    assert!(coaches::detect(&session.data).unwrap().ready());
    assert_eq!(
        session
            .data
            .game
            .integer::<u8>(session.data.economy_id().unwrap(), "ecodriving")
            .unwrap(),
        3
    );
    assert_eq!(editor::fuel(&session.data, &coach).unwrap(), 50.0);
    session
        .edit(|data| {
            let id = data.info.first_id("save_container")?.to_owned();
            data.info.set_array(&id, "dependencies", Vec::new())
        })
        .unwrap();
    let before = session.data.game.render().unwrap().into_owned();
    assert!(
        session
            .edit(|data| coaches::set_progress(data, 1, [0; 6]))
            .is_err()
    );
    assert_eq!(session.data.game.render().unwrap(), before);
}
