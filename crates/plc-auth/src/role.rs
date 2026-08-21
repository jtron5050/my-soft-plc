//! Roles and the permission matrix (PR-12 contract).

use core::fmt;

/// Inclusive role hierarchy: `admin > engineer > operator > viewer`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// Read-only status, config, programs, tags, metrics, diagnostics, audit.
    Viewer,
    /// Viewer plus mode changes and tag force.
    Operator,
    /// Operator plus config writes and program load/arm/activate/delete.
    Engineer,
    /// Engineer plus keys and user administration.
    Admin,
}

impl Role {
    /// Parse a config role name (`viewer` / `operator` / `engineer` / `admin`).
    pub fn parse(s: &str) -> Result<Self, crate::AuthError> {
        match s.trim().to_ascii_lowercase().as_str() {
            "viewer" => Ok(Self::Viewer),
            "operator" => Ok(Self::Operator),
            "engineer" => Ok(Self::Engineer),
            "admin" => Ok(Self::Admin),
            other => Err(crate::AuthError::Config(format!("unknown role '{other}'"))),
        }
    }

    /// Canonical lowercase name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Operator => "operator",
            Self::Engineer => "engineer",
            Self::Admin => "admin",
        }
    }

    /// Numeric rank used for inclusive hierarchy checks.
    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            Self::Viewer => 0,
            Self::Operator => 1,
            Self::Engineer => 2,
            Self::Admin => 3,
        }
    }

    /// Whether this role grants `perm`.
    #[must_use]
    pub const fn allows(self, perm: Permission) -> bool {
        self.rank() >= perm.min_role().rank()
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Management-plane permissions matching the REST resource sketch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Permission {
    /// `GET /health`, `/status`, `/status/tasks`, `/status/io`.
    StatusRead,
    /// `GET /config`.
    ConfigRead,
    /// `PUT` / `PATCH /config`.
    ConfigWrite,
    /// `GET /programs`, `GET /programs/{id}`.
    ProgramRead,
    /// `POST /programs`.
    ProgramUpload,
    /// `POST /programs/{id}/arm`.
    ProgramArm,
    /// `POST /programs/{id}/activate`.
    ProgramActivate,
    /// `DELETE /programs/{id}`.
    ProgramDelete,
    /// `POST /mode`.
    ModeWrite,
    /// `GET /tags`, `GET /tags/{name}`.
    TagRead,
    /// `PUT /tags/{name}` force write.
    TagForce,
    /// `GET /metrics`.
    MetricsRead,
    /// `GET /diagnostics/events`.
    DiagnosticsRead,
    /// `GET /diagnostics/audit`.
    AuditRead,
    /// Keys and user administration.
    UserAdmin,
}

impl Permission {
    /// Canonical snake_case name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StatusRead => "status_read",
            Self::ConfigRead => "config_read",
            Self::ConfigWrite => "config_write",
            Self::ProgramRead => "program_read",
            Self::ProgramUpload => "program_upload",
            Self::ProgramArm => "program_arm",
            Self::ProgramActivate => "program_activate",
            Self::ProgramDelete => "program_delete",
            Self::ModeWrite => "mode_write",
            Self::TagRead => "tag_read",
            Self::TagForce => "tag_force",
            Self::MetricsRead => "metrics_read",
            Self::DiagnosticsRead => "diagnostics_read",
            Self::AuditRead => "audit_read",
            Self::UserAdmin => "user_admin",
        }
    }

    /// Least role that grants this permission.
    #[must_use]
    pub const fn min_role(self) -> Role {
        match self {
            Self::StatusRead
            | Self::ConfigRead
            | Self::ProgramRead
            | Self::TagRead
            | Self::MetricsRead
            | Self::DiagnosticsRead
            | Self::AuditRead => Role::Viewer,
            Self::ModeWrite | Self::TagForce => Role::Operator,
            Self::ConfigWrite
            | Self::ProgramUpload
            | Self::ProgramArm
            | Self::ProgramActivate
            | Self::ProgramDelete => Role::Engineer,
            Self::UserAdmin => Role::Admin,
        }
    }

    /// Every permission (for matrix tests and OpenAPI mapping).
    pub const ALL: [Self; 15] = [
        Self::StatusRead,
        Self::ConfigRead,
        Self::ConfigWrite,
        Self::ProgramRead,
        Self::ProgramUpload,
        Self::ProgramArm,
        Self::ProgramActivate,
        Self::ProgramDelete,
        Self::ModeWrite,
        Self::TagRead,
        Self::TagForce,
        Self::MetricsRead,
        Self::DiagnosticsRead,
        Self::AuditRead,
        Self::UserAdmin,
    ];
}

impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
