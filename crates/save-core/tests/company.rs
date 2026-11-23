use std::collections::BTreeMap;
use ts_save_core::{
    company::{self, DriverStatus, MoveKind},
    document::{Document, UnitSpec},
    events,
    session::{Game, SaveData},
    value,
};

fn unit(kind: &str, id: &str, scalars: &[(&str, &str)], arrays: &[(&str, &[&str])]) -> UnitSpec {
    UnitSpec {
        kind: kind.into(),
        id: id.into(),
        scalars: scalars
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect(),
        arrays: arrays
            .iter()
            .map(|(key, values)| {
                (
                    key.to_string(),
                    values.iter().map(|value| value.to_string()).collect(),
                )
            })
            .collect(),
    }
}
fn fixture() -> SaveData {
    let mut specs = vec![
        unit(
            "economy",
            "economy.test",
            &[
                ("player", "player.test"),
                ("bank", "bank.test"),
                ("event_queue", "queue.test"),
                ("game_time", "100"),
            ],
            &[
                ("driver_pool", &["driver.2"]),
                ("visited_cities", &["\"a\""]),
                ("visited_cities_count", &["9"]),
            ],
        ),
        unit(
            "player",
            "player.test",
            &[("hq_city", "\"a\"")],
            &[
                ("drivers", &["driver.0", "driver.1"]),
                ("driver_readiness_timer", &["0", "42"]),
                ("driver_quit_warned", &["false", "true"]),
                ("dismissed_drivers", &[]),
            ],
        ),
        unit("bank", "bank.test", &[("money_account", "1000")], &[]),
        unit("driver_player", "driver.0", &[], &[]),
        unit(
            "garage",
            "garage.a",
            &[("status", "2"), ("future_data", "\"retained exactly\"")],
            &[
                ("drivers", &["driver.0", "driver.1", "null"]),
                ("vehicles", &["truck.1", "truck.2", "null"]),
                ("trailers", &["trailer.1"]),
            ],
        ),
        unit(
            "garage",
            "garage.b",
            &[("status", "3")],
            &[
                ("drivers", &["null"; 5]),
                ("vehicles", &["null"; 5]),
                ("trailers", &[]),
            ],
        ),
        unit(
            "garage",
            "garage.c",
            &[("status", "0")],
            &[("drivers", &[]), ("vehicles", &[]), ("trailers", &[])],
        ),
        unit("vehicle", "truck.1", &[("fuel_relative", "1")], &[]),
        unit("vehicle", "truck.2", &[("fuel_relative", "0.5")], &[]),
        unit("trailer", "trailer.1", &[], &[]),
        unit(
            "economy_event_queue",
            "queue.test",
            &[],
            &[("data", &["event.stable", "event.pool"])],
        ),
        unit(
            "economy_event",
            "event.stable",
            &[
                ("time", "250"),
                ("unit_link", "company.volatile.example.a"),
                ("param", "0"),
                ("future_event_data", "\"preserve me\""),
            ],
            &[],
        ),
        unit(
            "economy_event",
            "event.pool",
            &[("time", "500"), ("unit_link", "driver.2"), ("param", "3")],
            &[],
        ),
        unit("company", "company.volatile.example.a", &[], &[]),
    ];
    for id in ["driver.1", "driver.2"] {
        let job = if id == "driver.1" { "job.1" } else { "job.2" };
        let mut spec = unit(
            "driver_ai",
            id,
            &[
                ("hometown", if id == "driver.1" { "\"a\"" } else { "\"\"" }),
                ("current_city", "\"a\""),
                ("old_hometown", "\"\""),
                ("experience_points", "1234"),
                ("training_policy", "1"),
                ("adr", "1"),
                ("state", "1"),
                ("on_duty_timer", "0"),
                ("extra_maintenance", "0"),
                ("driver_job", job),
                ("assigned_truck", "null"),
                ("adopted_truck", "null"),
                ("assigned_trailer", "null"),
                ("adopted_trailer", "null"),
                ("future_driver_data", "(1, 2, 3)"),
            ],
            &[],
        );
        for key in ts_save_core::editor::TRUCK_SKILLS {
            spec.scalars.insert(key.into(), "1".into());
        }
        specs.push(spec);
        specs.push(unit("job_info", job, &[("cargo", "null")], &[]));
    }
    let mut doc = Document::parse("SiiNunit\n{\n}\n").unwrap();
    doc.insert_units(&specs).unwrap();
    let game = Document::parse(doc.render().unwrap().into_owned()).unwrap();
    SaveData {
        game,
        profile: Document::parse(
            "SiiNunit\n{\nuser_profile : profile.test {\n profile_name: \"Synthetic\"\n}\n}\n",
        )
        .unwrap(),
        info: Document::parse("SiiNunit\n{\nsave_container : info.test {\n version: 105\n}\n}\n")
            .unwrap(),
        simulator: Game::Ets2,
    }
}

