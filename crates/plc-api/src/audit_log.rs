//! Append-only audit log. Rotates at 16 MiB and keeps 8 files.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use plc_auth::{AuditAction, AuditEvent, AuditSink};
use serde::{Deserialize, Serialize};

/// Active file size that triggers rotation.
pub const AUDIT_MAX_BYTES: u64 = 16 * 1024 * 1024;
/// Active file plus rotated siblings.
pub const AUDIT_MAX_FILES: usize = 8;

const ACTIVE_NAME: &str = "audit.jsonl";
const INCOMING_NAME: &str = "audit.jsonl.new";
const DROPPING_NAME: &str = "audit.jsonl.dropping";
/// Tail window used to learn a segment's last sequence without reading it all.
const TAIL_WINDOW: u64 = 64 * 1024;

/// One persisted audit row (export and tests).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditRecord {
    /// Monotonic sequence, stable across rotation and reopen.
    pub seq: u64,
    /// Wall-clock unix seconds.
    pub unix_secs: u64,
    /// Principal id.
    pub principal_id: String,
    /// Action name (`ProgramArm`, …).
    pub action: String,
    /// Free-form detail.
    pub detail: String,
    /// Client address, when the request had one.
    pub client_ip: Option<String>,
}

struct ActiveFile {
    file: Option<File>,
    len: u64,
    next_seq: u64,
}

/// Directory of `audit.jsonl` plus `audit.jsonl.1` … `audit.jsonl.{n-1}`.
pub struct RotatingAudit {
    dir: PathBuf,
    max_bytes: u64,
    max_files: usize,
    inner: Mutex<ActiveFile>,
}

impl RotatingAudit {
    /// Open `dir` with the production limits ([`AUDIT_MAX_BYTES`] × [`AUDIT_MAX_FILES`]).
    pub fn open(dir: impl Into<PathBuf>) -> io::Result<Self> {
        Self::open_with_limits(dir, AUDIT_MAX_BYTES, AUDIT_MAX_FILES)
    }

    /// Open `dir`. `max_files` includes the active file (at least 2).
    pub fn open_with_limits(
        dir: impl Into<PathBuf>,
        max_bytes: u64,
        max_files: usize,
    ) -> io::Result<Self> {
        let dir = dir.into();
        let max_files = max_files.max(2);
        let max_bytes = max_bytes.max(1);
        fs::create_dir_all(&dir)?;
        recover_incomplete_rotate(&dir, max_files)?;
        let active_path = dir.join(ACTIVE_NAME);
        let len = repair_torn_tail(&active_path)?;
        let next_seq = max_seq(&dir, max_files)?.saturating_add(1);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&active_path)?;
        Ok(Self {
            dir,
            max_bytes,
            max_files,
            inner: Mutex::new(ActiveFile {
                file: Some(file),
                len,
                next_seq,
            }),
        })
    }

    /// Rows with `seq > cursor`, oldest first.
    pub fn page(&self, cursor: u64, limit: usize) -> io::Result<Vec<AuditRecord>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        // Open the segments, then release the writer lock before reading.
        let mut files = {
            let _guard = self.inner.lock().expect("audit log");
            let mut opened = Vec::new();
            for path in files_oldest_first(&self.dir, self.max_files) {
                match File::open(&path) {
                    Ok(file) => opened.push(file),
                    Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                    Err(e) => return Err(e),
                }
            }
            opened
        };
        let mut out = Vec::new();
        for file in &mut files {
            if let Some(last) = trailing_seq(file)? {
                if last <= cursor {
                    continue;
                }
            }
            file.seek(SeekFrom::Start(0))?;
            let mut text = String::new();
            file.read_to_string(&mut text)?;
            for line in text.lines() {
                if line.is_empty() {
                    continue;
                }
                let Ok(row) = serde_json::from_str::<AuditRecord>(line) else {
                    continue;
                };
                if row.seq > cursor {
                    out.push(row);
                    if out.len() == limit {
                        return Ok(out);
                    }
                }
            }
        }
        Ok(out)
    }

    fn append(&self, event: AuditEvent) -> io::Result<()> {
        let mut inner = self.inner.lock().expect("audit log");
        let seq = inner.next_seq;
        let line = serde_json::to_string(&AuditRecord {
            seq,
            unix_secs: event.unix_secs,
            principal_id: event.principal_id,
            action: action_name(event.action).to_string(),
            detail: event.detail,
            client_ip: event.client_ip.map(|ip| ip.to_string()),
        })
        .expect("audit record is plain json");
        let bytes = {
            let mut bytes = line.into_bytes();
            bytes.push(b'\n');
            bytes
        };
        let add = bytes.len() as u64;
        if inner.len > 0 && inner.len.saturating_add(add) > self.max_bytes {
            self.rotate(&mut inner)?;
        }
        let start = inner.len;
        let path = self.dir.join(ACTIVE_NAME);
        let file = inner
            .file
            .as_mut()
            .ok_or_else(|| io::Error::other("audit file closed"))?;
        // Advance only after the line is the readable tail. A failed write
        // truncates back to `start` so the next record can reuse `seq`.
        commit_line(&path, file, start, &bytes)?;
        inner.len = start.saturating_add(add);
        inner.next_seq = inner.next_seq.saturating_add(1);
        Ok(())
    }

    fn rotate(&self, inner: &mut ActiveFile) -> io::Result<()> {
        let current = inner
            .file
            .as_ref()
            .ok_or_else(|| io::Error::other("audit file closed"))?;
        current.sync_all()?;

        let incoming_path = self.dir.join(INCOMING_NAME);
        let dropping = self.dir.join(DROPPING_NAME);
        let oldest = self.max_files - 1;

        // Replacement exists before any name moves, so a later failure can
        // leave the current handle on `audit.jsonl`. Append and truncate
        // together are EINVAL, so empty the path first, then open append-only.
        if incoming_path.exists() {
            fs::remove_file(&incoming_path)?;
        }
        let incoming = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&incoming_path)?;
        incoming.sync_all()?;

        if let Err(err) = shift_segments(&self.dir, oldest) {
            abort_rotate(&self.dir, oldest);
            return Err(err);
        }

        // Names already point at the new active inode. Publish the handle
        // before directory sync so a sync error cannot keep appending into
        // the segment that was just rotated away.
        inner.file = Some(incoming);
        inner.len = 0;
        sync_dir(&self.dir)?;
        let _ = fs::remove_file(&dropping);
        Ok(())
    }
}

