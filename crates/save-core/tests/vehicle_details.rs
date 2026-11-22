use ts_save_core::{
    document::Document,
    editor::VehicleKind,
    session::{Game, SaveData},
    vehicle,
};

fn fixture() -> SaveData {
    SaveData {
        simulator:Game::Ets2,
        profile:Document::parse("SiiNunit\n{\n}\n").unwrap(),
        info:Document::parse("SiiNunit\n{\n}\n").unwrap(),
        game:Document::parse("SiiNunit\n{\neconomy : economy.test {\n player: player.test\n}\nplayer : player.test {\n trucks: 1\n trucks[0]: truck.test\n}\nvehicle : truck.test {\n fuel_relative: 0.5\n engine_wear: &3e800000\n engine_wear_unfixable: 0.1\n wheels_wear: 2\n wheels_wear[0]: 0.2\n wheels_wear[1]: 0.3\n odometer: 500001\n integrity_odometer: 12345\n accessories: 2\n accessories[0]: paint.test\n accessories[1]: addon.test\n future_field: \"retain\"\n}\nvehicle_paint_job_accessory : paint.test {\n base_color: (&3f000000, 0.25, 1)\n mask_r_color: (1, 0, 0)\n data_path: \"/def/vehicle/truck/sample/paint_job/plain.sii\"\n future_paint_field: 3\n}\nvehicle_addon_accessory : addon.test {\n data_path: \"/def/vehicle/truck/sample/mirror/small.sii\"\n refund: 999\n future_addon_field: \"retain\"\n}\nvehicle_addon_accessory : choice.test {\n data_path: \"/def/vehicle/truck/sample/mirror/large.sii\"\n}\nvehicle_addon_accessory : wrong.test {\n data_path: \"/def/vehicle/truck/other/mirror/large.sii\"\n}\n}\n").unwrap(),
    }
}

#[test]
fn individual_wear_changes_only_requested_existing_components() {
    let mut data = fixture();
    let mut edits = vehicle::condition(&data, VehicleKind::Truck, "truck.test").unwrap();
    assert_eq!(edits.len(), 4);
    edits[0].fraction = 0.75;
    edits[3].fraction = 0.9;
    vehicle::set_condition(&mut data, VehicleKind::Truck, "truck.test", &edits).unwrap();
    let actual = vehicle::condition(&data, VehicleKind::Truck, "truck.test").unwrap();
    assert_eq!(actual, edits);
    assert_eq!(
        data.game.value("truck.test", "engine_wear_unfixable"),
        Some("0.1")
    );
    assert_eq!(
        data.game.array("truck.test", "wheels_wear").unwrap()[0],
        "0.2"
    );
    for (field, value) in [
        ("odometer", "500001"),
        ("integrity_odometer", "12345"),
        ("future_field", "\"retain\""),
    ] {
        assert_eq!(data.game.value("truck.test", field), Some(value));
    }
    for bad in [f32::NAN, -0.1, 1.1] {
        let before = data.game.render().unwrap().into_owned();
        edits[0].fraction = bad;
        assert!(
            vehicle::set_condition(&mut data, VehicleKind::Truck, "truck.test", &edits).is_err()
        );
        assert_eq!(data.game.render().unwrap(), before);
    }
    edits[0].fraction = 0.3;
    edits[1].field = "odometer".into();
    assert!(vehicle::set_condition(&mut data, VehicleKind::Truck, "truck.test", &edits).is_err());
}