fn occupants(data: &SaveData) -> BTreeMap<String, String> {
    company::garages(data)
        .unwrap()
        .into_iter()
        .flat_map(|garage| {
            garage
                .drivers
                .into_iter()
                .chain(garage.vehicles)
                .chain(garage.trailers)
                .filter(|id| !value::is_null(id))
                .map(move |id| (id, garage.city.clone()))
        })
        .collect()
}
#[test]
fn shrinking_and_selling_relocate_every_occupant_and_keep_player_at_hq() {
    let mut data = fixture();
    let original = occupants(&data);
    company::resize_garages(&mut data, &["garage.c".into()], 2).unwrap();
    company::resize_garages(&mut data, &["garage.a".into()], 6).unwrap();
    assert_eq!(
        occupants(&data).keys().collect::<Vec<_>>(),
        original.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        data.game.array("garage.a", "drivers").unwrap(),
        ["driver.0"]
    );
    assert_eq!(data.game.string("driver.1", "hometown").unwrap(), "b");
    company::move_trailers(&mut data, "garage.a", "garage.b").unwrap();
    company::resize_garages(&mut data, &["garage.b".into()], 0).unwrap();
    assert_eq!(
        occupants(&data).keys().collect::<Vec<_>>(),
        original.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        data.game.array("garage.c", "drivers").unwrap()[0],
        "driver.1"
    );
    assert_eq!(
        data.game.array("garage.a", "trailers").unwrap(),
        ["trailer.1"]
    );
    assert_eq!(
        data.game.value("garage.a", "future_data"),
        Some("\"retained exactly\"")
    );
    let reparsed = Document::parse(data.game.render().unwrap().into_owned()).unwrap();
    assert_eq!(reparsed.array("garage.b", "vehicles").unwrap().len(), 0);
}
#[test]
fn rejected_garage_edits_do_not_partially_change_the_save() {
    let mut data = fixture();
    company::resize_garages(&mut data, &["garage.b".into()], 0).unwrap();
    let before = data.game.render().unwrap().into_owned();
    assert!(company::resize_garages(&mut data, &["garage.a".into()], 6).is_err());
    assert_eq!(data.game.render().unwrap(), before);
    assert!(company::resize_garages(&mut data, &["garage.a".into()], 0).is_err());
    assert_eq!(data.game.render().unwrap(), before);
    assert!(company::move_slot(&mut data, "garage.a", 0, "garage.b", 0, MoveKind::Pair).is_err());
    assert_eq!(data.game.render().unwrap(), before);
}
#[test]
fn hiring_and_idle_dismissal_keep_staff_arrays_and_events_consistent() {
    let mut data = fixture();
    company::hire(&mut data, "driver.2", "garage.b").unwrap();
    assert_eq!(
        data.game.array("player.test", "drivers").unwrap(),
        ["driver.0", "driver.1", "driver.2"]
    );
    assert_eq!(
        data.game
            .array("player.test", "driver_readiness_timer")
            .unwrap(),
        ["0", "42", "0"]
    );
    assert!(
        data.game
            .array("economy.test", "driver_pool")
            .unwrap()
            .is_empty()
    );
    let (_, queue) = events::entries(&data.game, "economy.test").unwrap();
    assert_eq!(queue.len(), 2);
    assert_eq!(queue[0].time, 100);
    assert_eq!(queue[0].unit_link, "driver.2");
    assert_eq!(
        data.game.value("event.stable", "future_event_data"),
        Some("\"preserve me\"")
    );
    company::set_driver(&mut data, "driver.2", 40000, 63, [6; 5], Some(7)).unwrap();
    company::fire(&mut data, "driver.2").unwrap();
    assert_eq!(
        data.game.array("player.test", "drivers").unwrap(),
        ["driver.0", "driver.1"]
    );
    assert_eq!(
        data.game
            .array("player.test", "driver_quit_warned")
            .unwrap(),
        ["false", "true"]
    );
    assert_eq!(
        data.game.array("economy.test", "driver_pool").unwrap(),
        ["driver.2"]
    );
    assert_eq!(
        events::entries(&data.game, "economy.test").unwrap().1.len(),
        1
    );
    assert_eq!(
        data.game.value("driver.2", "future_driver_data"),
        Some("(1, 2, 3)")
    );
    assert_eq!(
        company::drivers(&data)
            .unwrap()
            .iter()
            .find(|driver| driver.id == "driver.2")
            .unwrap()
            .status,
        DriverStatus::Available
    );
}
#[test]
fn active_driver_dismissal_and_rehire_preserve_the_job_and_do_not_duplicate_staff() {
    let mut data = fixture();
    data.game.set("job.1", "cargo", "cargo.example").unwrap();
    events::rewrite(
        &mut data.game,
        "economy.test",
        |_| false,
        &[("driver.1".into(), 3, 140)],
    )
    .unwrap();
    let count = events::entries(&data.game, "economy.test").unwrap().1.len();
    company::fire(&mut data, "driver.1").unwrap();
    assert_eq!(
        data.game.array("player.test", "dismissed_drivers").unwrap(),
        ["driver.1"]
    );
    assert_eq!(data.game.array("player.test", "drivers").unwrap().len(), 2);
    assert_eq!(
        events::entries(&data.game, "economy.test").unwrap().1.len(),
        count
    );
    company::hire(&mut data, "driver.1", "garage.b").unwrap();
    assert!(
        data.game
            .array("player.test", "dismissed_drivers")
            .unwrap()
            .is_empty()
    );
    assert_eq!(data.game.array("player.test", "drivers").unwrap().len(), 2);
    assert_eq!(
        data.game.required("job.1", "cargo").unwrap(),
        "cargo.example"
    );
    assert_eq!(
        events::entries(&data.game, "economy.test").unwrap().1.len(),
        count
    );
}
#[test]
fn moving_slots_and_cities_retains_counts_and_unrelated_records() {
    let mut data = fixture();
    company::move_slot(&mut data, "garage.a", 1, "garage.b", 4, MoveKind::Pair).unwrap();
    assert_eq!(
        data.game.array("garage.b", "drivers").unwrap()[4],
        "driver.1"
    );
    assert_eq!(
        data.game.array("garage.b", "vehicles").unwrap()[4],
        "truck.2"
    );
    assert_eq!(data.game.string("driver.1", "hometown").unwrap(), "b");
    company::set_visited(&mut data, &["b".into(), "c".into()], true).unwrap();
    assert_eq!(
        data.game
            .array("economy.test", "visited_cities_count")
            .unwrap(),
        ["9", "1", "1"]
    );
    company::set_visited(&mut data, &["b".into()], false).unwrap();
    assert_eq!(
        data.game.array("economy.test", "visited_cities").unwrap(),
        ["\"a\"", "\"c\""]
    );
    assert_eq!(
        data.game
            .array("economy.test", "visited_cities_count")
            .unwrap(),
        ["9", "1"]
    );
}

