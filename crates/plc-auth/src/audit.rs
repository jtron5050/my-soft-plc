//! Audit event types and an in-memory sink. Persistence and rotation live elsewhere.

use std::collections::VecDeque;
use std::net::IpAddr;
use std::sync::Mutex;

/// Diagnostics/audit ring capacity (matches architecture event ring size).
pub const AUDIT_CAP: usize = 4096;

/// Privileged / auth actions recorded for later export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditAction {
    /// Successful authenticate.
    AuthSuccess,
    /// Failed authenticate (bad or missing credentials).
    AuthFailure,
    /// Source IP lockout engaged or hit.
    AuthLocked,
    /// `POST /mode`.
    ModeChange,
    /// Program arm.
    ProgramArm,
    /// Program activate.
    ProgramActivate,
    /// Config write.
    ConfigWrite,
    /// Tag force overlay.
    TagForce,
    /// Keys / users.
    UserAdmin,
}

/// One audit record. `plc-auth` does not persist these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEvent {
    /// Wall-clock unix seconds (telemetry timebase; not TON/TOF).
    pub unix_secs: u64,
    /// Principal id (`anonymous` if unauthenticated).
    pub principal_id: String,
    /// Action kind.
    pub action: AuditAction,
    /// Free-form detail (mode name, program id, tag name).
    pub detail: String,
    /// Optional client address.
    pub client_ip: Option<IpAddr>,
}

/// Non-RT audit destination.
pub trait AuditSink: Send + Sync {
    /// Append one event.
    fn record(&self, event: AuditEvent);
}

/// In-memory ring for tests and as a stand-in before file rotation.
#[derive(Debug)]
pub struct MemoryAudit {
    cap: usize,
    events: Mutex<VecDeque<AuditEvent>>,
}

impl MemoryAudit {
    /// Ring of [`AUDIT_CAP`] events.
    #[must_use]
    pub fn new() -> Self {
        Self::with_cap(AUDIT_CAP)
    }

    /// Ring of `cap` events (overwrite oldest).
    #[must_use]
    pub fn with_cap(cap: usize) -> Self {
        Self {
            cap: cap.max(1),
            events: Mutex::new(VecDeque::new()),
        }
    }

    /// Snapshot of stored events (oldest first).
    #[must_use]
    pub fn events(&self) -> Vec<AuditEvent> {
        self.events
            .lock()
            .expect("memory audit")
            .iter()
            .cloned()
            .collect()
    }
}

impl Default for MemoryAudit {
    fn default() -> Self {
        Self::new()
    }
}

impl AuditSink for MemoryAudit {
    fn record(&self, event: AuditEvent) {
        let mut q = self.events.lock().expect("memory audit");
        if q.len() >= self.cap {
            q.pop_front();
        }
        q.push_back(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_overwrites_oldest() {
        let sink = MemoryAudit::with_cap(2);
        for i in 0..3 {
            sink.record(AuditEvent {
                unix_secs: i,
                principal_id: "eng".into(),
                action: AuditAction::AuthSuccess,
                detail: i.to_string(),
                client_ip: None,
            });
        }
        let ev = sink.events();
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0].unix_secs, 1);
        assert_eq!(ev[1].unix_secs, 2);
    }
}