#[test]
fn paint_palette_import_is_atomic_and_retains_material_and_unknown_fields() {
    let mut data = fixture();
    let mut colors = vehicle::paint(&data, VehicleKind::Truck, "truck.test").unwrap();
    let original = colors.clone();
    let sharing = vehicle::export_paint(&colors, "paint.test").unwrap();
    colors[0].linear = [0.1, 0.2, 0.3];
    vehicle::set_paint(&mut data, VehicleKind::Truck, "truck.test", &colors).unwrap();
    assert_eq!(
        vehicle::paint(&data, VehicleKind::Truck, "truck.test").unwrap(),
        colors
    );
    assert_eq!(
        data.game.value("paint.test", "mask_r_color"),
        Some("(1, 0, 0)")
    );
    assert_eq!(
        data.game.value("paint.test", "future_paint_field"),
        Some("3")
    );
    assert_eq!(
        data.game.string("paint.test", "data_path").unwrap(),
        "/def/vehicle/truck/sample/paint_job/plain.sii"
    );
    vehicle::import_paint(&sharing, &mut colors, "paint.test").unwrap();
    assert_eq!(colors, original);
    for bad in [
        sharing.replace("base_color", "future_color"),
        sharing.replace("0.5", "-0.5"),
        sharing.replace("\"truckcraft_paint\": 1", "\"truckcraft_paint\": 2"),
        "x".repeat(4097),
    ] {
        let before = colors.clone();
        assert!(vehicle::import_paint(&bad, &mut colors, "paint.test").is_err());
        assert_eq!(colors, before);
    }
}

#[test]
fn addon_choices_cannot_change_other_models_or_structural_parts() {
    let mut data = fixture();
    let mut parts = vehicle::parts(&data, VehicleKind::Truck, "truck.test").unwrap();
    let choices = vehicle::part_choices(&data, &parts[1]);
    assert_eq!(choices.len(), 2);
    assert!(!choices.iter().any(|p| p.contains("/other/")));
    parts[1].path = "/def/vehicle/truck/sample/mirror/large.sii".into();
    vehicle::set_parts(&mut data, VehicleKind::Truck, "truck.test", &parts).unwrap();
    assert_eq!(data.game.value("addon.test", "refund"), Some("999"));
    assert_eq!(
        data.game.value("addon.test", "future_addon_field"),
        Some("\"retain\"")
    );
    let before = data.game.render().unwrap().into_owned();
    parts[1].path = "/def/vehicle/truck/other/mirror/large.sii".into();
    assert!(vehicle::set_parts(&mut data, VehicleKind::Truck, "truck.test", &parts).is_err());
    parts[1].path = choices[0].clone();
    parts[0].path = "/def/vehicle/truck/sample/paint_job/other.sii".into();
    assert!(vehicle::set_parts(&mut data, VehicleKind::Truck, "truck.test", &parts).is_err());
    assert_eq!(data.game.render().unwrap(), before);
}

#[test]
fn shared_accessory_and_malformed_values_are_preserved_on_rejection() {
    let mut data = fixture();
    data.game
        .set_array(
            "truck.test",
            "accessories",
            vec![
                "paint.test".into(),
                "addon.test".into(),
                "paint.test".into(),
            ],
        )
        .unwrap();
    assert!(vehicle::parts(&data, VehicleKind::Truck, "truck.test").is_err());
    data.game
        .set_array(
            "truck.test",
            "accessories",
            vec!["paint.test".into(), "addon.test".into()],
        )
        .unwrap();
    data.game
        .set("paint.test", "base_color", "(0, 0, 0, 1)")
        .unwrap();
    assert!(vehicle::paint(&data, VehicleKind::Truck, "truck.test").is_err());
    data.game
        .set("paint.test", "base_color", "(0, 0, 0)")
        .unwrap();
    // Duplicate a vehicle rather than guessing an accessory type for the owner.
    data.game
        .duplicate_unit("truck.test", "other.truck")
        .unwrap();
    let mut colors = vehicle::paint(&data, VehicleKind::Truck, "truck.test").unwrap();
    colors[0].linear = [1.0; 3];
    let before = data.game.render().unwrap().into_owned();
    assert!(vehicle::set_paint(&mut data, VehicleKind::Truck, "truck.test", &colors).is_err());
    assert_eq!(data.game.render().unwrap(), before);
}
