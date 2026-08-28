//! Device configuration schema (versioned).

use serde::{Deserialize, Serialize};

/// Current on-disk schema version. Forward-only: refuse unknown majors.
pub const CONFIG_SCHEMA_VERSION: u32 = 1;

/// Root device configuration document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceConfig {
    /// Schema version (must equal [`CONFIG_SCHEMA_VERSION`] for this crate).
    pub version: u32,
    /// Runtime profile (`dev` allows insecure local defaults; `prod` is strict).
    #[serde(default)]
    pub profile: ProfileKind,
    /// Device identity used in status and Sparkplug edge node id.
    pub device: DeviceIdentity,
    /// Cooperative task table.
    pub scan: ScanConfig,
    /// MQTT / Sparkplug telemetry settings.
    #[serde(default)]
    pub telemetry: TelemetryConfig,
    /// On-disk paths for programs, retain, audit.
    #[serde(default)]
    pub paths: PathsConfig,
    /// Operational limits (uploads, rates).
    #[serde(default)]
    pub limits: LimitsConfig,
    /// Program package policy.
    #[serde(default)]
    pub program: ProgramConfig,
    /// Authn/authz configuration (roles, principals, dual control).
    #[serde(default)]
    pub auth: AuthConfig,
    /// I/O subsystem flags (map details live in io-map YAML — PR-03).
    #[serde(default)]
    pub io: IoConfig,
    /// Watchdog configuration.
    #[serde(default)]
    pub watchdog: WatchdogConfig,
    /// Output behavior when `mode=STOP`.
    #[serde(default)]
    pub stop_output_policy: StopOutputPolicy,
}

/// Deployment profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ProfileKind {
    /// Local / CI: may allow unsigned packages and plain HTTP later.
    #[default]
    Dev,
    /// Production: signature required, secure defaults.
    Prod,
}

/// Device identity block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceIdentity {
    /// Stable device id (also default Sparkplug `edge_node_id`).
    pub id: String,
    /// Human-readable site / line name.
    #[serde(default)]
    pub name: String,
    /// Optional site code for ops tooling.
    #[serde(default)]
    pub site: String,
}

/// Scan / task configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScanConfig {
    /// Ordered task definitions (priority is schedule order; rate-monotonic expected).
    pub tasks: Vec<TaskConfig>,
    /// Consecutive logic overruns before FAULT (architecture default: 2).
    #[serde(default = "default_overrun_limit")]
    pub overrun_limit: u32,
    /// Optional CPU affinity for the RT scan thread.
    #[serde(default)]
    pub cpu_affinity: Option<usize>,
}

fn default_overrun_limit() -> u32 {
    2
}

/// One cooperative cyclic task.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskConfig {
    /// Task name (e.g. `fast`, `main`, `slow`).
    pub name: String,
    /// Period in milliseconds.
    pub period_ms: u32,
    /// IR entry symbol (e.g. `task.main`).
    pub entry: String,
    /// Schedule priority (higher runs first when multiple tasks are due).
    #[serde(default)]
    pub priority: u8,
}

/// Telemetry / MQTT Sparkplug settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TelemetryConfig {
    /// Master enable.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Sparkplug `group_id`.
    #[serde(default = "default_group_id")]
    pub group_id: String,
    /// Sparkplug device id under the edge node (optional leaf).
    #[serde(default = "default_telemetry_device_id")]
    pub device_id: String,
    /// MQTT broker URL (e.g. `mqtts://broker:8883`).
    #[serde(default)]
    pub broker_url: String,
    /// Analog re-publish period ms.
    #[serde(default = "default_analog_period_ms")]
    pub analog_period_ms: u32,
    /// Digital change-of-state minimum period ms.
    #[serde(default = "default_digital_cos_ms")]
    pub digital_cos_ms: u32,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            group_id: default_group_id(),
            device_id: default_telemetry_device_id(),
            broker_url: String::new(),
            analog_period_ms: default_analog_period_ms(),
            digital_cos_ms: default_digital_cos_ms(),
        }
    }
}

fn default_true() -> bool {
    true
}
fn default_group_id() -> String {
    "plantA".into()
}
fn default_telemetry_device_id() -> String {
    "line".into()
}
fn default_analog_period_ms() -> u32 {
    500
}
fn default_digital_cos_ms() -> u32 {
    20
}

/// On-disk data paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathsConfig {
    /// Program package store root.
    #[serde(default = "default_programs_path")]
    pub programs: String,
    /// Retain store directory.
    #[serde(default = "default_retain_path")]
    pub retain: String,
    /// Audit log directory.
    #[serde(default = "default_audit_path")]
    pub audit: String,
    /// Optional io-map YAML path.
    #[serde(default = "default_io_map_path")]
    pub io_map: String,
}

impl Default for PathsConfig {
    fn default() -> Self {
        Self {
            programs: default_programs_path(),
            retain: default_retain_path(),
            audit: default_audit_path(),
            io_map: default_io_map_path(),
        }
    }
}

fn default_programs_path() -> String {
    "/var/lib/soft-plc/programs".into()
}
fn default_retain_path() -> String {
    "/var/lib/soft-plc/retain".into()
}
fn default_audit_path() -> String {
    "/var/lib/soft-plc/audit".into()
}
fn default_io_map_path() -> String {
    "/var/lib/soft-plc/io-map.yaml".into()
}

