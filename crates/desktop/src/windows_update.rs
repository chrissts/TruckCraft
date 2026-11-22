use ed25519_dalek::{Signature, VerifyingKey};
use semver::Version;
use sha2::{Digest, Sha256};
use std::{
    ffi::OsStr,
    fs::{self, File, OpenOptions},
    io::{self, Read},
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, WAIT_OBJECT_0},
    Storage::FileSystem::ReplaceFileW,
    System::Threading::{OpenProcess, WaitForSingleObject},
    UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW},
};

const PUBLIC_KEY: &str = include_str!("../../../release/windows-update-public-key.txt");
const MARKER: &str = "<!-- truckcraft-windows-update-v1 ";
const PREFIX: &str = ".truckcraft-update-";
const UPDATE_EXE: &str = "download.exe";
const HELPER_EXE: &str = "updater.exe";
const BACKUP_EXE: &str = "previous.exe";
const MANIFEST: &str = "update.txt";
const READY: &str = "ready";
const APPLY: &str = "--apply-windows-update";
const CONFIRM: &str = "--confirm-windows-update";
const MAX_DOWNLOAD: u64 = 100 * 1024 * 1024;
const MAX_MANIFEST: u64 = 512;

#[derive(Clone, Debug)]
pub(crate) struct Manifest {
    tag: String,
    size: u64,
    digest: [u8; 32],
    line: String,
}

impl Manifest {
    pub(crate) fn from_release(notes: &str, tag: &str) -> Option<Self> {
        if !cfg!(target_arch = "x86_64") {
            return None;
        }
        let mut markers = notes.lines().filter(|line| line.starts_with(MARKER));
        let line = markers.next()?;
        if markers.next().is_some() {
            return None;
        }
        Self::verify(line, PUBLIC_KEY).filter(|manifest| manifest.tag == tag)
    }

    fn verify(line: &str, public_key: &str) -> Option<Self> {
        if line.len() as u64 > MAX_MANIFEST {
            return None;
        }
        let fields = line.strip_prefix(MARKER)?.strip_suffix(" -->")?;
        let mut fields = fields.split(' ');
        let tag = fields.next()?;
        let actual = Version::parse(tag.strip_prefix('v')?).ok()?;
        let installed = Version::parse(env!("CARGO_PKG_VERSION")).ok()?;
        if !actual.pre.is_empty()
            || !actual.build.is_empty()
            || actual.cmp_precedence(&installed).is_le()
        {
            return None;
        }
        let size = fields.next()?;
        let length = size
            .parse::<u64>()
            .ok()
            .filter(|n| (1..=MAX_DOWNLOAD).contains(n))?;
        let sha = fields.next()?;
        let signature = fields.next()?;
        if fields.next().is_some() {
            return None;
        }
        let key = VerifyingKey::from_bytes(&decode_hex(public_key.trim())?).ok()?;
        let signature = Signature::from_bytes(&decode_hex(signature)?);
        let signed = format!("TruckCraft Windows x64 update v1\n{tag}\n{size}\n{sha}\n");
        key.verify_strict(signed.as_bytes(), &signature).ok()?;
        Some(Self {
            tag: tag.into(),
            size: length,
            digest: decode_hex(sha)?,
            line: line.into(),
        })
    }

    fn verify_file(&self, path: &Path) -> io::Result<()> {
        let file = File::open(path)?;
        let mut reader = file.take(self.size + 1);
        let mut hash = Sha256::new();
        let mut buffer = [0; 16 * 1024];
        let mut size = 0;
        loop {
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hash.update(&buffer[..read]);
            size += read as u64;
        }
        let digest: [u8; 32] = hash.finalize().into();
        if size != self.size || digest != self.digest {
            return Err(invalid(
                "The downloaded update did not match its signed manifest.",
            ));
        }
        Ok(())
    }
}

