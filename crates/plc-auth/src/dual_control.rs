//! Optional upload-by-A / activate-by-B helper.

use crate::principal::ANONYMOUS_ID;

/// Whether activator may complete an activate started by `armed_by`.
///
/// When `enabled` is false, always true. When true, ids must be non-empty,
/// not [`ANONYMOUS_ID`], and distinct.
#[must_use]
pub fn dual_control_allowed(enabled: bool, armed_by: &str, activator: &str) -> bool {
    if !enabled {
        return true;
    }
    let a = armed_by.trim();
    let b = activator.trim();
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a == ANONYMOUS_ID || b == ANONYMOUS_ID {
        return false;
    }
    a != b
}
