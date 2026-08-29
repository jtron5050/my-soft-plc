//! JSON DTOs matching `docs/openapi/openapi.yaml`.

use plc_io::PlcValue;
use plc_package::{RestartPolicy, TagKind};
use plc_runtime::{ArmReport, ProgramInfo, TagView};
use plc_types::{OperatingMode, ProgramPhase, Quality};
use serde::{Deserialize, Serialize};

/// `GET /health`.
#[derive(Debug, Serialize)]
pub struct HealthBody {
    /// Always `ok` if the process is serving.
    pub status: &'static str,
}

/// `POST /mode`.
#[derive(Debug, Deserialize)]
pub struct ModeBody {
    /// `RUN` / `STOP` / `FAULT_RESET` / `SIM`.
    pub mode: String,
}

/// Mode change response.
#[derive(Debug, Serialize)]
pub struct ModeResponse {
    /// Requested token.
    pub requested: String,
    /// Last observed engine mode.
    pub mode: String,
}

/// Activate accepted.
#[derive(Debug, Serialize)]
pub struct ActivateAccepted {
    /// Client correlator; poll `GET /status`.
    pub job_id: String,
    /// Always `pending` on 202.
    pub status: &'static str,
}

/// Activate no-op.
#[derive(Debug, Serialize)]
pub struct ActivateNoOp {
    /// `idle`.
    pub status: &'static str,
    /// Current program id.
    pub program_id: String,
}

/// `GET /status` document.
#[derive(Debug, Serialize)]
pub struct StatusBody {
    /// Operator mode.
    pub mode: String,
    /// Program identity + phase.
    pub program: ProgramStatusBody,
    /// Scan timing.
    pub scan: ScanBody,
    /// Watchdog: `ok` or `fault`.
    pub watchdog: &'static str,
    /// I/O summary.
    pub io: IoStatusBody,
    /// Process uptime.
    pub uptime_s: u64,
}

/// `program` object.
#[derive(Debug, Serialize)]
pub struct ProgramStatusBody {
    /// Epoch phase.
    pub phase: String,
    /// Running package.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<ProgramCurrentBody>,
    /// Armed package.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub armed: Option<ProgramArmedBody>,
}

/// Current program (architecture illustration).
#[derive(Debug, Serialize)]
pub struct ProgramCurrentBody {
    /// Id.
    pub id: String,
    /// Version.
    pub version: String,
    /// Build id.
    pub build_id: String,
    /// Compatibility hash.
    pub compatibility_hash: String,
    /// Non-zero signature present at arm.
    pub signed: bool,
}

/// Armed program.
#[derive(Debug, Serialize)]
pub struct ProgramArmedBody {
    /// Id.
    pub id: String,
    /// Version.
    pub version: String,
    /// Build id.
    pub build_id: String,
    /// Compatibility hash.
    pub compatibility_hash: String,
    /// Manifest restart policy.
    pub restart_policy: String,
    /// Bumpless honored at arm.
    pub bumpless_eligible: bool,
}

/// Scan timing wrapper.
#[derive(Debug, Serialize)]
pub struct ScanBody {
    /// Per-task stats.
    pub tasks: Vec<TaskTimingBody>,
}

/// One task.
#[derive(Debug, Serialize)]
pub struct TaskTimingBody {
    /// Task name.
    pub name: String,
    /// Period ms.
    pub period_ms: u32,
    /// Last invocation µs.
    pub last_us: u64,
    /// Max invocation µs.
    pub max_us: u64,
    /// Lifetime overruns.
    pub overruns: u32,
}

/// I/O summary.
#[derive(Debug, Serialize)]
pub struct IoStatusBody {
    /// Any module Bad.
    pub degraded: bool,
    /// Module ids with Bad quality (empty until drivers expose ids).
    pub modules_bad: Vec<String>,
}

/// Retain remap counts.
#[derive(Debug, Serialize)]
pub struct RetainReportBody {
    /// Kept symbols.
    pub kept: u32,
    /// Cold-defaulted.
    pub cold_defaults: u32,
    /// Dropped.
    pub dropped: u32,
    /// Zeroed incompatible names.
    pub zeroed_incompat: Vec<String>,
}

/// Stored / armed program GET.
#[derive(Debug, Serialize)]
pub struct ProgramDetailBody {
    /// Metadata.
    pub id: String,
    /// Version.
    pub version: String,
    /// Build id.
    pub build_id: String,
    /// Hash.
    pub compatibility_hash: String,
    /// Bytes on disk.
    pub size: u64,
    /// Uploader.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uploader: Option<String>,
    /// Retain report when this id is armed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retain: Option<RetainReportBody>,
}

/// Tag dictionary entry.
#[derive(Debug, Serialize)]
pub struct TagDictEntry {
    /// Name.
    pub name: String,
    /// IEC type.
    #[serde(rename = "type")]
    pub ty: String,
    /// Kind.
    pub kind: String,
    /// Slot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot: Option<u32>,
}

/// Tag list.
#[derive(Debug, Serialize)]
pub struct TagListBody {
    /// Entries.
    pub tags: Vec<TagDictEntry>,
}

