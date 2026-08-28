//! Optional upload-by-A / activate-by-B helper.

use crate::principal::ANONYMOUS_ID;

/// Whether `activator` may activate a package uploaded by `uploader`.
///
/// When `enabled` is false, always true. When true, ids must be non-empty,
/// not [`ANONYMOUS_ID`], and distinct.
#[must_use]
pub fn dual_control_allowed(enabled: bool, uploader: &str, activator: &str) -> bool {
    if !enabled {
        return true;
    }
    let a = uploader.trim();
    let b = activator.trim();
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a == ANONYMOUS_ID || b == ANONYMOUS_ID {
        return false;
    }
    a != b
}
