use super::*;
use semver::Version;
use std::io::Read;
use std::process::{Command, Stdio};

const API: &str = "https://api.github.com/repos/chrissts/TruckCraft/releases/latest";
const LIMIT: usize = 1024 * 1024;

pub(super) enum ReleaseCheck {
    Latest { tag: String, newer: bool },
    Unpublished,
    Failed,
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
    }
    let Ok(Release { tag_name }) = serde_json::from_slice(body) else {
        return ReleaseCheck::Failed;
    };
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
    ReleaseCheck::Latest {
        tag: tag_name,
        newer: latest.cmp_precedence(&installed).is_gt(),
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
    pub(super) fn check_release(&mut self, ctx: &egui::Context) {
        if self.checking_release {
            return;
        }
        self.release_result = None;
        self.checking_release = true;
        let sender = self.tx.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let _ = sender.send(Task::ReleaseChecked(
                fetch().unwrap_or(ReleaseCheck::Failed),
            ));
            ctx.request_repaint();
        });
    }

    pub(super) fn release_dialog(&mut self, ctx: &egui::Context) {
        let Some(result) = &self.release_result else {
            return;
        };
        let mut open = true;
        egui::Window::new(tr("Check for updates"))
            .id(egui::Id::unique("release_result"))
            .open(&mut open)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.label(format!("{} {}", tr("Installed version"), display_version()));
                match result {
                    ReleaseCheck::Latest { tag, newer } => {
                        ui.label(format!("{} {tag}", tr("Latest release")));
                        ui.label(tr(if *newer {
                            "An update is available."
                        } else {
                            "You are up to date."
                        }));
                        ui.hyperlink_to(
                            tr("Open release on GitHub"),
                            format!("{GITHUB}/releases/tag/{tag}"),
                        );
                    }
                    ReleaseCheck::Unpublished => {
                        ui.label(tr("No public release is available."));
                    }
                    ReleaseCheck::Failed => {
                        ui.label(tr("Could not check GitHub. Try again later."));
                    }
                }
            });
        if !open {
            self.release_result = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
