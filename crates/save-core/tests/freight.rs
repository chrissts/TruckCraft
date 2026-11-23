use ts_save_core::{
    document::Document,
    events, freight,
    session::{Game, SaveData},
};

fn fixture() -> SaveData {
    let text = "SiiNunit\n{\neconomy : economy.test {\n game_time: 100\n companies: 2\n companies[0]: company.volatile.a.city\n companies[1]: company.volatile.b.city\n event_queue: queue.test\n}\ncompany : company.volatile.a.city {\n job_offer: 2\n job_offer[0]: offer.a\n job_offer[1]: offer.b\n future_company_field: 42\n}\ncompany : company.volatile.b.city {\n job_offer: 0\n}\njob_offer_data : offer.a {\n target: \"b.city\"\n expiration_time: 200\n urgency: 0\n shortest_distance_km: 50\n ferry_time: 0\n ferry_price: 0\n cargo: cargo.sample\n company_truck: \"sample_truck\"\n trailer_variant: trailer.sample\n trailer_definition: trailer_def.sample\n units_count: 10\n fill_ratio: 1\n trailer_place: 0\n future_array: 2\n future_array[0]: \"first\"\n future_array[1]: \"second\"\n # future comment\n future_link: offer.b\n}\njob_offer_data : offer.b {\n target: \"b.city\"\n expiration_time: 300\n urgency: 1\n shortest_distance_km: 60\n ferry_time: 0\n ferry_price: 0\n cargo: cargo.other\n company_truck: \"other_truck\"\n trailer_variant: trailer.other\n trailer_definition: trailer_def.other\n units_count: 20\n fill_ratio: 1\n trailer_place: 0\n}\neconomy_event_queue : queue.test {\n data: 3\n data[0]: event.other\n data[1]: event.a\n data[2]: event.b\n}\neconomy_event : event.other {\n time: 150\n unit_link: queue.test\n param: 77\n future: 123\n}\neconomy_event : event.a {\n time: 200\n unit_link: company.volatile.a.city\n param: 0\n future: \"retained\"\n}\neconomy_event : event.b {\n time: 300\n unit_link: company.volatile.a.city\n param: 1\n}\n}\n";
    SaveData {
        game: Document::parse(text).unwrap(),
        profile: Document::parse("SiiNunit\n{\n}\n").unwrap(),
        info: Document::parse("SiiNunit\n{\n}\n").unwrap(),
        simulator: Game::Ets2,
    }
}

#[test]
fn create_edit_remove_clear_keep_events_and_unknown_data_consistent() {
    let mut data = fixture();
    let source = "company.volatile.a.city";
    let original = data.game.clone();
    let mut terms = freight::offers(&data, source).unwrap()[0].terms.clone();
    terms.expiration = 400;
    let id = freight::create(&mut data, source, "offer.a", &terms).unwrap();
    assert_eq!(
        data.game.array(source, "job_offer").unwrap(),
        ["offer.a", "offer.b", &id]
    );
    assert_eq!(
        data.game.array(&id, "future_array").unwrap(),
        ["\"first\"", "\"second\""]
    );
    assert_eq!(data.game.value(&id, "future_link"), Some("offer.b"));
    assert!(
        data.game
            .render()
            .unwrap()
            .matches("# future comment")
            .count()
            == 2
    );
    terms.expiration = 175;
    freight::configure(&mut data, source, &id, "offer.b", &terms).unwrap();
    assert_eq!(data.game.value(&id, "cargo"), Some("cargo.other"));
    assert_eq!(data.game.value(&id, "units_count"), Some("20"));
    let events = events::entries(&data.game, "economy.test").unwrap().1;
    assert_eq!(
        events.iter().map(|event| event.time).collect::<Vec<_>>(),
        [150, 175, 200, 300]
    );
    freight::remove(&mut data, source, &["offer.a".into()]).unwrap();
    assert_eq!(
        data.game.array(source, "job_offer").unwrap(),
        ["offer.b", &id]
    );
    let events = events::entries(&data.game, "economy.test").unwrap().1;
    assert!(
        events
            .iter()
            .any(|event| event.unit_link == source && event.param == 0 && event.time == 300)
    );
    assert!(
        events
            .iter()
            .any(|event| event.unit_link == source && event.param == 1 && event.time == 175)
    );
    assert_eq!(data.game.fields("offer.a"), original.fields("offer.a"));
    assert_eq!(data.game.value("event.a", "future"), Some("\"retained\""));
    freight::clear(&mut data, source).unwrap();
    assert!(data.game.array(source, "job_offer").unwrap().is_empty());
    assert_eq!(
        events::entries(&data.game, "economy.test").unwrap().1.len(),
        1
    );
    assert_eq!(
        data.game.fields("event.other"),
        original.fields("event.other")
    );
    Document::parse(data.game.render().unwrap().into_owned()).unwrap();
}

#[test]
fn invalid_or_incomplete_graph_edits_roll_back_every_field() {
    let source = "company.volatile.a.city";
    let mut data = fixture();
    let mut terms = freight::offers(&data, source).unwrap()[0].terms.clone();
    let before = data.game.render().unwrap().into_owned();
    terms.expiration = 100;
    assert!(freight::update(&mut data, source, "offer.a", &terms).is_err());
    assert_eq!(data.game.render().unwrap(), before);
    terms.expiration = 250;
    terms.target = "missing.city".into();
    assert!(freight::create(&mut data, source, "offer.a", &terms).is_err());
    assert_eq!(data.game.render().unwrap(), before);
    terms.target = "b.city".into();
    data.game
        .set_array(
            "queue.test",
            "data",
            vec!["event.other".into(), "event.a".into()],
        )
        .unwrap();
    let before = data.game.render().unwrap().into_owned();
    assert!(freight::update(&mut data, source, "offer.a", &terms).is_err());
    assert!(freight::create(&mut data, source, "offer.a", &terms).is_err());
    assert!(freight::clear(&mut data, source).is_err());
    assert_eq!(data.game.render().unwrap(), before);
}