impl AuditSink for RotatingAudit {
    fn record(&self, event: AuditEvent) {
        if let Err(err) = self.append(event) {
            tracing::error!("audit append failed: {err}");
        }
    }
}

fn action_name(action: AuditAction) -> &'static str {
    match action {
        AuditAction::AuthSuccess => "AuthSuccess",
        AuditAction::AuthFailure => "AuthFailure",
        AuditAction::AuthLocked => "AuthLocked",
        AuditAction::ModeChange => "ModeChange",
        AuditAction::ProgramArm => "ProgramArm",
        AuditAction::ProgramActivate => "ProgramActivate",
        AuditAction::ConfigWrite => "ConfigWrite",
        AuditAction::TagForce => "TagForce",
        AuditAction::UserAdmin => "UserAdmin",
    }
}

fn rotated_name(index: usize) -> String {
    format!("{ACTIVE_NAME}.{index}")
}

fn files_oldest_first(dir: &Path, max_files: usize) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for i in (1..max_files).rev() {
        let path = dir.join(rotated_name(i));
        if path.exists() {
            paths.push(path);
        }
    }
    let active = dir.join(ACTIVE_NAME);
    if active.exists() {
        paths.push(active);
    }
    paths
}

fn shift_segments(dir: &Path, oldest: usize) -> io::Result<()> {
    let active = dir.join(ACTIVE_NAME);
    let incoming = dir.join(INCOMING_NAME);
    let dropping = dir.join(DROPPING_NAME);
    let oldest_path = dir.join(rotated_name(oldest));
    if oldest_path.exists() {
        if dropping.exists() {
            fs::remove_file(&dropping)?;
        }
        fs::rename(&oldest_path, &dropping)?;
    }
    for i in (1..oldest).rev() {
        let from = dir.join(rotated_name(i));
        if from.exists() {
            fs::rename(&from, dir.join(rotated_name(i + 1)))?;
        }
    }
    if active.exists() {
        fs::rename(&active, dir.join(rotated_name(1)))?;
    }
    fs::rename(&incoming, &active)?;
    Ok(())
}

