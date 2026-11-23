//! Opt-in, read-only validation. Game installations are never modified.
use std::{env, fs, path::PathBuf};
use ts_save_core::{
    editor::VehicleKind,
    game_data::Progression,
    profile,
    session::{Game, SAVE_FORMAT_TARGET, SaveSession},
    starter,
};

#[test]
#[ignore = "requires private ETS2/ATS game installation paths"]
fn create_starter_from_installed_games() {
    for (game, variable) in [
        (Game::Ets2, "TRUCKCRAFT_ETS2_GAME_DIR"),
        (Game::Ats, "TRUCKCRAFT_ATS_GAME_DIR"),
    ] {
        let installation = PathBuf::from(env::var_os(variable).expect(variable));
        let scratch = tempfile::tempdir().unwrap();
        let game_home = scratch.path().join(game.folder());
        fs::create_dir(&game_home).unwrap();
        let name = "Starter \"QA\"";
        let path = starter::create(game, &installation, &game_home, name).unwrap();
        let session = SaveSession::load(&path, game).unwrap();
        assert_eq!(session.data.profile_name(), name);
        assert_eq!(session.data.format_version().unwrap(), SAVE_FORMAT_TARGET);
        let profile = session.data.profile_id().unwrap();
        let model = session.data.profile.required(profile, "brand").unwrap();
        let truck_pack = match game {
            Game::Ets2 => "def.scs".to_owned(),
            Game::Ats => format!("dlc_{model}.scs"),
        };
        let archive = scs_codec::Archive::open(installation.join(truck_pack)).unwrap();
        assert!(
            !archive
                .read_path_limited(&format!("def/vehicle/truck_desktop/{model}.sii"), 64 * 1024)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            session
                .data
                .game
                .value(session.data.economy_id().unwrap(), "new_game"),
            Some("true")
        );
        let original = fs::read(&path).unwrap();
        assert!(starter::create(game, &installation, &game_home, name).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(fs::read_dir(game_home.join("profiles")).unwrap().count(), 1);
    }
}

#[test]
#[ignore = "requires private ETS2/ATS game installation paths"]
fn installed_game_progression_matches_verified_fallbacks() {
    for (game, variable) in [
        (Game::Ets2, "TRUCKCRAFT_ETS2_GAME_DIR"),
        (Game::Ats, "TRUCKCRAFT_ATS_GAME_DIR"),
    ] {
        let folder = PathBuf::from(
            env::var_os(variable).expect("set both private game installation variables"),
        );
        let data = Progression::load(game, &folder).unwrap();
        assert_eq!(data.truck_ranks, [6; 5]);
        assert_eq!(data.adr_ranks, 6);
        assert_eq!(data.coach_ranks, (game == Game::Ets2).then_some([3; 6]));
        for level in 0..=150 {
            let actual = data.truck_xp.xp_for_level(level).unwrap();
            assert_eq!(
                actual,
                profile::xp_for_level(game, level).unwrap(),
                "fallback XP mismatch at level {level}"
            );
            assert_eq!(data.truck_xp.level_for_xp(actual), level);
            assert_eq!(data.truck_xp.level_for_xp(actual + 1), level + 1);
        }
        if let Some(coach) = &data.coach_xp {
            for level in 0..=150 {
                let xp = coach.xp_for_level(level).unwrap();
                assert_eq!(coach.level_for_xp(xp), level);
            }
        }
        assert!(data.definition_files >= 2);
        assert!(!data.plate_assets.regions.is_empty());
        assert!(!data.plate_assets.badges.is_empty());
        let (region, other) = match game {
            Game::Ets2 => ("germany", "uk"),
            Game::Ats => ("california", "nevada"),
        };
        let background = data
            .plate_assets
            .background(region, VehicleKind::Truck)
            .unwrap_or_else(|| panic!("Missing {region} truck plate background"));
        assert_eq!(
            background.rgba.len(),
            background.size.iter().product::<usize>() * 4
        );
        let other_background = data
            .plate_assets
            .background(other, VehicleKind::Truck)
            .unwrap_or_else(|| panic!("Missing {other} truck plate background"));
        assert_ne!(background.rgba, other_background.rgba);
        if game == Game::Ats {
            let california_car = data
                .plate_assets
                .background(region, VehicleKind::Car)
                .expect("ATS California passenger-car artwork");
            let nevada_car = data
                .plate_assets
                .background(other, VehicleKind::Car)
                .expect("ATS Nevada passenger-car artwork");
            assert_ne!(california_car.rgba, nevada_car.rgba);
            assert_ne!(california_car.rgba, background.rgba);
        }
        let archive = scs_codec::Archive::open(folder.join("def.scs")).unwrap();
        let catalog = ts_save_core::definitions::Catalog::from_archive(
            &archive,
            &["def/cargo.sii", "def/economy_data.sii"],
        )
        .unwrap();
        assert!(
            catalog
                .units
                .values()
                .filter(|unit| unit.kind == "cargo_data")
                .count()
                > 20
        );
        assert!(catalog.files.len() > 20);
    }
}
