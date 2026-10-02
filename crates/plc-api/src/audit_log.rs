//! Append-only audit log. Rotates at 16 MiB and keeps 8 files.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use plc_auth::{AuditAction, AuditEvent, AuditSink};
use serde::{Deserialize, Serialize};

/// Active file size that triggers rotation.
pub const AUDIT_MAX_BYTES: u64 = 16 * 1024 * 1024;
/// Active file plus rotated siblings.
pub const AUDIT_MAX_FILES: usize = 8;

const ACTIVE_NAME: &str = "audit.jsonl";

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
        let _guard = self.inner.lock().expect("audit log");
        let mut out = Vec::new();
        if limit == 0 {
            return Ok(out);
        }
        for path in files_oldest_first(&self.dir, self.max_files) {
            let text = match fs::read_to_string(&path) {
                Ok(text) => text,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e),
            };
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
        let file = inner
            .file
            .as_mut()
            .ok_or_else(|| io::Error::other("audit file closed"))?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        inner.len = inner.len.saturating_add(add);
        inner.next_seq = inner.next_seq.saturating_add(1);
        Ok(())
    }

    fn rotate(&self, inner: &mut ActiveFile) -> io::Result<()> {
        if let Some(file) = inner.file.take() {
            file.sync_all()?;
        }
        let oldest = self.max_files - 1;
        let _ = fs::remove_file(self.dir.join(rotated_name(oldest)));
        for i in (1..oldest).rev() {
            let from = self.dir.join(rotated_name(i));
            if from.exists() {
                fs::rename(&from, self.dir.join(rotated_name(i + 1)))?;
            }
        }
        let active = self.dir.join(ACTIVE_NAME);
        if active.exists() {
            fs::rename(&active, self.dir.join(rotated_name(1)))?;
        }
        let file = OpenOptions::new().create(true).append(true).open(&active)?;
        inner.file = Some(file);
        inner.len = 0;
        sync_dir(&self.dir)?;
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

/// Drop a trailing partial line so the next append stays valid JSONL.
fn repair_torn_tail(path: &Path) -> io::Result<u64> {
    if !path.exists() {
        return Ok(0);
    }
    let mut data = fs::read(path)?;
    if data.is_empty() || data.last() == Some(&b'\n') {
        return Ok(data.len() as u64);
    }
    let keep = data
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |i| i + 1);
    data.truncate(keep);
    fs::write(path, &data)?;
    Ok(keep as u64)
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

    #[test]
    fn rotates_keeps_eight_and_reopens_in_seq_order() {
        let dir = std::env::temp_dir().join(format!(
            "plc-audit-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
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
}