#[test]
fn current_flags_and_timers_stay_aligned_and_deprecated_warning_array_stays_empty() {
    let mut data = fixture();
    let text=data.game.render().unwrap().lines().filter(|line|!line.trim().starts_with("driver_quit_warned[")).map(|line|line.replace("driver_quit_warned: 2","driver_quit_warned: 0")).collect::<Vec<_>>().join("\n").replace("player : player.test {","player : player.test {\n driver_flags: 2\n driver_flags[0]: 0\n driver_flags[1]: 17\n driver_undrivable_truck_timers: 2\n driver_undrivable_truck_timers[0]: 0\n driver_undrivable_truck_timers[1]: 211\n");
    data.game = Document::parse(text).unwrap();
    data.game
        .set("event.stable", "param", "4294967295")
        .unwrap();
    company::hire(&mut data, "driver.2", "garage.b").unwrap();
    assert_eq!(
        data.game.array("player.test", "driver_flags").unwrap(),
        ["0", "17", "0"]
    );
    assert_eq!(
        data.game
            .array("player.test", "driver_undrivable_truck_timers")
            .unwrap(),
        ["0", "211", "0"]
    );
    assert!(
        data.game
            .array("player.test", "driver_quit_warned")
            .unwrap()
            .is_empty()
    );
    company::fire(&mut data, "driver.2").unwrap();
    assert_eq!(
        data.game.array("player.test", "driver_flags").unwrap(),
        ["0", "17"]
    );
    assert_eq!(
        data.game
            .array("player.test", "driver_undrivable_truck_timers")
            .unwrap(),
        ["0", "211"]
    );
    assert!(
        data.game
            .array("player.test", "driver_quit_warned")
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        data.game.required("event.stable", "param").unwrap(),
        "4294967295"
    );
}

