use ts_save_core::{
    document::Document,
    profile::{self, Identity},
    session::{Game, SaveData},
};

#[test]
fn identity_changes_are_targeted_and_invalid_choices_do_not_partially_apply() {
    let mut data = SaveData {
        simulator:Game::Ets2,
        game:Document::parse("SiiNunit\n{\n}\n").unwrap(),
        info:Document::parse("SiiNunit\n{\n}\n").unwrap(),
        profile:Document::parse("SiiNunit\n{\nuser_profile : profile.test {\n face: 999\n logo: 999\n male: true\n company_name: \"Sample\"\n future_identity: 42\n}\n}\n").unwrap(),
    };
    let original = profile::identity(&data).unwrap();
    profile::set_identity(&mut data, &original, &[1, 2], &[0, 1]).unwrap();
    assert!(
        !data.profile.is_dirty(),
        "custom saved choices must be preserved"
    );
    let edit = Identity {
        face: 1,
        logo: "logo_0".into(),
        male: false,
    };
    profile::set_identity(&mut data, &edit, &[1, 2], &[0, 1]).unwrap();
    assert_eq!(profile::identity(&data).unwrap(), edit);
    assert_eq!(
        data.profile.value("profile.test", "future_identity"),
        Some("42")
    );
    assert_eq!(
        data.profile.string("profile.test", "company_name").unwrap(),
        "Sample"
    );
    let before = data.profile.render().unwrap().into_owned();
    assert!(
        profile::set_identity(
            &mut data,
            &Identity {
                face: 2,
                logo: "logo_99".into(),
                male: true
            },
            &[1, 2],
            &[0, 1]
        )
        .is_err()
    );
    assert_eq!(data.profile.render().unwrap(), before);
}
