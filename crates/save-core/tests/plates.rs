use ts_save_core::plates::{Part, Plate, Tag};

#[test]
fn structured_plate_roundtrips_all_formatting_without_exposing_it_as_text() {
    let original = "<color value=FF332211><font xscale=0.8 yscale=1.2 future='retained'><offset hshift=5 vshift=-7><img src=/material/ui/lp/germany/flag.mat width=10 height=15><align left=23>AB<sup>12</sup><sub>3</sub></align><ret>CD</font><future value=42>|germany";
    let plate = Plate::parse(original).unwrap();
    assert_eq!(plate.render().unwrap(), original);
    assert_eq!(plate.text(), "AB123CD");
    assert_eq!(plate.first_color(), [0x11, 0x22, 0x33, 0xff]);
    assert_eq!(plate.country.as_deref(), Some("germany"));
    assert!(
        plate
            .parts
            .iter()
            .any(|part| matches!(part,Part::Tag(tag) if tag.name()=="img"))
    );
}

#[test]
fn visual_text_color_and_scale_changes_preserve_unknown_formatting() {
    let original = "<font  xscale = '0.8' future='keep'><color value=FF000000>AB 123</font><unrecognized a=7>|california";
    let mut plate = Plate::parse(original).unwrap();
    plate.set_text("TRUCK 42").unwrap();
    plate.set_color([200, 100, 50, 255]).unwrap();
    if let Part::Tag(tag) = &mut plate.parts[0] {
        tag.set("xscale", "1.1").unwrap();
    }
    let rendered = plate.render().unwrap();
    assert!(rendered.contains("<font  xscale = '1.1' future='keep'>"));
    assert!(rendered.contains("<unrecognized a=7>"));
    assert!(rendered.ends_with("|california"));
    let reloaded = Plate::parse(&rendered).unwrap();
    assert_eq!(reloaded.text(), "TRUCK 42");
    assert_eq!(reloaded.first_color(), [200, 100, 50, 255]);
}

#[test]
fn simple_plate_colour_and_region_edits_require_no_markup_input() {
    let mut plate = Plate::parse("AB 123|germany").unwrap();
    plate.set_text("NEW 456").unwrap();
    plate.set_color([1, 2, 3, 255]).unwrap();
    plate.country = Some("texas".into());
    assert_eq!(
        plate.render().unwrap(),
        "<color value=FF030201>NEW 456|texas"
    );
    let mut default = Plate::parse("NO REGION").unwrap();
    default.set_text("NEW").unwrap();
    assert_eq!(default.render().unwrap(), "NEW");
}

#[test]
fn malformed_or_injected_plate_edits_are_rejected() {
    for raw in ["<font xscale=1", "AB\n123|germany"] {
        assert!(Plate::parse(raw).is_err());
    }
    let mut plate = Plate::parse("AB<sup>12</sup>|germany").unwrap();
    assert!(plate.set_text("flattened").is_err());
    for text in ["<font>bad", "ABC|country", "AB\rCD"] {
        assert!(plate.set_text(text).is_err());
    }
    let mut tag = Tag::create("font").unwrap();
    for value in ["1 other=2", "1><img", "\"1\"", "NaN\n"] {
        assert!(tag.set("xscale", value).is_err());
    }
    assert!(Tag::create("script").is_err());
}
