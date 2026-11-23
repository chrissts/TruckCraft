use super::*;
use semver::Version;
use std::io::Read;
use std::process::{Command, Stdio};

#[cfg(windows)]
use crate::windows_update::{Manifest, Prepared, Startup};

const API: &str = "https://api.github.com/repos/chrissts/TruckCraft/releases/latest";
const LIMIT: usize = 1024 * 1024;

fn show_manual_update(ui: &mut egui::Ui) {
    if cfg!(target_os = "macos") {
        ui.label(tr("In-app updates are unavailable on macOS. macOS test builds are available in GitHub Actions."));
        ui.hyperlink_to(
            tr("Open macOS test builds"),
            format!("{GITHUB}/actions/workflows/package-check.yml"),
        );
    } else if cfg!(target_os = "linux") {
        ui.label(tr("Update TruckCraft through your package manager or Flatpak if you installed it from a TruckCraft repository. You can also download the matching package from GitHub."));
        ui.hyperlink_to(
            tr("Set up Linux updates"),
            format!("{GITHUB}#linux-updates"),
        );
    } else {
        ui.label(tr(
            "In-app installation is unavailable for this update. Download it from GitHub Releases.",
        ));
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CheckOrigin {
    Startup,
    Manual,
}

pub(super) enum ReleaseCheck {
    Latest {
        tag: String,
        newer: bool,
        #[cfg(windows)]
        manifest: Option<Manifest>,
    },
    Unpublished,
    Failed,
}

impl ReleaseCheck {
    pub(super) fn visible(self, origin: CheckOrigin) -> Option<Self> {
        (origin == CheckOrigin::Manual || matches!(self, Self::Latest { newer: true, .. }))
            .then_some(self)
    }
}

#[cfg(windows)]
pub(super) enum UpdateState {
    Idle,
    Downloading,
    Ready(Prepared),
    Failed(String),
}

fn parse_release(body: &[u8], status: &[u8]) -> ReleaseCheck {
    if status == b"404" {
        return ReleaseCheck::Unpublished;
    }
    if status != b"200" {
        return ReleaseCheck::Failed;
    }
    #[derive(serde::Deserialize)]
    struct Release {
        tag_name: String,
        #[cfg(windows)]
        body: Option<String>,
    }
    let Ok(release) = serde_json::from_slice::<Release>(body) else {
        return ReleaseCheck::Failed;
    };
    let tag_name = release.tag_name;
    let raw = tag_name.strip_prefix('v').unwrap_or(&tag_name);
    let normalized = if raw.split('.').count() == 2 {
        format!("{raw}.0")
    } else {
        raw.to_owned()
    };
    let Ok(latest) = Version::parse(&normalized) else {
        return ReleaseCheck::Failed;
    };
    let installed = Version::parse(env!("CARGO_PKG_VERSION")).expect("Cargo version");
    #[cfg(windows)]
    let manifest = release
        .body
        .and_then(|body| Manifest::from_release(&body, &tag_name));
    ReleaseCheck::Latest {
        tag: tag_name,
        newer: latest.cmp_precedence(&installed).is_gt(),
        #[cfg(windows)]
        manifest,
    }
}

fn fetch() -> Option<ReleaseCheck> {
    #[cfg(windows)]
    let mut command = {
        use std::os::windows::process::CommandExt;
        let mut command =
            Command::new(PathBuf::from(std::env::var_os("SystemRoot")?).join("System32/curl.exe"));
        command.creation_flags(0x08000000);
        command
    };
    #[cfg(not(windows))]
    let mut command = Command::new("/usr/bin/curl");
    let mut child = command
        .args([
            "--disable",
            "--silent",
            "--max-time",
            "12",
            "--proto",
            "=https",
            "--user-agent",
            concat!("TruckCraft/", env!("CARGO_PKG_VERSION")),
            "--header",
            "Accept: application/vnd.github+json",
            "--write-out",
            "\\n%{http_code}",
            API,
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .ok()?;
    let mut bytes = Vec::new();
    let read = child
        .stdout
        .take()?
        .take((LIMIT + 1) as u64)
        .read_to_end(&mut bytes);
    if read.is_err() || bytes.len() > LIMIT {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }
    if !child.wait().ok()?.success() {
        return None;
    }
    let split = bytes.iter().rposition(|b| *b == b'\n')?;
    Some(parse_release(&bytes[..split], &bytes[split + 1..]))
}

impl EditorApp {
    #[cfg(windows)]
    pub(crate) fn with_update_confirmation(mut self, startup: Option<Startup>) -> Self {
        self.update_confirmation = startup;
        self
    }

    pub(super) fn check_release(&mut self, ctx: &egui::Context) {
        self.start_release_check(ctx, CheckOrigin::Manual);
    }

    pub(crate) fn check_release_on_launch(&mut self, ctx: &egui::Context) {
        if !self.isolated && self.browser.check_updates_on_launch() == Some(true) {
            self.start_release_check(ctx, CheckOrigin::Startup);
        }
    }

    pub(super) fn update_consent_dialog(&mut self, ctx: &egui::Context) {
        let Some(checked) = &mut self.update_consent else {
            return;
        };
        let accepted = egui::Modal::new(egui::Id::unique("update_check_consent"))
            .show(ctx, |ui| {
                ui.set_width(420.0);
                ui.heading(tr("Automatic update checks"));
                ui.label(tr("TruckCraft can contact GitHub over the internet at launch to check for a newer version."));
                ui.checkbox(checked, tr("Check for updates at launch"));
                ui.label(tr("You can change this choice in Settings."));
                design::primary(ui, tr("Continue"), true).clicked()
            })
            .inner;
        if accepted {
            let enabled = *checked;
            self.update_consent = None;
            match self.browser.set_check_updates_on_launch(enabled, true) {
                Ok(()) => self.check_release_on_launch(ctx),
                Err(error) => self.error = Some(error.to_string()),
            }
        }
    }

    fn start_release_check(&mut self, ctx: &egui::Context, origin: CheckOrigin) {
        if let Some(pending_origin) = &mut self.checking_release {
            if origin == CheckOrigin::Manual {
                *pending_origin = origin;
            }
            return;
        }
        #[cfg(windows)]
        if matches!(
            self.update_state,
            UpdateState::Downloading | UpdateState::Ready(_)
        ) {
            return;
        } else {
            self.update_state = UpdateState::Idle;
        }
        self.release_result = None;
        self.checking_release = Some(origin);
        let sender = self.tx.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let _ = sender.send(Task::ReleaseChecked(
                fetch().unwrap_or(ReleaseCheck::Failed),
            ));
            ctx.request_repaint();
        });
    }

    #[cfg(windows)]
    fn download_update(&mut self, manifest: Manifest, ctx: &egui::Context) {
        self.update_state = UpdateState::Downloading;
        let sender = self.tx.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let _ = sender.send(Task::UpdateDownloaded(Prepared::download(&manifest)));
            ctx.request_repaint();
        });
    }

    #[cfg(windows)]
    fn install_update(&mut self, ctx: &egui::Context) {
        if self.unsaved() || self.busy || self.saving || self.profile_tools.running {
            return;
        }
        let UpdateState::Ready(update) =
            std::mem::replace(&mut self.update_state, UpdateState::Idle)
        else {
            return;
        };
        match update.start() {
            Ok(()) => {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Err(error) => self.update_state = UpdateState::Failed(error.to_string()),
        }
    }

    pub(super) fn release_dialog(&mut self, ctx: &egui::Context) {
        let Some(result) = &self.release_result else {
            return;
        };
        let mut open = true;
        #[cfg(windows)]
        let (mut download, mut install) = (None, false);
        let window = egui::Window::new(tr("Check for updates"))
            .id(egui::Id::unique("release_result"))
            .collapsible(false);
        #[cfg(windows)]
        let window = if matches!(self.update_state, UpdateState::Downloading) {
            window
        } else {
            window.open(&mut open)
        };
        #[cfg(not(windows))]
        let window = window.open(&mut open);
        window.show(ctx, |ui| {
                ui.label(format!("{} {}", tr("Installed version"), display_version()));
                match result {
                    ReleaseCheck::Latest { tag, newer, .. } => {
                        ui.label(format!("{} {tag}", tr("Latest release")));
                        ui.label(tr(if *newer {
                            "An update is available."
                        } else {
                            "You are up to date."
                        }));
                        #[cfg(windows)]
                        let manual = matches!(result, ReleaseCheck::Latest { manifest: None, .. });
                        #[cfg(not(windows))]
                        let manual = true;
                        if *newer && manual {
                            show_manual_update(ui);
                        }
                        if ui.button(tr("Open release on GitHub")).clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(format!("{GITHUB}/releases/tag/{tag}")));
                        }
                        #[cfg(windows)]
                        if let ReleaseCheck::Latest { manifest: Some(manifest), .. } = result {
                            if let UpdateState::Failed(error) = &self.update_state {
                                ui.label(tr("Could not prepare the update."));
                                ui.label(error);
                            }
                            match &self.update_state {
                                UpdateState::Idle | UpdateState::Failed(_) => {
                                    if ui.button(tr("Download update")).clicked() {
                                        download = Some(manifest.clone());
                                    }
                                }
                                UpdateState::Downloading => {
                                    ui.horizontal(|ui| {
                                        ui.spinner();
                                        ui.label(tr("Downloading and verifying the update…"));
                                    });
                                }
                                UpdateState::Ready(_) => {
                                    let blocked = self.unsaved() || self.busy || self.saving || self.profile_tools.running;
                                    if blocked {
                                        ui.label(tr("Finish file operations and resolve unsaved changes before restarting."));
                                    }
                                    install = ui.add_enabled(!blocked, egui::Button::new(tr("Install and restart"))).clicked();
                                }
                            }
                        }
                    }
                    ReleaseCheck::Unpublished => {
                        ui.label(tr("No public release is available."));
                    }
                    ReleaseCheck::Failed => {
                        ui.label(tr("Could not check GitHub. Try again later."));
                        if ui.button(tr("Open GitHub Releases")).clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(format!("{GITHUB}/releases")));
                        }
                    }
                }
            });
        if !open {
            self.release_result = None;
            #[cfg(windows)]
            {
                self.update_state = UpdateState::Idle;
            }
        }
        #[cfg(windows)]
        if let Some(manifest) = download {
            self.download_update(manifest, ctx);
        } else if install {
            self.install_update(ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_skips_checks_without_consent_and_in_isolated_mode() {
        let context = egui::Context::default();
        let mut app = EditorApp::new(&eframe::CreationContext::_new_kittest(context.clone()));
        app.browser = crate::app::browser::Browser::new(true);
        app.isolated = false;
        app.check_release_on_launch(&context);
        assert!(app.checking_release.is_none());
        app.browser
            .set_check_updates_on_launch(false, false)
            .unwrap();
        app.check_release_on_launch(&context);
        assert!(app.checking_release.is_none());

        app.isolated = true;
        app.browser
            .set_check_updates_on_launch(true, false)
            .unwrap();
        app.check_release_on_launch(&context);
        assert!(app.checking_release.is_none());
    }

    #[test]
    fn launch_only_shows_a_newer_release() {
        let check = |tag: &str| {
            let body = serde_json::json!({"tag_name": tag}).to_string();
            parse_release(body.as_bytes(), b"200")
        };
        assert!(check("v9999.0.0").visible(CheckOrigin::Startup).is_some());
        assert!(
            check(env!("CARGO_PKG_VERSION"))
                .visible(CheckOrigin::Startup)
                .is_none()
        );
        assert!(ReleaseCheck::Failed.visible(CheckOrigin::Startup).is_none());
        assert!(
            ReleaseCheck::Unpublished
                .visible(CheckOrigin::Startup)
                .is_none()
        );
        assert!(ReleaseCheck::Failed.visible(CheckOrigin::Manual).is_some());
    }

    #[test]
    fn release_versions_and_unavailable_repositories() {
        for (tag, expected) in [
            ("v1.0", false),
            ("1.0.0", false),
            ("v1.10.0", true),
            ("0.9.0", false),
        ] {
            let data = serde_json::json!({"tag_name":tag}).to_string();
            assert!(
                matches!(parse_release(data.as_bytes(), b"200"), ReleaseCheck::Latest { newer, .. } if newer == expected)
            );
        }
        assert!(matches!(
            parse_release(b"", b"404"),
            ReleaseCheck::Unpublished
        ));
        assert!(matches!(parse_release(b"{}", b"403"), ReleaseCheck::Failed));
        assert!(matches!(
            parse_release(br#"{"tag_name":"../other"}"#, b"200"),
            ReleaseCheck::Failed
        ));
    }
}
