use std::collections::BTreeMap;

use de_app::{
    ConfigValue, PaneLayout, SavedSessionId, SavedSessionMetadata, SessionKind, SplitOrientation,
    TabId, TabMetadata, WorkspaceError, WorkspaceId, WorkspaceMetadata, WorkspaceSnapshot,
};
use de_terminal_core::TerminalSize;

fn sample_snapshot() -> WorkspaceSnapshot {
    let first_session = SavedSessionId::new(101).expect("non-zero session ID");
    let second_session = SavedSessionId::new(102).expect("non-zero session ID");
    let tab_id = TabId::new(11).expect("non-zero tab ID");

    let layout = PaneLayout::Split {
        orientation: SplitOrientation::Vertical,
        ratio_per_mille: 500,
        first: Box::new(PaneLayout::Session {
            session_id: first_session,
        }),
        second: Box::new(PaneLayout::Session {
            session_id: second_session,
        }),
    };
    let tab = TabMetadata::new(tab_id, "Main", first_session, layout);
    let workspace = WorkspaceMetadata::new(
        WorkspaceId::new(1).expect("non-zero workspace ID"),
        "Development",
        tab_id,
        vec![tab],
    );
    let sessions = vec![
        SavedSessionMetadata::new(
            first_session,
            "Shell 1",
            SessionKind::ExternalProgram,
            TerminalSize::new(120, 40).expect("valid terminal size"),
        ),
        SavedSessionMetadata::new(
            second_session,
            "Shell 2",
            SessionKind::DeShell,
            TerminalSize::new(80, 24).expect("valid terminal size"),
        ),
    ];

    WorkspaceSnapshot::new(
        WorkspaceId::new(1).expect("non-zero workspace ID"),
        vec![workspace],
        sessions,
    )
    .expect("valid workspace snapshot")
}

fn root_table(value: &mut ConfigValue) -> &mut BTreeMap<String, ConfigValue> {
    match value {
        ConfigValue::Table(table) => table,
        _ => panic!("snapshot root must be a table"),
    }
}

fn collect_keys(value: &ConfigValue, keys: &mut Vec<String>) {
    match value {
        ConfigValue::Table(table) => {
            for (key, value) in table {
                keys.push(key.clone());
                collect_keys(value, keys);
            }
        }
        ConfigValue::Array(values) => {
            for value in values {
                collect_keys(value, keys);
            }
        }
        ConfigValue::Null
        | ConfigValue::Boolean(_)
        | ConfigValue::Integer(_)
        | ConfigValue::String(_) => {}
    }
}

#[test]
fn snapshot_round_trips_workspace_tabs_split_layout_and_session_metadata() {
    let snapshot = sample_snapshot();
    let encoded = snapshot.to_value().expect("encode valid snapshot");
    let decoded = WorkspaceSnapshot::from_value(encoded).expect("decode valid snapshot");

    assert_eq!(decoded, snapshot);
    assert_eq!(decoded.active_workspace_id().get(), 1);
    assert_eq!(decoded.workspaces()[0].tabs()[0].title(), "Main");
    assert_eq!(decoded.sessions().len(), 2);
}

#[test]
fn rejects_non_table_root_and_unsupported_schema_version() {
    assert!(matches!(
        WorkspaceSnapshot::from_value(ConfigValue::String("corrupt".to_owned())),
        Err(WorkspaceError::RootMustBeTable)
    ));

    let mut encoded = sample_snapshot().to_value().expect("encode");
    root_table(&mut encoded).insert("schema_version".to_owned(), ConfigValue::Integer(99));
    assert!(matches!(
        WorkspaceSnapshot::from_value(encoded),
        Err(WorkspaceError::UnsupportedVersion {
            found: 99,
            expected: 1
        })
    ));
}

#[test]
fn rejects_missing_fields_wrong_types_and_unknown_keys() {
    let mut missing = sample_snapshot().to_value().expect("encode");
    root_table(&mut missing).remove("sessions");
    assert!(matches!(
        WorkspaceSnapshot::from_value(missing),
        Err(WorkspaceError::MissingField { key, .. }) if key == "sessions"
    ));

    let mut wrong_type = sample_snapshot().to_value().expect("encode");
    root_table(&mut wrong_type).insert("workspaces".to_owned(), ConfigValue::Boolean(true));
    assert!(matches!(
        WorkspaceSnapshot::from_value(wrong_type),
        Err(WorkspaceError::WrongType { key, .. }) if key == "workspaces"
    ));

    let mut unknown = sample_snapshot().to_value().expect("encode");
    root_table(&mut unknown).insert("unexpected".to_owned(), ConfigValue::Null);
    assert!(matches!(
        WorkspaceSnapshot::from_value(unknown),
        Err(WorkspaceError::UnknownKey { key, .. }) if key == "unexpected"
    ));
}

