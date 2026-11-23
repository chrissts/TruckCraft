//! Exercise the installed native binary in a disposable game home.
use super::*;
use std::{
    fs,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

enum Step {
    Start,
    Edit,
    Reopen,
    Restore,
    Verify,
}
struct PackageCheck {
    editor: EditorApp,
    _directory: tempfile::TempDir,
    slot: PathBuf,
    step: Step,
    started: Instant,
    passed: Arc<AtomicBool>,
}

impl PackageCheck {
    fn advance(&mut self, ctx: &egui::Context) -> Result<()> {
        if self.started.elapsed() > Duration::from_secs(60) {
            return Err(Error::InvalidSii("package check timed out"));
        }
        if self.editor.busy {
            return Ok(());
        }
        if let Some(error) = self.editor.error.take() {
            return Err(Error::InvalidEdit(error));
        }
        if matches!(self.step, Step::Start | Step::Reopen) {
            self.editor
                .begin_load(ctx.clone(), self.slot.clone(), Game::Ets2);
            self.step = if matches!(self.step, Step::Start) {
                Step::Edit
            } else {
                Step::Restore
            };
            return Ok(());
        }
        let session = self
            .editor
            .session
            .as_mut()
            .ok_or(Error::InvalidSii("sample failed to open"))?;
        let balance = |s: &SaveSession| -> Result<i64> {
            s.data.game.integer(s.data.bank_id()?, "money_account")
        };
        match self.step {
            Step::Edit => {
                session.edit(|data| editor::set_balance(data, 9007199254740993))?;
                if !session.undo()
                    || balance(session)? != 100
                    || !session.redo()
                    || balance(session)? != 9007199254740993
                {
                    return Err(Error::InvalidSii("sample undo or redo failed"));
                }
                self.editor.begin_save(ctx.clone());
                self.step = Step::Reopen;
            }
            Step::Restore => {
                if balance(session)? != 9007199254740993 {
                    return Err(Error::InvalidSii("sample balance changed on disk"));
                }
                let snapshots = session.recovery_snapshots()?;
                let [snapshot] = snapshots.as_slice() else {
                    return Err(Error::InvalidSii("expected one recovery copy"));
                };
                session.restore_recovery(snapshot)?;
                self.editor.begin_save(ctx.clone());
                self.step = Step::Verify;
            }
            Step::Verify => {
                let restored = SaveSession::load(&self.slot, Game::Ets2)?;
                if balance(&restored)? != 100 {
                    return Err(Error::InvalidSii("sample recovery failed"));
                }
                self.passed.store(true, Ordering::Relaxed);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            _ => unreachable!(),
        }
        Ok(())
    }
}

impl eframe::App for PackageCheck {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.editor.ui(ui, frame);
        if let Err(error) = self.advance(ui.ctx()) {
            self.editor.allow_close = true;
            eprintln!("{error}");
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ui.ctx().request_repaint_after(Duration::from_millis(30));
    }
}

pub(crate) fn run(options: eframe::NativeOptions) -> eframe::Result {
    let passed = Arc::new(AtomicBool::new(false));
    let result = passed.clone();
    eframe::run_native(
        "TruckCraft package check",
        options,
        Box::new(move |ctx| {
            let directory = tempfile::tempdir()?;
            let slot = directory.path().join("save/1");
            fs::create_dir_all(&slot)?;
            fs::write(
                directory.path().join("profile.sii"),
                "SiiNunit\n{\nuser_profile : profile.sample {\n profile_name: \"Sample Fleet\"\n}\n}\n",
            )?;
            fs::write(
                slot.join("info.sii"),
                "SiiNunit\n{\nsave_container : info.sample {\n version: 105\n}\n}\n",
            )?;
            fs::write(
                slot.join("game.sii"),
                "SiiNunit\n{\neconomy : economy.sample {\n player: player.sample\n bank: bank.sample\n}\nplayer : player.sample {\n}\nbank : bank.sample {\n money_account: 100\n}\n}\n",
            )?;
            let save = slot.join("game.sii");
            if let Ok(mode) = std::env::var("TRUCKCRAFT_CHECK_SAVE_DISCOVERY") {
                let custom = match mode.as_str() {
                    "default" => Vec::new(),
                    "custom" => vec![directory.path().to_path_buf()],
                    _ => return Err("unknown package check discovery mode".into()),
                };
                let catalog = ts_save_core::discovery::discover(Game::Ets2, &custom, false);
                let found = catalog
                    .profiles
                    .iter()
                    .flat_map(|profile| &profile.saves)
                    .any(|entry| entry.path == save);
                if !found {
                    return Err("package check could not discover the disposable save".into());
                }
            }
            Ok(Box::new(PackageCheck {
                editor: EditorApp::new(ctx),
                _directory: directory,
                slot: save,
                step: Step::Start,
                started: Instant::now(),
                passed,
            }))
        }),
    )?;
    if result.load(Ordering::Relaxed) {
        Ok(())
    } else {
        Err(eframe::Error::AppCreation("Package check failed".into()))
    }
}
