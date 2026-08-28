//! Permission matrix: every role × every permission.

use plc_auth::{Permission, Role};

const ROLES: [Role; 4] = [Role::Viewer, Role::Operator, Role::Engineer, Role::Admin];

#[test]
fn matrix_matches_architecture() {
    for role in ROLES {
        for perm in Permission::ALL {
            let allowed = role.allows(perm);
            let expected = role.rank() >= perm.min_role().rank();
            assert_eq!(
                allowed, expected,
                "{role} allows {perm} = {allowed}, expected {expected}"
            );
        }
    }
}

#[test]
fn viewer_is_read_only() {
    assert!(Role::Viewer.allows(Permission::StatusRead));
    assert!(Role::Viewer.allows(Permission::ConfigRead));
    assert!(Role::Viewer.allows(Permission::ProgramRead));
    assert!(Role::Viewer.allows(Permission::TagRead));
    assert!(Role::Viewer.allows(Permission::MetricsRead));
    assert!(Role::Viewer.allows(Permission::DiagnosticsRead));
    assert!(Role::Viewer.allows(Permission::AuditRead));
    assert!(!Role::Viewer.allows(Permission::ModeWrite));
    assert!(!Role::Viewer.allows(Permission::TagForce));
    assert!(!Role::Viewer.allows(Permission::ConfigWrite));
    assert!(!Role::Viewer.allows(Permission::ProgramActivate));
    assert!(!Role::Viewer.allows(Permission::UserAdmin));
}

#[test]
fn operator_mode_and_force_not_activate() {
    assert!(Role::Operator.allows(Permission::ModeWrite));
    assert!(Role::Operator.allows(Permission::TagForce));
    assert!(!Role::Operator.allows(Permission::ProgramActivate));
    assert!(!Role::Operator.allows(Permission::ConfigWrite));
    assert!(!Role::Operator.allows(Permission::UserAdmin));
}

#[test]
fn engineer_activate_not_user_admin() {
    assert!(Role::Engineer.allows(Permission::ProgramUpload));
    assert!(Role::Engineer.allows(Permission::ProgramArm));
    assert!(Role::Engineer.allows(Permission::ProgramActivate));
    assert!(Role::Engineer.allows(Permission::ProgramDelete));
    assert!(Role::Engineer.allows(Permission::ConfigWrite));
    assert!(!Role::Engineer.allows(Permission::UserAdmin));
}

#[test]
fn admin_has_everything() {
    for perm in Permission::ALL {
        assert!(Role::Admin.allows(perm), "admin missing {perm}");
    }
}
