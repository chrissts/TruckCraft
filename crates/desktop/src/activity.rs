//! Local, chronological activity log. No telemetry or save contents.
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_LOG: u64 = 2 * 1024 * 1024;
const RETAIN: u64 = 1024 * 1024;
fn local_file(name: &str) -> Option<PathBuf> {
    dirs::config_local_dir().map(|path| path.join("TruckCraft").join(name))
}

pub fn preference_path() -> Option<PathBuf> {
    local_file("settings.json")
}
pub fn configured_logging() -> bool {
    preference_path()
        .and_then(|path| ts_save_core::discovery::read_small(&path, 1024 * 1024).ok())
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|value| {
            value
                .get("logging_enabled")
                .and_then(serde_json::Value::as_bool)
        })
        .unwrap_or(true)
}

pub struct EventLog {
    pub path: Option<PathBuf>,
    pub enabled: bool,
}
impl EventLog {
    pub fn new(isolated: bool) -> Self {
        Self {
            path: if isolated {
                None
            } else {
                local_file("TruckCraft.log")
            },
            enabled: configured_logging(),
        }
    }
    pub fn record(&self, level: &str, message: &str) -> std::io::Result<()> {
        if !self.enabled {
            return Ok(());
        }
        let Some(path) = &self.path else {
            return Ok(());
        };
        fs::create_dir_all(path.parent().expect("log has a parent"))?;
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path)?;
        file.lock()?;
        let message: String = message
            .chars()
            .take(2048)
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        let elapsed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let line = format!(
            "{}.{:03} UTC [{level}] {message}\n",
            timestamp(elapsed.as_secs()),
            elapsed.subsec_millis()
        );
        let length = file.metadata()?.len();
        if length.saturating_add(line.len() as u64) > MAX_LOG {
            file.seek(SeekFrom::Start(length.saturating_sub(RETAIN)))?;
            let mut tail = Vec::new();
            Read::take(&mut file, RETAIN).read_to_end(&mut tail)?;
            let start = tail
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(tail.len(), |index| index + 1);
            file.set_len(0)?;
            file.seek(SeekFrom::Start(0))?;
            file.write_all(&tail[start..])?;
        }
        file.seek(SeekFrom::End(0))?;
        file.write_all(line.as_bytes())?;
        file.flush()?;
        Ok(())
    }
    pub fn read(&self) -> std::io::Result<Option<String>> {
        let Some(path) = &self.path else {
            return Ok(None);
        };
        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        file.lock_shared()?;
        let length = file.metadata()?.len();
        if length > MAX_LOG + 8192 {
            return Err(std::io::Error::other("Log exceeds the viewer size limit."));
        }
        let mut bytes = Vec::new();
        Read::take(&mut file, MAX_LOG + 8193).read_to_end(&mut bytes)?;
        Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
    }
}

pub(crate) fn timestamp(seconds: u64) -> String {
    // Gregorian civil date from days since 1970-01-01, using 400-year eras.
    let z = seconds / 86400 + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    if month <= 2 {
        year += 1;
    }
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}",
        (seconds / 3600) % 24,
        (seconds / 60) % 60,
        seconds % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chronological_log_detects_absence_disable_and_sanitizes_lines() {
        let temporary = tempfile::tempdir().unwrap();
        let mut log = EventLog {
            path: Some(temporary.path().join("TruckCraft.log")),
            enabled: true,
        };
        assert!(log.read().unwrap().is_none());
        log.enabled = false;
        log.record("INFO", "Disabled startup").unwrap();
        assert!(!log.path.as_ref().unwrap().exists());
        log.enabled = true;
        log.record("INFO", "First action").unwrap();
        log.record("ERROR", "Second\nforged line").unwrap();
        let text = log.read().unwrap().unwrap();
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with("First action"));
        assert!(lines[1].ends_with("Second forged line"));
        assert!(lines[0].contains(" UTC [INFO]"));
        log.enabled = false;
        log.record("INFO", "Hidden").unwrap();
        assert_eq!(log.read().unwrap().unwrap(), text);
        log.enabled = true;
        log.record("INFO", "Third action").unwrap();
        assert!(
            log.read()
                .unwrap()
                .unwrap()
                .lines()
                .last()
                .unwrap()
                .ends_with("Third action")
        );
    }
    #[test]
    fn log_compaction_keeps_recent_events_in_time_order_and_dates_are_utc() {
        assert_eq!(timestamp(0), "1970-01-01 00:00:00");
        assert_eq!(timestamp(1709164800), "2024-02-29 00:00:00");
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("TruckCraft.log");
        fs::write(&path, "old event\n".repeat(MAX_LOG as usize / 10 + 1)).unwrap();
        let log = EventLog {
            path: Some(path.clone()),
            enabled: true,
        };
        log.record("INFO", "Newest event").unwrap();
        assert!(fs::metadata(path).unwrap().len() < MAX_LOG);
        assert!(log.read().unwrap().unwrap().ends_with("Newest event\n"));
    }
    #[test]
    fn simultaneous_writers_keep_complete_lines_and_all_events() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("TruckCraft.log");
        let writers: Vec<_> = (0..2)
            .map(|writer| {
                let path = path.clone();
                std::thread::spawn(move || {
                    let log = EventLog {
                        path: Some(path),
                        enabled: true,
                    };
                    for event in 0..25 {
                        log.record("INFO", &format!("Writer {writer}, event {event}"))
                            .unwrap();
                    }
                })
            })
            .collect();
        for writer in writers {
            writer.join().unwrap();
        }
        let log = EventLog {
            path: Some(path),
            enabled: true,
        };
        let text = log.read().unwrap().unwrap();
        assert_eq!(text.lines().count(), 50);
        for writer in 0..2 {
            for event in 0..25 {
                assert_eq!(
                    text.lines()
                        .filter(|line| line.ends_with(&format!("Writer {writer}, event {event}")))
                        .count(),
                    1
                );
            }
        }
    }
}