#[test]
fn hq_swaps_update_displaced_ai_home_and_city_edits_retain_unrelated_zero_counts() {
    let mut data = fixture();
    company::move_slot(&mut data, "garage.a", 1, "garage.b", 0, MoveKind::Pair).unwrap();
    data.game
        .insert_units(&[
            unit("vehicle", "truck.3", &[], &[]),
            unit("vehicle", "truck.4", &[], &[]),
        ])
        .unwrap();
    data.game.set("garage.b", "status", "2").unwrap();
    data.game
        .set_array(
            "garage.b",
            "drivers",
            vec!["driver.1".into(), "null".into(), "null".into()],
        )
        .unwrap();
    data.game
        .set_array(
            "garage.b",
            "vehicles",
            vec!["truck.2".into(), "truck.3".into(), "truck.4".into()],
        )
        .unwrap();
    ts_save_core::editor::move_headquarters(&mut data, "b").unwrap();
    assert_eq!(data.game.string("driver.1", "hometown").unwrap(), "a");
    assert_eq!(
        data.game.array("garage.b", "drivers").unwrap()[0],
        "driver.0"
    );
    data.game
        .set_array(
            "economy.test",
            "visited_cities",
            vec!["\"c\"".into(), "\"a\"".into()],
        )
        .unwrap();
    data.game
        .set_array(
            "economy.test",
            "visited_cities_count",
            vec!["0".into(), "9".into()],
        )
        .unwrap();
    company::set_visited(&mut data, &["b".into()], true).unwrap();
    assert_eq!(
        data.game.array("economy.test", "visited_cities").unwrap(),
        ["\"c\"", "\"a\"", "\"b\""]
    );
    assert_eq!(
        data.game
            .array("economy.test", "visited_cities_count")
            .unwrap(),
        ["0", "9", "1"]
    );
}

#[test]
fn maximise_hired_preserves_pool_jobs_training_and_higher_experience() {
    let event_data = |doc: &Document| {
        events::entries(doc, "economy.test")
            .unwrap()
            .1
            .into_iter()
            .map(|event| (event.id, event.time, event.unit_link, event.param))
            .collect::<Vec<_>>()
    };
    for simulator in [Game::Ets2, Game::Ats] {
        let mut data = fixture();
        data.simulator = simulator;
        let pool_xp = data
            .game
            .value("driver.2", "experience_points")
            .unwrap()
            .to_owned();
        let events_before = event_data(&data.game);
        assert_eq!(company::maximise_hired_drivers(&mut data).unwrap(), 1);
        let driver = company::drivers(&data)
            .unwrap()
            .into_iter()
            .find(|driver| driver.id == "driver.1")
            .unwrap();
        assert_eq!(
            driver.xp,
            ts_save_core::profile::xp_for_level(simulator, 150).unwrap()
        );
        assert_eq!(driver.adr, 63);
        assert_eq!(driver.skills, [6; 5]);
        assert_eq!(driver.training, 1);
        assert_eq!(data.game.value("driver.1", "driver_job"), Some("job.1"));
        assert_eq!(
            data.game.value("driver.1", "future_driver_data"),
            Some("(1, 2, 3)")
        );
        assert_eq!(
            data.game.value("driver.2", "experience_points"),
            Some(pool_xp.as_str())
        );
        assert_eq!(event_data(&data.game), events_before);
        assert!(data.game.value("driver.0", "experience_points").is_none());
        data.game
            .set("driver.1", "experience_points", i32::MAX.to_string())
            .unwrap();
        company::maximise_hired_drivers(&mut data).unwrap();
        assert_eq!(
            data.game
                .integer::<u32>("driver.1", "experience_points")
                .unwrap(),
            i32::MAX as u32
        );
        company::fire(&mut data, "driver.1").unwrap();
        let before = data.game.render().unwrap().into_owned();
        assert_eq!(company::maximise_hired_drivers(&mut data).unwrap(), 0);
        assert_eq!(data.game.render().unwrap(), before);
    }
}
