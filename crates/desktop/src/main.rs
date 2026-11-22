#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod activity;
mod app;
mod artwork;
mod game_artwork;
mod i18n;
#[cfg(windows)]
mod windows_update;
const APP_NAME: &str = "TruckCraft";
fn display_version() -> &'static str {
    let version = env!("CARGO_PKG_VERSION");
    version.strip_suffix(".0").unwrap_or(version)
}

fn isolated_launch() -> bool {
    std::env::args_os().any(|arg| arg == "--isolated" || arg == "--package-check")
}

fn main() -> eframe::Result {
    #[cfg(windows)]
    if let Some(result) = windows_update::run_helper() {
        return result.map_err(|error| eframe::Error::AppCreation(Box::new(error)));
    }
    let isolated = isolated_launch();
    let _ = activity::EventLog::new(isolated).record(
        "INFO",
        &format!("{APP_NAME} {} starting", display_version()),
    );
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        let _ = activity::EventLog::new(isolated)
            .record("ERROR", &format!("Application panic: {panic}"));
        previous(panic);
    }));
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_app_id("io.github.chrissts.TruckCraft")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([1180.0, 720.0])
            .with_icon(artwork::window_icon()),
        ..Default::default()
    };
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "--package-check")
    {
        if std::env::args_os().count() != 2 {
            return Err(eframe::Error::AppCreation(
                "Run --package-check on its own.".into(),
            ));
        }
        return app::package_check::run(options);
    }
    let result = eframe::run_native(
        &format!("{APP_NAME} {}", display_version()),
        options,
        Box::new(|context| {
            let app = app::EditorApp::new(context);
            #[cfg(windows)]
            let app = app.with_update_confirmation(windows_update::Startup::from_args());
            Ok(Box::new(app))
        }),
    );
    let _ = activity::EventLog::new(isolated).record(
        if result.is_ok() { "INFO" } else { "ERROR" },
        &match &result {
            Ok(()) => "TruckCraft closed".into(),
            Err(error) => format!("Application startup/runtime failed: {error}"),
        },
    );
    result
}