fn abort_rotate(dir: &Path, oldest: usize) {
    let active = dir.join(ACTIVE_NAME);
    let incoming = dir.join(INCOMING_NAME);
    let dropping = dir.join(DROPPING_NAME);
    let oldest_path = dir.join(rotated_name(oldest));
    if !active.exists() {
        let _ = fs::rename(dir.join(rotated_name(1)), &active);
    }
    if incoming.exists() {
        let _ = fs::remove_file(&incoming);
    }
    if dropping.exists() && !oldest_path.exists() {
        let _ = fs::rename(&dropping, &oldest_path);
    }
}

/// Put a crashed rotate back so the numbered segments match `page`.
fn recover_incomplete_rotate(dir: &Path, max_files: usize) -> io::Result<()> {
    let incoming = dir.join(INCOMING_NAME);
    if incoming.exists() {
        fs::remove_file(&incoming)?;
    }
    let dropping = dir.join(DROPPING_NAME);
    if !dropping.exists() {
        return Ok(());
    }
    let oldest = dir.join(rotated_name(max_files - 1));
    if oldest.exists() {
        fs::remove_file(&dropping)?;
    } else {
        fs::rename(&dropping, &oldest)?;
    }
    Ok(())
}

/// Make `bytes` the durable tail at `start`, or put the file back if not.
fn commit_line(path: &Path, file: &mut File, start: u64, bytes: &[u8]) -> io::Result<()> {
    if tail_matches(path, start, bytes)? {
        return Ok(());
    }
    rewind_to(file, start)?;
    if let Err(err) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        if tail_matches(path, start, bytes)? {
            return Ok(());
        }
        rewind_to(file, start)?;
        return Err(err);
    }
    Ok(())
}

fn rewind_to(file: &mut File, start: u64) -> io::Result<()> {
    if file.metadata()?.len() != start {
        file.set_len(start)?;
        file.sync_all()?;
    }
    Ok(())
}

fn tail_matches(path: &Path, start: u64, bytes: &[u8]) -> io::Result<bool> {
    let mut reader = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    let len = reader.metadata()?.len();
    let add = bytes.len() as u64;
    if len != start.saturating_add(add) {
        return Ok(false);
    }
    reader.seek(SeekFrom::Start(start))?;
    let mut buf = vec![0u8; bytes.len()];
    reader.read_exact(&mut buf)?;
    Ok(buf == bytes)
}

/// Drop a trailing partial line by shortening the file. The prefix stays put.
fn repair_torn_tail(path: &Path) -> io::Result<u64> {
    if !path.exists() {
        return Ok(0);
    }
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    let len = file.metadata()?.len();
    if len == 0 {
        return Ok(0);
    }
    file.seek(SeekFrom::End(-1))?;
    let mut last = [0u8; 1];
    file.read_exact(&mut last)?;
    if last[0] == b'\n' {
        return Ok(len);
    }
    let keep = prefix_end_before_torn_tail(&mut file, len)?;
    if keep < len {
        file.set_len(keep)?;
        file.sync_all()?;
    }
    Ok(keep)
}

fn prefix_end_before_torn_tail(file: &mut File, len: u64) -> io::Result<u64> {
    let mut pos = len;
    let mut buf = [0u8; 8192];
    while pos > 0 {
        let window = pos.min(buf.len() as u64);
        pos -= window;
        file.seek(SeekFrom::Start(pos))?;
        file.read_exact(&mut buf[..window as usize])?;
        if let Some(i) = buf[..window as usize]
            .iter()
            .rposition(|byte| *byte == b'\n')
        {
            return Ok(pos + i as u64 + 1);
        }
    }
    Ok(0)
}

fn trailing_seq(file: &mut File) -> io::Result<Option<u64>> {
    let len = file.metadata()?.len();
    if len == 0 {
        return Ok(None);
    }
    let window = len.min(TAIL_WINDOW);
    file.seek(SeekFrom::Start(len - window))?;
    let mut buf = vec![0u8; window as usize];
    file.read_exact(&mut buf)?;
    let text = String::from_utf8_lossy(&buf);
    let mut max_seq = None;
    for line in text.lines() {
        if let Ok(row) = serde_json::from_str::<AuditRecord>(line) {
            max_seq = Some(row.seq.max(max_seq.unwrap_or(0)));
        }
    }
    Ok(max_seq)
}

fn max_seq(dir: &Path, max_files: usize) -> io::Result<u64> {
    let mut max = 0u64;
    for path in files_oldest_first(dir, max_files) {
        let text = fs::read_to_string(&path)?;
        for line in text.lines() {
            if let Ok(row) = serde_json::from_str::<AuditRecord>(line) {
                max = max.max(row.seq);
            }
        }
    }
    Ok(max)
}