/// Operational limits (architecture frozen defaults).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LimitsConfig {
    /// Max `.spkg` upload size in bytes (default 8 MiB).
    #[serde(default = "default_max_package_bytes")]
    pub max_package_bytes: u64,
    /// Sustained authenticated REST requests per second.
    #[serde(default = "default_rest_rate")]
    pub rest_rate_per_s: u32,
    /// Burst REST requests.
    #[serde(default = "default_rest_burst")]
    pub rest_burst: u32,
    /// Auth failure lockout threshold per IP per minute.
    #[serde(default = "default_auth_fail_per_min")]
    pub auth_fail_per_min: u32,
    /// Max FB instances (verifier / arm guard).
    #[serde(default = "default_max_instances")]
    pub max_instances: u32,
}

impl Default for LimitsConfig {
    fn default() -> Self {
        Self {
            max_package_bytes: default_max_package_bytes(),
            rest_rate_per_s: default_rest_rate(),
            rest_burst: default_rest_burst(),
            auth_fail_per_min: default_auth_fail_per_min(),
            max_instances: default_max_instances(),
        }
    }
}

fn default_max_package_bytes() -> u64 {
    8 * 1024 * 1024
}
fn default_rest_rate() -> u32 {
    30
}
fn default_rest_burst() -> u32 {
    60
}
fn default_auth_fail_per_min() -> u32 {
    5
}
fn default_max_instances() -> u32 {
    4096
}

/// Program package policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgramConfig {
    /// Reject unsigned packages when true (prod default true).
    #[serde(default = "default_require_signature")]
    pub require_signature: bool,
}

impl Default for ProgramConfig {
    fn default() -> Self {
        Self {
            require_signature: default_require_signature(),
        }
    }
}

fn default_require_signature() -> bool {
    // Dev-friendly default in schema; prod profile validation can force true later (PR-20).
    false
}

/// Allowed principal role names in device YAML/JSON (lowercase).
pub const AUTH_ROLES: &[&str] = &["viewer", "operator", "engineer", "admin"];

/// Authn/authz configuration. TLS paths are consumed by the REST listener;
/// this crate only stores them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthConfig {
    /// Require authentication for privileged REST.
    #[serde(default)]
    pub required: bool,
    /// Path to TLS cert (optional).
    #[serde(default)]
    pub tls_cert_path: String,
    /// Path to TLS key (optional).
    #[serde(default)]
    pub tls_key_path: String,
    /// Path to mTLS client CA (optional).
    #[serde(default)]
    pub client_ca_path: String,
    /// When true, program activate must be a different principal than the
    /// uploader (upload by A, activate by B).
    #[serde(default)]
    pub dual_control: bool,
    /// Lockout duration in seconds after the failure threshold is hit.
    #[serde(default = "default_lockout_secs")]
    pub lockout_secs: u32,
    /// Pre-provisioned principals (bearer token and/or client-cert hashes).
    #[serde(default)]
    pub principals: Vec<PrincipalConfig>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            required: false,
            tls_cert_path: String::new(),
            tls_key_path: String::new(),
            client_ca_path: String::new(),
            dual_control: false,
            lockout_secs: default_lockout_secs(),
            principals: Vec::new(),
        }
    }
}

fn default_lockout_secs() -> u32 {
    60
}

/// One locally configured principal.
///
/// Secrets are never stored: `token_sha256` is SHA-256 of the bearer secret;
/// `cert_sha256` is SHA-256 of the raw client certificate DER. At least one
/// identity hash is required.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrincipalConfig {
    /// Stable principal id (unique within `auth.principals`).
    pub id: String,
    /// Role name: `viewer`, `operator`, `engineer`, or `admin`.
    pub role: String,
    /// Lowercase hex SHA-256 of the bearer token secret.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_sha256: Option<String>,
    /// Lowercase hex SHA-256 of the client certificate DER.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cert_sha256: Option<String>,
}

/// I/O subsystem flags (driver list / fault policy). Full bindings are in io-map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IoConfig {
    /// Enter FAULT when any module is Bad (default false: degrade only).
    #[serde(default)]
    pub fault_on_module_bad: bool,
    /// Enabled driver kinds: `sim`, `gpio`, `modbus_tcp`.
    #[serde(default = "default_io_drivers")]
    pub drivers: Vec<String>,
}

impl Default for IoConfig {
    fn default() -> Self {
        Self {
            fault_on_module_bad: false,
            drivers: default_io_drivers(),
        }
    }
}

fn default_io_drivers() -> Vec<String> {
    vec!["sim".into()]
}

/// Hardware / software watchdog settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchdogConfig {
    /// Open and stroke `/dev/watchdog` when true.
    #[serde(default)]
    pub hardware_enabled: bool,
    /// Device path for hardware watchdog.
    #[serde(default = "default_hw_wd_path")]
    pub hardware_path: String,
}

impl Default for WatchdogConfig {
    fn default() -> Self {
        Self {
            hardware_enabled: false,
            hardware_path: default_hw_wd_path(),
        }
    }
}

fn default_hw_wd_path() -> String {
    "/dev/watchdog".into()
}

/// Output policy when operating mode is STOP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StopOutputPolicy {
    /// Drive outputs to configured `safe_state` (architecture default).
    #[default]
    Safe,
    /// Hold last program outputs.
    Hold,
}