fn decode_hex<const N: usize>(value: &str) -> Option<[u8; N]> {
    let mut bytes = [0; N];
    hex::decode_to_slice(value, &mut bytes).ok()?;
    Some(bytes)
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub(crate) struct Prepared {
    stage: tempfile::TempDir,
    target: PathBuf,
}

impl Prepared {
    pub(crate) fn download(manifest: &Manifest) -> io::Result<Self> {
        let target = std::env::current_exe()?;
        let parent = target
            .parent()
            .ok_or_else(|| invalid("Missing application folder."))?;
        let stage = tempfile::Builder::new().prefix(PREFIX).tempdir_in(parent)?;
        let candidate = stage.path().join(UPDATE_EXE);
        let url = format!(
            "https://github.com/chrissts/TruckCraft/releases/download/{}/TruckCraft.exe",
            manifest.tag
        );
        let curl = PathBuf::from(
            std::env::var_os("SystemRoot")
                .ok_or_else(|| invalid("Missing Windows system folder."))?,
        )
        .join("System32/curl.exe");
        use std::os::windows::process::CommandExt;
        let mut output = File::create_new(&candidate)?;
        let mut download = Command::new(curl)
            .creation_flags(0x08000000)
            .args([
                "--disable",
                "--silent",
                "--fail",
                "--location",
                "--max-redirs",
                "4",
                "--proto",
                "=https",
                "--proto-redir",
                "=https",
                "--connect-timeout",
                "10",
                "--max-time",
                "180",
                "--max-filesize",
            ])
            .arg(manifest.size.to_string())
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let size = io::copy(
            &mut download
                .stdout
                .take()
                .expect("piped curl output")
                .take(manifest.size + 1),
            &mut output,
        );
        if !matches!(size, Ok(size) if size == manifest.size) {
            let _ = download.kill();
            let _ = download.wait();
            return Err(invalid("The update download was incomplete or too large."));
        }
        if !download.wait()?.success() {
            return Err(invalid("GitHub could not provide the update."));
        }
        output.sync_all()?;
        drop(output);
        manifest.verify_file(&candidate)?;
        fs::write(stage.path().join(MANIFEST), &manifest.line)?;
        fs::copy(&target, stage.path().join(HELPER_EXE))?;
        Ok(Self { stage, target })
    }

    pub(crate) fn start(self) -> io::Result<()> {
        Command::new(self.stage.path().join(HELPER_EXE))
            .arg(APPLY)
            .arg(&self.target)
            .arg(std::process::id().to_string())
            .arg(if crate::isolated_launch() {
                "--isolated"
            } else {
                "--normal"
            })
            .spawn()?;
        let _ = self.stage.keep();
        Ok(())
    }
}

fn validate_stage(stage: &Path, target: &Path) -> io::Result<PathBuf> {
    let stage = stage.canonicalize()?;
    if !stage.is_dir()
        || !stage
            .file_name()
            .and_then(OsStr::to_str)
            .is_some_and(|name| name.starts_with(PREFIX))
        || stage.parent() != target.parent()
    {
        return Err(invalid("The update staging folder is invalid."));
    }
    Ok(stage)
}

fn wait_for_exit(pid: u32, timeout: Duration) -> io::Result<()> {
    if pid == 0 || pid == std::process::id() {
        return Err(invalid("The update process ID is invalid."));
    }
    // SYNCHRONIZE grants waiting only. An already-exited process needs no wait.
    let handle = unsafe { OpenProcess(0x0010_0000, 0, pid) };
    if handle.is_null() {
        let error = io::Error::last_os_error();
        return if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
            Ok(())
        } else {
            Err(error)
        };
    }
    let result = unsafe { WaitForSingleObject(handle, timeout.as_millis() as u32) };
    unsafe { CloseHandle(handle) };
    if result != WAIT_OBJECT_0 {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "TruckCraft did not exit before the updater timed out.",
        ));
    }
    Ok(())
}

fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

