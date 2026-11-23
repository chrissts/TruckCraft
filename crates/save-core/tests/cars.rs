use ts_save_core::{
    cars::{self, CarState},
    document::Document,
    editor::{self, VehicleKind},
    session::{Game, SaveData},
};

fn fixture() -> SaveData {
    SaveData {
        simulator: Game::Ats,
        profile: Document::parse("SiiNunit\n{\n}\n").unwrap(),
        info: Document::parse("SiiNunit\n{\nsave_container : info.test {\n dependencies: 1\n dependencies[0]: \"dlc|ats_r_ford|DLC - Road Trip: Ford\"\n}\n}\n").unwrap(),
        game: Document::parse("SiiNunit\n{\neconomy : economy.test {\n player: player.test\n}\nplayer : player.test {\n assigned_vehicles_mode: \"car\"\n assigned_vehicles: vehicles.test\n current_car_job: job.test\n cars: 0\n trucks: 1\n trucks[0]: truck.test\n}\nplayer_vehicles : vehicles.test {\n vehicle: car.test\n trailer: null\n}\nvehicle : car.test {\n fuel_relative: 0.5\n engine_wear: 0.4\n odometer: 123456\n future_field: \"preserve\"\n}\nvehicle : truck.test {\n fuel_relative: 0.2\n}\nplayer_car_job : job.test {\n company_car: car.test\n future_job_field: 99\n}\n}\n").unwrap(),
    }
}

#[test]
fn car_journey_is_distinct_from_trucks_and_edits_preserve_unknown_fields() {
    let mut data = fixture();
    assert!(cars::detect(&data).unwrap().ready());
    assert_eq!(
        editor::fleet(&data, VehicleKind::Truck).unwrap()[0].id,
        "truck.test"
    );
    assert_eq!(editor::fleet(&data, VehicleKind::Truck).unwrap().len(), 1);
    assert!(editor::fleet(&data, VehicleKind::Coach).unwrap().is_empty());
    assert_eq!(
        editor::fleet(&data, VehicleKind::Car).unwrap()[0].id,
        "car.test"
    );
    assert!(editor::set_fuel(&mut data, VehicleKind::Car, "truck.test", 100.0).is_err());
    editor::set_fuel(&mut data, VehicleKind::Car, "car.test", 100.0).unwrap();
    editor::repair(&mut data, VehicleKind::Car, "car.test", false).unwrap();
    assert_eq!(data.game.value("car.test", "engine_wear"), Some("0"));
    assert_eq!(data.game.value("car.test", "odometer"), Some("123456"));
    assert_eq!(
        data.game.value("car.test", "future_field"),
        Some("\"preserve\"")
    );
    assert_eq!(data.game.value("job.test", "future_job_field"), Some("99"));
}

#[test]
fn dlc_active_car_and_saved_journey_are_required_before_writes() {
    for case in 0..4 {
        let mut data = fixture();
        let expected = match case {
            0 => {
                data.simulator = Game::Ets2;
                CarState::WrongGame
            }
            1 => {
                data.info
                    .set_array("info.test", "dependencies", Vec::new())
                    .unwrap();
                CarState::MissingDlc
            }
            2 => {
                data.game
                    .set("player.test", "current_car_job", "null")
                    .unwrap();
                CarState::NoJourney
            }
            _ => {
                data.game
                    .set_string("player.test", "assigned_vehicles_mode", "truck")
                    .unwrap();
                CarState::NoActiveCar
            }
        };
        assert_eq!(cars::detect(&data).unwrap(), expected);
        let before = data.game.render().unwrap().into_owned();
        assert!(editor::set_fuel(&mut data, VehicleKind::Car, "car.test", 100.0).is_err());
        assert_eq!(data.game.render().unwrap(), before);
    }
}

#[test]
fn malformed_car_references_reject_without_partial_changes() {
    for (unit, field, target) in [
        ("player.test", "assigned_vehicles", "car.test"),
        ("vehicles.test", "vehicle", "missing.test"),
        ("player.test", "current_car_job", "car.test"),
        ("job.test", "company_car", "truck.test"),
    ] {
        let mut data = fixture();
        data.game.set(unit, field, target).unwrap();
        let before = data.game.render().unwrap().into_owned();
        assert!(cars::detect(&data).is_err());
        assert!(editor::repair(&mut data, VehicleKind::Car, "car.test", true).is_err());
        assert_eq!(data.game.render().unwrap(), before);
    }
}