#[test]
fn rejects_duplicate_persistent_session_ids() {
    let mut encoded = sample_snapshot().to_value().expect("encode");
    let root = root_table(&mut encoded);
    let Some(ConfigValue::Array(sessions)) = root.get_mut("sessions") else {
        panic!("sessions must be an array");
    };
    let Some(ConfigValue::Table(second)) = sessions.get_mut(1) else {
        panic!("session entry must be a table");
    };
    second.insert("id".to_owned(), ConfigValue::Integer(101));

    assert!(matches!(
        WorkspaceSnapshot::from_value(encoded),
        Err(WorkspaceError::InvalidValue {
            field: "sessions.id",
            ..
        })
    ));
}

#[test]
fn rejects_dangling_session_references_and_invalid_split_ratios() {
    let mut dangling = sample_snapshot().to_value().expect("encode");
    let root = root_table(&mut dangling);
    let Some(ConfigValue::Array(workspaces)) = root.get_mut("workspaces") else {
        panic!("workspaces must be an array");
    };
    let Some(ConfigValue::Table(workspace)) = workspaces.get_mut(0) else {
        panic!("workspace entry must be a table");
    };
    let Some(ConfigValue::Array(tabs)) = workspace.get_mut("tabs") else {
        panic!("tabs must be an array");
    };
    let Some(ConfigValue::Table(tab)) = tabs.get_mut(0) else {
        panic!("tab entry must be a table");
    };
    tab.insert("active_session_id".to_owned(), ConfigValue::Integer(999));
    let Some(ConfigValue::Table(layout)) = tab.get_mut("layout") else {
        panic!("layout must be a table");
    };
    let Some(ConfigValue::Table(first)) = layout.get_mut("first") else {
        panic!("first pane must be a table");
    };
    first.insert("session_id".to_owned(), ConfigValue::Integer(999));

    assert!(matches!(
        WorkspaceSnapshot::from_value(dangling),
        Err(WorkspaceError::InvalidValue {
            field: "workspaces.tabs.layout",
            ..
        })
    ));

    let mut bad_ratio = sample_snapshot().to_value().expect("encode");
    let root = root_table(&mut bad_ratio);
    let Some(ConfigValue::Array(workspaces)) = root.get_mut("workspaces") else {
        panic!("workspaces must be an array");
    };
    let Some(ConfigValue::Table(workspace)) = workspaces.get_mut(0) else {
        panic!("workspace entry must be a table");
    };
    let Some(ConfigValue::Array(tabs)) = workspace.get_mut("tabs") else {
        panic!("tabs must be an array");
    };
    let Some(ConfigValue::Table(tab)) = tabs.get_mut(0) else {
        panic!("tab entry must be a table");
    };
    let Some(ConfigValue::Table(layout)) = tab.get_mut("layout") else {
        panic!("layout must be a table");
    };
    layout.insert("ratio_per_mille".to_owned(), ConfigValue::Integer(999));

    assert!(matches!(
        WorkspaceSnapshot::from_value(bad_ratio),
        Err(WorkspaceError::InvalidValue {
            field: "workspaces.tabs.layout.ratio_per_mille",
            ..
        })
    ));
}

#[test]
fn rejects_pane_trees_that_exceed_the_maximum_depth() {
    let session_id = SavedSessionId::new(101).expect("non-zero session ID");
    let mut layout = PaneLayout::Session { session_id };
    for _ in 0..9 {
        layout = PaneLayout::Split {
            orientation: SplitOrientation::Horizontal,
            ratio_per_mille: 500,
            first: Box::new(layout),
            second: Box::new(PaneLayout::Session { session_id }),
        };
    }

    let tab_id = TabId::new(11).expect("non-zero tab ID");
    let workspace_id = WorkspaceId::new(1).expect("non-zero workspace ID");
    let result = WorkspaceSnapshot::new(
        workspace_id,
        vec![WorkspaceMetadata::new(
            workspace_id,
            "Development",
            tab_id,
            vec![TabMetadata::new(tab_id, "Main", session_id, layout)],
        )],
        vec![SavedSessionMetadata::new(
            session_id,
            "Shell",
            SessionKind::ExternalProgram,
            TerminalSize::new(80, 24).expect("valid terminal size"),
        )],
    );

    assert!(matches!(
        result,
        Err(WorkspaceError::InvalidValue {
            field: "workspaces.tabs.layout",
            ..
        })
    ));
}