fn replace(target: &Path, replacement: &Path, backup: Option<&Path>) -> io::Result<()> {
    let target = wide(target.as_os_str());
    let replacement = wide(replacement.as_os_str());
    let backup = backup.map(|path| wide(path.as_os_str()));
    let backup = backup
        .as_ref()
        .map_or(std::ptr::null(), |path| path.as_ptr());
    // These UTF-16 strings stay alive for the call. All paths are on the same volume.
    let result = unsafe {
        ReplaceFileW(
            target.as_ptr(),
            replacement.as_ptr(),
            backup,
            0,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if result == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn restart(target: &Path, stage: &Path, isolated: bool) -> io::Result<Child> {
    let mut command = Command::new(target);
    command
        .arg(CONFIRM)
        .arg(stage)
        .arg(std::process::id().to_string());
    if isolated {
        command.arg("--isolated");
    }
    command.spawn()
}

fn apply(stage: &Path, target: &Path, isolated: bool, public_key: &str) -> io::Result<()> {
    let candidate = stage.join(UPDATE_EXE);
    let backup = stage.join(BACKUP_EXE);
    let mut line = String::new();
    File::open(stage.join(MANIFEST))?
        .take(MAX_MANIFEST + 1)
        .read_to_string(&mut line)?;
    let manifest = Manifest::verify(&line, public_key)
        .ok_or_else(|| invalid("The update signature is invalid."))?;
    manifest.verify_file(&candidate)?;

    if let Err(error) = replace(target, &candidate, Some(&backup)) {
        if !target.exists() && backup.is_file() {
            fs::rename(&backup, target)?;
        }
        return Err(error);
    }
    let mut child = restart(target, stage, isolated);
    if let Ok(child) = &mut child {
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            if stage.join(READY).is_file() {
                return Ok(());
            }
            if !matches!(child.try_wait(), Ok(None)) {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        // An acknowledgement can arrive between the final poll and process exit.
        if stage.join(READY).is_file() {
            return Ok(());
        }
        if child.try_wait()?.is_none()
            && let Err(error) = child.kill()
            && child.try_wait()?.is_none()
        {
            return Err(error);
        }
        child.wait()?;
    }

    replace(target, &backup, None)?;
    Err(invalid(
        "The new version did not start. The previous version was restored.",
    ))
}

fn show_failure(error: &io::Error) {
    let title = wide(OsStr::new("TruckCraft update"));
    let message = wide(OsStr::new(&format!(
        "Could not install the update.\n\n{error}"
    )));
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

pub(crate) fn run_helper() -> Option<io::Result<()>> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(OsStr::new(APPLY)) {
        return None;
    }
    let result = (|| {
        let target = PathBuf::from(
            args.next()
                .ok_or_else(|| invalid("Missing update target."))?,
        )
        .canonicalize()?;
        let pid = args
            .next()
            .and_then(|value| value.to_str()?.parse().ok())
            .ok_or_else(|| invalid("Missing application process ID."))?;
        let isolated = match args.next().as_deref().and_then(OsStr::to_str) {
            Some("--isolated") => true,
            Some("--normal") => false,
            _ => return Err(invalid("Missing relaunch mode.")),
        };
        let helper = std::env::current_exe()?;
        let folder = helper
            .parent()
            .ok_or_else(|| invalid("Missing updater folder."))?;
        let stage = validate_stage(folder, &target)?;
        if helper.file_name() != Some(OsStr::new(HELPER_EXE)) || args.next().is_some() {
            return Err(invalid("Unexpected updater launch."));
        }
        let parent = stage.parent().expect("validated stage has a parent");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(parent.join(".truckcraft-update.lock"))?;
        lock.try_lock()
            .map_err(|_| invalid("Another TruckCraft update is in progress."))?;
        wait_for_exit(pid, Duration::from_secs(30))?;
        if let Err(error) = apply(&stage, &target, isolated, PUBLIC_KEY) {
            show_failure(&error);
            if target.is_file() && !stage.join(BACKUP_EXE).exists() {
                restart(&target, &stage, isolated)?;
            }
        }
        Ok(())
    })();
    if let Err(error) = &result {
        show_failure(error);
    }
    Some(result)
}

pub(crate) struct Startup {
    stage: PathBuf,
    helper: u32,
}

impl Startup {
    pub(crate) fn from_args() -> Option<Self> {
        let mut args = std::env::args_os().skip(1);
        if args.next().as_deref() != Some(OsStr::new(CONFIRM)) {
            return None;
        }
        let target = std::env::current_exe().ok()?.canonicalize().ok()?;
        let stage = validate_stage(&PathBuf::from(args.next()?), &target).ok()?;
        let helper = args.next()?.to_str()?.parse().ok()?;
        if !stage.join(HELPER_EXE).is_file() {
            return None;
        }
        Some(Self { stage, helper })
    }

    pub(crate) fn confirm(self) {
        if File::create_new(self.stage.join(READY)).is_err() {
            return;
        }
        thread::spawn(move || {
            if wait_for_exit(self.helper, Duration::from_secs(120)).is_err() {
                return;
            }
            for name in [HELPER_EXE, UPDATE_EXE, BACKUP_EXE, MANIFEST, READY] {
                let _ = fs::remove_file(self.stage.join(name));
            }
            let _ = fs::remove_dir(self.stage);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn signed(tag: &str, bytes: &[u8]) -> (String, String) {
        let size = bytes.len();
        let digest = hex::encode(Sha256::digest(bytes));
        let key = SigningKey::from_bytes(&[42; 32]);
        let signature = key.sign(
            format!("TruckCraft Windows x64 update v1\n{tag}\n{size}\n{digest}\n").as_bytes(),
        );
        (
            format!(
                "{MARKER}{tag} {size} {digest} {} -->",
                hex::encode(signature.to_bytes())
            ),
            hex::encode(key.verifying_key().to_bytes()),
        )
    }

    #[test]
    fn rejects_unsigned_tampered_and_non_newer_metadata() {
        let (line, key) = signed("v9999.0.0", b"update");
        assert!(Manifest::verify(&line, &key).is_some());
        assert!(Manifest::verify(&line, PUBLIC_KEY).is_none());
        assert!(Manifest::verify(&line.replace("v9999.0.0", "v9999.0.1"), &key).is_none());
        assert!(Manifest::verify(&line.replace(" 6 ", " 7 "), &key).is_none());
        for tag in ["v0.0.0", "v9999.0.0-beta", "v9999.0.0+build"] {
            let (line, key) = signed(tag, b"update");
            assert!(Manifest::verify(&line, &key).is_none());
        }
    }

    #[test]
    fn validates_download_and_can_restore_the_previous_file() -> io::Result<()> {
        let folder = tempfile::tempdir()?;
        let target = folder.path().join("app.exe");
        let update = folder.path().join(UPDATE_EXE);
        let backup = folder.path().join(BACKUP_EXE);
        fs::write(&target, b"original")?;
        fs::write(&update, b"wrong!")?;
        let (line, key) = signed("v9999.0.0", b"update");
        let manifest = Manifest::verify(&line, &key).expect("valid signature");
        assert!(manifest.verify_file(&update).is_err());
        fs::write(&update, b"update\0")?;
        assert!(manifest.verify_file(&update).is_err());
        fs::write(&update, b"update")?;
        manifest.verify_file(&update)?;
        replace(&target, &update, Some(&backup))?;
        assert_eq!(fs::read(&target)?, b"update");
        assert_eq!(fs::read(&backup)?, b"original");
        replace(&target, &backup, None)?;
        assert_eq!(fs::read(target)?, b"original");
        Ok(())
    }

    #[test]
    fn restores_the_original_when_the_replacement_cannot_launch() -> io::Result<()> {
        let folder = tempfile::tempdir()?;
        let stage = folder.path().join(".truckcraft-update-test");
        let target = folder.path().join("app.exe");
        fs::create_dir(&stage)?;
        fs::write(&target, b"old executable")?;
        fs::write(stage.join(UPDATE_EXE), b"invalid executable")?;
        let (line, key) = signed("v9999.0.0", b"invalid executable");
        fs::write(stage.join(MANIFEST), line)?;
        let error = apply(&stage.canonicalize()?, &target.canonicalize()?, true, &key)
            .expect_err("invalid executable must fail to launch");
        assert!(error.to_string().contains("previous version was restored"));
        assert_eq!(fs::read(target)?, b"old executable");
        assert!(!stage.join(BACKUP_EXE).exists());
        Ok(())
    }
}