/// Debug tag read.
#[derive(Debug, Serialize)]
pub struct TagReadBody {
    /// Name.
    pub name: String,
    /// IEC type.
    #[serde(rename = "type")]
    pub ty: String,
    /// Kind.
    pub kind: String,
    /// JSON value.
    pub value: serde_json::Value,
    /// Quality name.
    pub quality: String,
    /// Force overlay.
    pub forced: bool,
}

/// Force write.
#[derive(Debug, Deserialize)]
pub struct TagWriteBody {
    /// Forced value (bool or number).
    pub value: serde_json::Value,
}

/// Force result.
#[derive(Debug, Serialize)]
pub struct TagWriteResponse {
    /// Overlay is active.
    pub forced: bool,
}

/// Config write result.
#[derive(Debug, Serialize)]
pub struct ConfigWriteResponse {
    /// Whether scan/io changes need a process restart.
    pub restart_required: bool,
}

/// Activate query.
#[derive(Debug, Deserialize)]
pub struct ActivateQuery {
    /// Optional block timeout.
    pub wait_ms: Option<u64>,
}

/// Audit / events page query.
#[derive(Debug, Deserialize)]
pub struct PageQuery {
    /// Max items (default 100, max 1000).
    pub limit: Option<u32>,
    /// Return items with `seq` greater than this (events) or index (audit).
    pub cursor: Option<u64>,
}

impl From<&ProgramInfo> for ProgramCurrentBody {
    fn from(p: &ProgramInfo) -> Self {
        Self {
            id: p.id.clone(),
            version: p.version.clone(),
            build_id: p.build_id.clone(),
            compatibility_hash: p.compatibility_hash.clone(),
            signed: p.signed,
        }
    }
}

impl From<&ProgramInfo> for ProgramArmedBody {
    fn from(p: &ProgramInfo) -> Self {
        Self {
            id: p.id.clone(),
            version: p.version.clone(),
            build_id: p.build_id.clone(),
            compatibility_hash: p.compatibility_hash.clone(),
            restart_policy: restart_wire(p.restart_policy),
            bumpless_eligible: p.bumpless_eligible,
        }
    }
}

/// Wire name for restart policy.
#[must_use]
pub fn restart_wire(p: RestartPolicy) -> String {
    match p {
        RestartPolicy::SafeReset => "safe_reset".into(),
        RestartPolicy::Bumpless => "bumpless".into(),
    }
}

/// Wire mode.
#[must_use]
pub fn mode_wire(m: OperatingMode) -> String {
    m.as_str().to_string()
}

/// Wire phase.
#[must_use]
pub fn phase_wire(p: ProgramPhase) -> String {
    p.as_str().to_string()
}

/// Quality name.
#[must_use]
pub fn quality_wire(q: Quality) -> String {
    match q {
        Quality::Good => "Good".into(),
        Quality::Uncertain => "Uncertain".into(),
        Quality::Bad => "Bad".into(),
    }
}

/// Tag kind wire.
#[must_use]
pub fn kind_wire(k: TagKind) -> String {
    match k {
        TagKind::I => "I".into(),
        TagKind::Q => "Q".into(),
        TagKind::M => "M".into(),
        TagKind::R => "R".into(),
        TagKind::Internal => "INTERNAL".into(),
    }
}

/// `PlcValue` as JSON.
#[must_use]
pub fn value_json(v: PlcValue) -> serde_json::Value {
    match v {
        PlcValue::Bool(b) => serde_json::Value::Bool(b),
        PlcValue::Int(n) => serde_json::json!(n),
        PlcValue::Dint(n) | PlcValue::Time(n) => serde_json::json!(n),
        PlcValue::Real(n) => serde_json::json!(n),
    }
}

/// Parse a JSON force value into a [`PlcValue`] using the tag type name.
pub fn parse_force_value(ty: &str, v: &serde_json::Value) -> Result<PlcValue, String> {
    match ty {
        "BOOL" => v
            .as_bool()
            .map(PlcValue::Bool)
            .ok_or_else(|| "expected boolean".into()),
        "INT" => v
            .as_i64()
            .and_then(|n| i16::try_from(n).ok())
            .map(PlcValue::Int)
            .ok_or_else(|| "expected INT".into()),
        "DINT" => v
            .as_i64()
            .and_then(|n| i32::try_from(n).ok())
            .map(PlcValue::Dint)
            .ok_or_else(|| "expected DINT".into()),
        "TIME" => v
            .as_i64()
            .and_then(|n| i32::try_from(n).ok())
            .map(PlcValue::Time)
            .ok_or_else(|| "expected TIME ms".into()),
        "REAL" => v
            .as_f64()
            .map(|n| PlcValue::Real(n as f32))
            .ok_or_else(|| "expected REAL".into()),
        other => Err(format!("cannot force type {other}")),
    }
}

impl From<&TagView> for TagReadBody {
    fn from(t: &TagView) -> Self {
        Self {
            name: t.name.clone(),
            ty: t.type_name().to_string(),
            kind: kind_wire(t.kind),
            value: value_json(t.value),
            quality: quality_wire(t.quality),
            forced: t.forced,
        }
    }
}

impl From<&ArmReport> for RetainReportBody {
    fn from(r: &ArmReport) -> Self {
        Self {
            kept: r.retain.kept,
            cold_defaults: r.retain.cold_defaults,
            dropped: r.retain.dropped,
            zeroed_incompat: r.retain.zeroed_incompat.clone(),
        }
    }
}
