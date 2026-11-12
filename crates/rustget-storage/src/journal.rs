use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rustget_core::LedgerSnapshot;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const JOURNAL_VERSION: u32 = 1;
const COMPACT_EVERY: u64 = 512;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalState {
    pub task_id: Uuid,
    pub url: String,
    pub destination: PathBuf,
    pub part_path: PathBuf,
    pub total_size: u64,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub range_supported: bool,
    pub segments: LedgerSnapshot,
    pub complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalRecord {
    pub version: u32,
    pub saved_at_unix_ms: u64,
    pub state: JournalState,
}

#[derive(Debug)]
pub struct JournalStore {
    path: PathBuf,
    file: File,
    records: u64,
}

impl JournalStore {
    pub fn create(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&path)?;
        Ok(Self {
            path,
            file,
            records: 0,
        })
    }

    /// Append one full-state record. Readers take the last complete line, so
    /// simply appending is crash safe even if the process dies mid-write.
    pub fn append(&mut self, state: &JournalState, sync: bool) -> std::io::Result<()> {
        let record = JournalRecord {
            version: JOURNAL_VERSION,
            saved_at_unix_ms: now_ms(),
            state: state.clone(),
        };
        let mut line = serde_json::to_vec(&record)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        line.push(b'\n');
        self.file.write_all(&line)?;
        self.file.flush()?;
        if sync {
            self.file.sync_data()?;
        }
        self.records += 1;
        if self.records >= COMPACT_EVERY {
            self.compact(state)?;
        }
        Ok(())
    }

    /// Replace the append-only journal with a single latest snapshot.
    pub fn compact(&mut self, state: &JournalState) -> std::io::Result<()> {
        let record = JournalRecord {
            version: JOURNAL_VERSION,
            saved_at_unix_ms: now_ms(),
            state: state.clone(),
        };
        let tmp = self.path.with_extension("rustget.tmp");
        {
            let mut file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&tmp)?;
            serde_json::to_writer(&mut file, &record)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            file.write_all(b"\n")?;
            file.sync_data()?;
        }
        // Windows rename does not replace. Keep the previous journal until the
        // replacement is in place; recovery also accepts a complete .tmp file.
        if self.path.exists() {
            std::fs::remove_file(&self.path)?;
        }
        std::fs::rename(&tmp, &self.path)?;
        self.file = OpenOptions::new()
            .append(true)
            .read(true)
            .open(&self.path)?;
        self.records = 1;
        Ok(())
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[derive(Debug, Clone)]
pub struct Journal;

impl Journal {
    /// Load the newest valid snapshot. A torn final line or a stale compacted
    /// temporary file is tolerated; unreadable journals return `None` rather
    /// than silently treating the download as fresh.
    #[must_use]
    pub fn load(path: impl AsRef<Path>) -> Option<JournalState> {
        let path = path.as_ref();
        let tmp = path.with_extension("rustget.tmp");
        let from_main = read_last(path);
        let from_tmp = read_last(&tmp);
        match (from_main, from_tmp) {
            (Some((main_seq, main)), Some((tmp_seq, tmp))) => {
                if tmp_seq > main_seq {
                    Some(tmp)
                } else {
                    Some(main)
                }
            }
            (Some((_, state)), None) => Some(state),
            (None, Some((_, state))) => Some(state),
            (None, None) => None,
        }
    }
}

fn read_last(path: &Path) -> Option<(u64, JournalState)> {
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);
    let mut last = None;
    for line in reader.lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(record) = serde_json::from_str::<JournalRecord>(&line) else {
            // A torn tail is expected after a crash; any later complete record
            // supersedes it.
            continue;
        };
        if record.version != JOURNAL_VERSION {
            continue;
        }
        last = Some((record.saved_at_unix_ms, record.state));
    }
    last
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustget_core::{ByteRange, EngineConfig, SegmentLedger};

    fn state(total: u64) -> JournalState {
        let ledger = SegmentLedger::new(total, EngineConfig::default()).unwrap();
        JournalState {
            task_id: Uuid::new_v4(),
            url: "https://example.test/file.bin".into(),
            destination: PathBuf::from("file.bin"),
            part_path: PathBuf::from("file.bin.part"),
            total_size: total,
            etag: Some("\"abc\"".into()),
            last_modified: None,
            range_supported: true,
            segments: ledger.snapshot(),
            complete: false,
        }
    }

    #[test]
    fn latest_record_wins() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.rustget");
        let mut store = JournalStore::create(&path).unwrap();
        let mut first = state(1_000_000);
        store.append(&first, false).unwrap();
        first.complete = true;
        store.append(&first, true).unwrap();
        let loaded = Journal::load(&path).expect("journal");
        assert!(loaded.complete);
    }

    #[test]
    fn torn_tail_falls_back_to_previous_record() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.rustget");
        let mut store = JournalStore::create(&path).unwrap();
        let first = state(1_000_000);
        store.append(&first, false).unwrap();
        drop(store);
        let mut bytes = std::fs::read(&path).unwrap();
        bytes.extend_from_slice(b"{\"version\":1,\"saved_at_unix_ms\":");
        std::fs::write(&path, bytes).unwrap();
        let loaded = Journal::load(&path).expect("journal");
        assert_eq!(loaded.total_size, 1_000_000);
    }

    #[test]
    fn empty_journal_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Journal::load(dir.path().join("missing.rustget")).is_none());
    }

    #[test]
    fn compaction_keeps_latest_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.rustget");
        let mut store = JournalStore::create(&path).unwrap();
        let mut state = state(4096);
        for _ in 0..5 {
            store.append(&state, false).unwrap();
        }
        state.complete = true;
        store.compact(&state).unwrap();
        assert!(Journal::load(&path).unwrap().complete);
        assert!(!path.with_extension("rustget.tmp").exists());
    }

    #[test]
    fn segment_snapshot_is_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let mut ledger = SegmentLedger::new(100_000, EngineConfig::default()).unwrap();
        let assignments = ledger.initial_assignments();
        ledger
            .report_progress(assignments[0].worker, assignments[0].range.start + 77)
            .unwrap();
        let mut state = state(100_000);
        state.segments = ledger.snapshot();
        let path = dir.path().join("x.rustget");
        JournalStore::create(&path)
            .unwrap()
            .append(&state, true)
            .unwrap();
        let loaded = Journal::load(&path).unwrap();
        assert_eq!(loaded.segments.segments.len(), assignments.len());
        assert_eq!(loaded.segments.segments[0].written, 77);
    }

    #[test]
    fn byte_ranges_survive_json() {
        let range = ByteRange::new(5, 10).unwrap();
        let json = serde_json::to_string(&range).unwrap();
        assert_eq!(serde_json::from_str::<ByteRange>(&json).unwrap(), range);
    }
}