#[test]
fn serialized_snapshot_contains_no_live_process_state_or_launch_specification() {
    let encoded = sample_snapshot().to_value().expect("encode");
    let mut keys = Vec::new();
    collect_keys(&encoded, &mut keys);

    for forbidden in [
        "program",
        "executable",
        "args",
        "arguments",
        "environment",
        "pid",
        "process_id",
        "process_group",
        "pty",
        "pty_fd",
        "pending_input",
        "exit_status",
    ] {
        assert!(
            !keys.iter().any(|key| key == forbidden),
            "snapshot must not serialize process field {forbidden}"
        );
    }

    let restored = WorkspaceSnapshot::from_value(encoded).expect("decode metadata");
    assert_eq!(restored.sessions().len(), 2);
    // The decoded value is presentation metadata only. It has no runtime
    // service or process-spawn operation through which a child could be relaunched.
    assert_eq!(restored.sessions()[0].title(), "Shell 1");
}

#[test]
fn rejects_titles_with_control_characters_and_duplicate_tab_ids() {
    let session_id = SavedSessionId::new(101).expect("non-zero session ID");
    let tab_id = TabId::new(11).expect("non-zero tab ID");
    let workspace_id = WorkspaceId::new(1).expect("non-zero workspace ID");
    let session = SavedSessionMetadata::new(
        session_id,
        "Shell",
        SessionKind::ExternalProgram,
        TerminalSize::new(80, 24).expect("valid terminal size"),
    );

    let invalid_title = WorkspaceSnapshot::new(
        workspace_id,
        vec![WorkspaceMetadata::new(
            workspace_id,
            "Bad\nName",
            tab_id,
            vec![TabMetadata::new(
                tab_id,
                "Main",
                session_id,
                PaneLayout::Session { session_id },
            )],
        )],
        vec![session.clone()],
    );
    assert!(matches!(
        invalid_title,
        Err(WorkspaceError::InvalidValue {
            field: "workspaces.name",
            ..
        })
    ));

    let duplicate_tabs = WorkspaceSnapshot::new(
        workspace_id,
        vec![WorkspaceMetadata::new(
            workspace_id,
            "Development",
            tab_id,
            vec![
                TabMetadata::new(
                    tab_id,
                    "Main",
                    session_id,
                    PaneLayout::Session { session_id },
                ),
                TabMetadata::new(
                    tab_id,
                    "Duplicate",
                    session_id,
                    PaneLayout::Session { session_id },
                ),
            ],
        )],
        vec![session],
    );
    assert!(matches!(
        duplicate_tabs,
        Err(WorkspaceError::InvalidValue {
            field: "workspaces.tabs.id",
            ..
        })
    ));
}

#[test]
fn rejects_orphaned_saved_session_metadata() {
    let session_id = SavedSessionId::new(101).expect("non-zero session ID");
    let orphan_id = SavedSessionId::new(102).expect("non-zero session ID");
    let tab_id = TabId::new(11).expect("non-zero tab ID");
    let workspace_id = WorkspaceId::new(1).expect("non-zero workspace ID");
    let size = TerminalSize::new(80, 24).expect("valid terminal size");

    let result = WorkspaceSnapshot::new(
        workspace_id,
        vec![WorkspaceMetadata::new(
            workspace_id,
            "Development",
            tab_id,
            vec![TabMetadata::new(
                tab_id,
                "Main",
                session_id,
                PaneLayout::Session { session_id },
            )],
        )],
        vec![
            SavedSessionMetadata::new(session_id, "Shell 1", SessionKind::ExternalProgram, size),
            SavedSessionMetadata::new(orphan_id, "Shell 2", SessionKind::DeShell, size),
        ],
    );

    assert!(matches!(
        result,
        Err(WorkspaceError::InvalidValue {
            field: "sessions",
            ..
        })
    ));
}

#[test]
fn format_neutral_schema_has_explicit_root_fields() {
    let encoded = sample_snapshot().to_value().expect("encode");
    let ConfigValue::Table(root) = encoded else {
        panic!("encoded snapshot must be a table");
    };
    assert_eq!(
        root.keys().map(String::as_str).collect::<Vec<_>>(),
        [
            "active_workspace_id",
            "schema_version",
            "sessions",
            "workspaces"
        ]
    );
}
