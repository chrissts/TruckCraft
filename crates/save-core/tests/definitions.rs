use std::collections::BTreeMap;
use ts_save_core::{
    Error,
    definitions::{self, Catalog},
    game_data::Progression,
    session::Game,
    value,
};

#[test]
fn native_linux_game_folders_require_the_selected_games_executable() {
    for (game, executable, other_game) in [
        (Game::Ets2, "eurotrucks2", Game::Ats),
        (Game::Ats, "amtrucks", Game::Ets2),
    ] {
        let folder = tempfile::tempdir().unwrap();
        let bin = folder.path().join("bin/linux_x64");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join(executable), []).unwrap();

        assert!(matches!(
            Progression::load(game, folder.path()),
            Err(Error::InvalidDefinition(message))
                if message.starts_with("could not open game definition archive:")
        ));
        assert!(matches!(
            Progression::load(other_game, folder.path()),
            Err(Error::InvalidDefinition(message))
                if message.starts_with("Choose the selected game's installation folder")
        ));
    }
}

fn load(files: &[(&str, &str)], roots: &[&str]) -> ts_save_core::Result<Catalog> {
    let files: BTreeMap<_, _> = files.iter().copied().collect();
    Catalog::load(roots, &mut |path, limit| {
        let content = files
            .get(path)
            .ok_or_else(|| Error::Missing("test definition".into()))?;
        assert!(content.len() <= limit);
        Ok(content.as_bytes().to_vec())
    })
}

#[test]
fn includes_multiline_strings_arrays_and_overrides_follow_definition_order() {
    let files = [
        (
            "def/root.sii",
            "\u{feff}SiiNunit\n{\n@include \"parts/units.sui\"\n}\n",
        ),
        (
            "def/parts/units.sui",
            "skill_data : .skills.test\n{\nmax_rank: 6 # comment\n@include \"../defaults.sui\"\nrank_icons[]: \"rank_2\"\nrank_icons[0]: \"overridden\"\nname: \"https://example/#quoted\" // real comment\ndescription: \"First line\n# still string // with { braces } and \\\"quotes\\\"\nLast line\"\n}\n",
        ),
        (
            "def/defaults.sui",
            "max_rank: 3\nrank_icons[]: \"rank_1\"\n",
        ),
    ];
    let catalog = load(&files, &["def/root.sii"]).unwrap();
    assert_eq!(catalog.files.len(), 3);
    let unit = catalog.unit(".skills.test").unwrap();
    assert_eq!(unit.kind, "skill_data");
    assert_eq!(unit.integer::<u32>("max_rank").unwrap(), 3);
    assert_eq!(
        unit.array("rank_icons").unwrap(),
        ["\"overridden\"", "\"rank_2\""]
    );
    assert_eq!(
        value::unquote(unit.raw("name").unwrap()).unwrap(),
        "https://example/#quoted"
    );
    assert!(
        value::unquote(unit.raw("description").unwrap())
            .unwrap()
            .contains("# still string // with { braces }")
    );
}

#[test]
fn fragment_array_reset_and_block_comments_do_not_join_records() {
    let catalog = load(&[("x.sii", "SiiNunit {\nx : test {\na[]: first\na: 0\na[]: second\nnumber: 12 /* multi\nline comment */\nother: 34\n}\n}\n")], &["x.sii"]).unwrap();
    let unit = catalog.unit("test").unwrap();
    assert_eq!(unit.array("a").unwrap(), ["second"]);
    assert_eq!(unit.integer::<u32>("number").unwrap(), 12);
    assert_eq!(unit.integer::<u32>("other").unwrap(), 34);
}

#[test]
fn malformed_includes_and_unbounded_inputs_are_rejected_atomically() {
    for text in [
        "@include \"x.sii\"",
        "@include \"../outside.sui\"",
        "@include \"C:/data.sui\"",
        "@include \"//server/share.sui\"",
        "@include \"https://example/file\"",
        "x : x {\na[999999999]: 1\n}\n",
        "x : x {\na: \"unterminated\n}\n",
        "x : x {\na: 1 /* unterminated\n}\n",
        "x : x {\na: 1\n",
        "}\n",
    ] {
        assert!(
            load(&[("x.sii", text)], &["x.sii"]).is_err(),
            "accepted {text}"
        );
    }
    let mut reads = 0;
    let result = Catalog::load(&["x.sii"], &mut |_, limit| {
        reads += 1;
        Ok(vec![b' '; limit + 1])
    });
    assert!(result.is_err());
    assert_eq!(reads, 1);
    assert!(
        load(
            &[
                ("x.sii", "@include \"fragment.sui\""),
                ("fragment.sui", "@include \"x.sii\"")
            ],
            &["x.sii"]
        )
        .is_err()
    );
}

#[test]
fn virtual_include_paths_stay_in_the_archive_namespace() {
    assert_eq!(
        definitions::virtual_path("def/a/main.sii", "../shared.sui").unwrap(),
        "def/shared.sui"
    );
    assert_eq!(
        definitions::virtual_path("def/a/main.sii", "/def/shared.sui").unwrap(),
        "def/shared.sui"
    );
    for path in [
        "../../../../x",
        "//server/share",
        "file:///x",
        "C:\\x",
        "x\n.sui",
    ] {
        assert!(definitions::virtual_path("def/main.sii", path).is_err());
    }
}

#[test]
fn definition_xp_curves_validate_steps_and_guard_presets() {
    use ts_save_core::profile::XpCurve;
    let curve = XpCurve::from_steps(vec![200, 500, 700]).unwrap();
    assert_eq!(curve.level_for_xp(0), 0);
    assert_eq!(curve.level_for_xp(199), 0);
    assert_eq!(curve.level_for_xp(200), 1);
    assert_eq!(curve.xp_for_level(1).unwrap(), 699);
    assert_eq!(curve.xp_for_level(3).unwrap(), 2099);
    assert_eq!(curve.level_for_xp(2100), 4);
    assert!(curve.xp_for_level(151).is_err());
    for steps in [vec![], vec![0], vec![1, 0], vec![u32::MAX, 1]] {
        assert!(XpCurve::from_steps(steps).is_err());
    }
    assert!(
        XpCurve::from_steps(vec![i32::MAX as u32])
            .unwrap()
            .xp_for_level(150)
            .is_err()
    );
}

#[test]
fn include_depth_and_record_lengths_are_limited_before_expansion() {
    let mut reads = 0;
    let result = Catalog::load(&["0.sui"], &mut |path, _| {
        reads += 1;
        let index: usize = path.trim_end_matches(".sui").parse().unwrap();
        Ok(format!("@include \"{}.sui\"", index + 1).into_bytes())
    });
    assert!(result.is_err());
    assert_eq!(reads, 32);
    let text = format!("x : x {{\na: \"{}\"\n}}\n", "a".repeat(128 * 1024));
    assert!(load(&[("x.sii", &text)], &["x.sii"]).is_err());
}
