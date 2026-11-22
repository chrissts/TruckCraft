//! Opt-in car fixture tests. Always copy the supplied private fixture before writes.
use std::{env, fs, path::PathBuf};
use ts_save_core::{
    cars::{self, CarState},
    editor::{self, VehicleKind},
    session::{Game, SaveSession},
};

#[test]
#[ignore = "requires a private ATS Road Trip save fixture"]
fn saved_road_trip_car_schema_is_supported() {
    let source = PathBuf::from(
        env::var_os("TSSET_CAR_FIXTURE")
            .expect("set TSSET_CAR_FIXTURE to the copied private profile"),
    );
    let temporary = tempfile::tempdir().unwrap();
    let profile = temporary.path().join("profile");
    let save = profile.join("save/1");
    fs::create_dir_all(&save).unwrap();
    fs::copy(source.join("profile.sii"), profile.join("profile.sii")).unwrap();
    for name in ["game.sii", "info.sii"] {
        fs::copy(source.join("save/1").join(name), save.join(name)).unwrap();
    }
    let originals: Vec<_> = ["profile.sii", "save/1/game.sii", "save/1/info.sii"]
        .into_iter()
        .map(|name| (name, fs::read(source.join(name)).unwrap()))
        .collect();
    let mut session = SaveSession::load(save.join("game.sii"), Game::Ats).unwrap();
    let CarState::Ready { vehicle, job } = cars::detect(&session.data).unwrap() else {
        panic!("fixture must contain an active Road Trip journey");
    };
    assert_eq!(session.data.format_version().unwrap(), 105);
    assert!(
        !editor::fleet(&session.data, VehicleKind::Truck)
            .unwrap()
            .iter()
            .any(|v| v.id == vehicle)
    );
    let before = session.data.game.render().unwrap().into_owned();
    let original_document = session.data.game.clone();
    session
        .edit(|data| {
            let colors = ts_save_core::vehicle::paint(data, VehicleKind::Car, &vehicle)?;
            assert!(!colors.is_empty());
            ts_save_core::vehicle::set_paint(data, VehicleKind::Car, &vehicle, &colors)?;
            editor::set_fuel(data, VehicleKind::Car, &vehicle, 87.0)?;
            editor::repair(data, VehicleKind::Car, &vehicle, true)?;
            editor::set_license_plate(data, VehicleKind::Car, &vehicle, "TRIP 62")
        })
        .unwrap();
    let after = session.data.game.render().unwrap().into_owned();
    assert_ne!(before, after);
    assert!(session.undo());
    assert_eq!(session.data.game.render().unwrap(), before);
    assert!(session.redo());
    assert_eq!(session.data.game.render().unwrap(), after);
    session.save().unwrap();
    let reloaded = SaveSession::load(save.join("game.sii"), Game::Ats).unwrap();
    assert!(cars::detect(&reloaded.data).unwrap().ready());
    let condition =
        ts_save_core::vehicle::condition(&reloaded.data, VehicleKind::Car, &vehicle).unwrap();
    assert!(!condition.is_empty());
    assert!(condition.iter().all(|wear| wear.fraction == 0.0));
    assert!((editor::fuel(&reloaded.data, &vehicle).unwrap() - 87.0).abs() < 0.01);
    for unit in original_document
        .units()
        .filter(|unit| unit.id.as_ref() != vehicle)
    {
        assert_eq!(
            reloaded.data.game.fields(&unit.id),
            original_document.fields(&unit.id)
        );
    }
    assert!(reloaded.data.game.unit(&job).is_some());
    assert_eq!(
        reloaded.data.game.value(&vehicle, "odometer"),
        original_document.value(&vehicle, "odometer")
    );
    for (name, bytes) in originals {
        assert_eq!(fs::read(source.join(name)).unwrap(), bytes);
    }
}