fn sync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(detail: &str) -> AuditEvent {
        AuditEvent {
            unix_secs: 1,
            principal_id: "eng".into(),
            action: AuditAction::ProgramArm,
            detail: detail.into(),
            client_ip: None,
        }
    }

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "plc-audit-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn rotates_keeps_eight_and_reopens_in_seq_order() {
        let dir = temp_dir("rotate");
        let log = RotatingAudit::open_with_limits(&dir, 1, 8).expect("open");
        for i in 0..20 {
            log.record(event(&format!("e{i}")));
        }
        let files = fs::read_dir(&dir).expect("dir").count();
        assert_eq!(files, 8);
        let page = log.page(0, 1000).expect("page");
        assert_eq!(page.len(), 8);
        assert_eq!(page[0].seq, 13);
        assert_eq!(page[7].seq, 20);
        assert!(page.windows(2).all(|pair| pair[0].seq + 1 == pair[1].seq));
        assert_eq!(page[0].detail, "e12");
        drop(log);

        let reopened = RotatingAudit::open_with_limits(&dir, 1, 8).expect("reopen");
        let again = reopened.page(0, 1000).expect("page");
        assert_eq!(again, page);
        let after = reopened.page(18, 1000).expect("cursor");
        assert_eq!(after.len(), 2);
        assert_eq!(after[0].seq, 19);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn failed_rotate_keeps_the_active_file_writable() {
        let dir = temp_dir("rotate-fail");
        let log = RotatingAudit::open_with_limits(&dir, 1, 8).expect("open");
        log.record(event("first"));
        // Oldest slot exists, and the park destination is a directory, so the
        // shift fails after the replacement file is created and before the
        // active name moves.
        fs::write(dir.join("audit.jsonl.7"), b"old\n").expect("oldest");
        fs::create_dir(dir.join("audit.jsonl.dropping")).expect("park dir");
        fs::write(dir.join("audit.jsonl.dropping/blocker"), b"x").expect("blocker");
        log.record(event("second"));
        fs::remove_file(dir.join("audit.jsonl.dropping/blocker")).expect("clear blocker");
        fs::remove_dir(dir.join("audit.jsonl.dropping")).expect("clear park");
        fs::remove_file(dir.join("audit.jsonl.7")).expect("clear oldest");
        log.record(event("third"));
        let page = log.page(0, 20).expect("page");
        assert_eq!(
            page.iter()
                .map(|row| row.detail.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "third"]
        );
        assert_eq!(page[0].seq, 1);
        assert_eq!(page[1].seq, 2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn repair_drops_only_the_partial_tail() {
        let dir = temp_dir("repair");
        let log = RotatingAudit::open_with_limits(&dir, AUDIT_MAX_BYTES, 8).expect("open");
        log.record(event("keep"));
        drop(log);
        let path = dir.join(ACTIVE_NAME);
        let mut extra = OpenOptions::new().append(true).open(&path).expect("append");
        extra.write_all(b"{\"seq\":").expect("tear");
        extra.sync_all().expect("sync tear");
        drop(extra);
        let before = fs::read(&path).expect("read torn");
        let torn = b"{\"seq\":";
        assert!(before.ends_with(torn));
        let reopened = RotatingAudit::open_with_limits(&dir, AUDIT_MAX_BYTES, 8).expect("reopen");
        let page = reopened.page(0, 10).expect("page");
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].detail, "keep");
        let after = fs::read(&path).expect("read repaired");
        assert_eq!(after, &before[..before.len() - torn.len()]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn append_cuts_uncommitted_tail_before_the_next_record() {
        let dir = temp_dir("rollback");
        let log = RotatingAudit::open_with_limits(&dir, AUDIT_MAX_BYTES, 8).expect("open");
        log.record(event("one"));
        let path = dir.join(ACTIVE_NAME);
        let mut extra = OpenOptions::new().append(true).open(&path).expect("append");
        extra.write_all(b"TORN").expect("sneak");
        extra.sync_all().expect("sync sneak");
        drop(extra);
        log.record(event("two"));
        let text = fs::read_to_string(&path).expect("read");
        assert!(!text.contains("TORN"));
        let page = log.page(0, 10).expect("page");
        assert_eq!(page.len(), 2);
        assert_eq!(page[0].seq, 1);
        assert_eq!(page[1].seq, 2);
        assert_eq!(page[1].detail, "two");
        let _ = fs::remove_dir_all(&dir);
    }
}
