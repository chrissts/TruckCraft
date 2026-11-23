use ts_save_core::{
    document::Document,
    editor::{self, VehicleKind},
    session::{Game, SaveData},
};

fn fixture() -> SaveData {
    let game = "SiiNunit\n{\neconomy : economy.test {\n player: player.test\n}\nplayer : player.test {\n trailers: 1\n trailers[0]: trailer.a\n}\ntrailer : trailer.a {\n slave_trailer: trailer.b\n trailer_body_wear: 0.5\n trailer_body_wear_unfixable: 0.2\n cargo_damage: 0.3\n integrity_odometer: 5000\n integrity_odometer_float_part: 0.25\n odometer: 123456\n wheels_wear: 2\n wheels_wear[0]: 0.1\n wheels_wear[1]: 0.2\n wheels_wear_unfixable: 2\n wheels_wear_unfixable[0]: 0.3\n wheels_wear_unfixable[1]: 0.4\n future_field: \"keep\"\n}\ntrailer : trailer.b {\n slave_trailer: null\n trailer_body_wear: 0.4\n trailer_body_wear_unfixable: 0.1\n cargo_damage: 0.2\n integrity_odometer: 6000\n integrity_odometer_float_part: 0.75\n odometer: 456789\n wheels_wear: 1\n wheels_wear[0]: 0.6\n}\n}\n";
    SaveData {
        profile: Document::parse("SiiNunit\n{\n}\n").unwrap(),
        info: Document::parse("SiiNunit\n{\n}\n").unwrap(),
        game: Document::parse(game).unwrap(),
        simulator: Game::Ats,
    }
}

#[test]
fn trailer_repair_covers_body_cargo_and_chain_without_erasing_real_mileage() {
    let mut data = fixture();
    editor::repair(&mut data, VehicleKind::Trailer, "trailer.a", false).unwrap();
    for (id, count, mileage) in [("trailer.a", 2, "123456"), ("trailer.b", 1, "456789")] {
        assert_eq!(data.game.value(id, "trailer_body_wear"), Some("0"));
        assert_eq!(data.game.value(id, "cargo_damage"), Some("0"));
        assert_eq!(
            data.game.array(id, "wheels_wear").unwrap(),
            vec!["0"; count]
        );
        assert_eq!(data.game.value(id, "odometer"), Some(mileage));
    }
    assert_eq!(
        data.game.value("trailer.a", "integrity_odometer"),
        Some("5000")
    );
    assert_eq!(
        data.game.value("trailer.a", "trailer_body_wear_unfixable"),
        Some("0.2")
    );
    editor::repair(&mut data, VehicleKind::Trailer, "trailer.a", true).unwrap();
    for id in ["trailer.a", "trailer.b"] {
        for field in [
            "trailer_body_wear_unfixable",
            "integrity_odometer",
            "integrity_odometer_float_part",
        ] {
            assert_eq!(data.game.value(id, field), Some("0"));
        }
    }
    assert_eq!(
        data.game.value("trailer.a", "future_field"),
        Some("\"keep\"")
    );
    assert_eq!(
        data.game.value("trailer.a", "slave_trailer"),
        Some("trailer.b")
    );
    assert_eq!(
        data.game
            .array("trailer.a", "wheels_wear_unfixable")
            .unwrap(),
        ["0", "0"]
    );
}

#[test]
fn cyclic_or_missing_trailer_chains_reject_the_entire_repair() {
    for target in ["trailer.a", "missing.trailer"] {
        let mut data = fixture();
        data.game.set("trailer.b", "slave_trailer", target).unwrap();
        let before = data.game.render().unwrap().into_owned();
        assert!(editor::repair(&mut data, VehicleKind::Trailer, "trailer.a", true).is_err());
        assert_eq!(data.game.render().unwrap(), before);
    }
}
