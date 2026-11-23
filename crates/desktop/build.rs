use std::{
    env, fs,
    path::{Path, PathBuf},
};

const LOCAL_ART_FILES: &[&str] = &[
    "assets/ats-logo.png",
    "assets/ets2-logo.png",
    "assets/game/import-manifest.json",
    "assets/game/ats/import-manifest.json",
];

fn main() {
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    println!("cargo:rustc-check-cfg=cfg(truckcraft_local_game_art)");
    println!("cargo:rerun-if-env-changed=TRUCKCRAFT_BUNDLE_LOCAL_ART");
    for path in LOCAL_ART_FILES {
        println!("cargo:rerun-if-changed={path}");
    }
    if env::var("TRUCKCRAFT_BUNDLE_LOCAL_ART").as_deref() != Ok("0")
        && LOCAL_ART_FILES.iter().all(|path| Path::new(path).is_file())
    {
        println!("cargo:rustc-cfg=truckcraft_local_game_art");
    }
    #[cfg(windows)]
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=assets/truckcraft.rc");
        println!("cargo:rerun-if-changed=assets/truckcraft.ico");
        let version = env::var("CARGO_PKG_VERSION").unwrap();
        let numeric_version = format!(
            "{},{},{},0",
            env::var("CARGO_PKG_VERSION_MAJOR").unwrap(),
            env::var("CARGO_PKG_VERSION_MINOR").unwrap(),
            env::var("CARGO_PKG_VERSION_PATCH").unwrap(),
        );
        fs::write(
            out_dir.join("truckcraft_version.h"),
            format!(
                "#define TRUCKCRAFT_VERSION {numeric_version}\n#define TRUCKCRAFT_VERSION_TEXT \"{version}\"\n"
            ),
        )
        .expect("write executable version definitions");
        embed_resource::compile_for(
            "assets/truckcraft.rc",
            ["truckcraft"],
            embed_resource::ParamsIncludeDirs([&out_dir]),
        )
        .manifest_required()
        .expect("embed the TruckCraft executable resources");
    }

    println!("cargo:rerun-if-changed=locales");
    let mut files: Vec<_> = fs::read_dir("locales")
        .expect("read locale directory")
        .map(|entry| entry.expect("read locale entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    files.sort();

    let mut index = String::from("const SOURCES: &[(&str, &str)] = &[\n");
    for file in files {
        let code = file.file_stem().unwrap().to_str().unwrap();
        let relative = format!("/locales/{code}.json");
        index.push_str(&format!(
            "({code:?}, include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), {relative:?}))),\n"
        ));
    }
    index.push_str("];\n");
    fs::write(out_dir.join("locales.rs"), index).expect("write locale index");
}
