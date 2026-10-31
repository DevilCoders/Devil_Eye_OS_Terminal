//! Versioned, process-safe workspace and session restoration metadata.
//!
//! This module persists presentation metadata only. Decoding a snapshot never
//! starts a process, opens a PTY, restores a PID, or reconstructs command,
//! argument, environment, or other live process state.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::num::NonZeroU64;

use de_terminal_core::TerminalSize;

use crate::config::ConfigValue;
use crate::service_interfaces::SessionKind;

/// Current workspace snapshot schema version.
pub const WORKSPACE_SNAPSHOT_SCHEMA_VERSION: u64 = 1;

const MAX_WORKSPACES: usize = 32;
const MAX_TABS_PER_WORKSPACE: usize = 32;
const MAX_SESSIONS: usize = 128;
const MAX_TITLE_BYTES: usize = 128;
const MAX_PANE_DEPTH: usize = 8;
const MAX_LAYOUT_NODES: usize = 255;
const MIN_SPLIT_RATIO_PER_MILLE: u16 = 100;
const MAX_SPLIT_RATIO_PER_MILLE: u16 = 900;
const MAX_TERMINAL_COLUMNS: u16 = 500;
const MAX_TERMINAL_ROWS: u16 = 300;

macro_rules! persistent_id {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(NonZeroU64);

        impl $name {
            /// Creates a non-zero persistent identifier.
            pub const fn new(value: u64) -> Option<Self> {
                match NonZeroU64::new(value) {
                    Some(value) => Some(Self(value)),
                    None => None,
                }
            }

            /// Returns the numeric identifier.
            pub const fn get(self) -> u64 {
                self.0.get()
            }
        }
    };
}

persistent_id!(WorkspaceId, "Stable identifier for one saved workspace.");
persistent_id!(TabId, "Stable identifier for one saved terminal tab.");
persistent_id!(
    SavedSessionId,
    "Stable identifier for saved session metadata."
);

/// Orientation of a persisted pane split.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitOrientation {
    Horizontal,
    Vertical,
}

/// Presentation-only pane tree. Session leaves refer to saved metadata IDs,
/// not live runtime SessionIds or operating-system process identifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneLayout {
    Session {
        session_id: SavedSessionId,
    },
    Split {
        orientation: SplitOrientation,
        ratio_per_mille: u16,
        first: Box<PaneLayout>,
        second: Box<PaneLayout>,
    },
}

/// Metadata for one tab. The layout contains references to saved sessions;
/// it does not contain process launch specifications.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabMetadata {
    id: TabId,
    title: String,
    active_session_id: SavedSessionId,
    layout: PaneLayout,
}

impl TabMetadata {
    /// Creates tab metadata. Full reference validation occurs in the snapshot.
    pub fn new(
        id: TabId,
        title: impl Into<String>,
        active_session_id: SavedSessionId,
        layout: PaneLayout,
    ) -> Self {
        Self {
            id,
            title: title.into(),
            active_session_id,
            layout,
        }
    }

    /// Persistent tab identity.
    pub const fn id(&self) -> TabId {
        self.id
    }

    /// User-visible tab title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Active saved session in this tab.
    pub const fn active_session_id(&self) -> SavedSessionId {
        self.active_session_id
    }

    /// Persisted pane layout.
    pub fn layout(&self) -> &PaneLayout {
        &self.layout
    }
}

/// Metadata for one workspace and its ordered tabs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceMetadata {
    id: WorkspaceId,
    name: String,
    active_tab_id: TabId,
    tabs: Vec<TabMetadata>,
}

impl WorkspaceMetadata {
    /// Creates workspace metadata. Full reference validation occurs in the snapshot.
    pub fn new(
        id: WorkspaceId,
        name: impl Into<String>,
        active_tab_id: TabId,
        tabs: Vec<TabMetadata>,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            active_tab_id,
            tabs,
        }
    }

    /// Persistent workspace identity.
    pub const fn id(&self) -> WorkspaceId {
        self.id
    }

    /// User-visible workspace name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Active tab identity.
    pub const fn active_tab_id(&self) -> TabId {
        self.active_tab_id
    }

    /// Tabs in display order.
    pub fn tabs(&self) -> &[TabMetadata] {
        &self.tabs
    }
}

/// Safe metadata for a prior terminal session.
///
/// Deliberately excluded: executable, arguments, environment, PID, process
/// group, PTY descriptors, child exit state, and queued input. A restored
/// session is a metadata record requiring an explicit user action to launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedSessionMetadata {
    id: SavedSessionId,
    title: String,
    kind: SessionKind,
    terminal_size: TerminalSize,
}

impl SavedSessionMetadata {
    /// Creates process-free metadata for one saved terminal session.
    pub fn new(
        id: SavedSessionId,
        title: impl Into<String>,
        kind: SessionKind,
        terminal_size: TerminalSize,
    ) -> Self {
        Self {
            id,
            title: title.into(),
            kind,
            terminal_size,
        }
    }

    /// Persistent metadata identity; this is not a live SessionId.
    pub const fn id(&self) -> SavedSessionId {
        self.id
    }

    /// User-visible session title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Logical session backend.
    pub const fn kind(&self) -> SessionKind {
        self.kind
    }

    /// Last known terminal dimensions.
    pub const fn terminal_size(&self) -> TerminalSize {
        self.terminal_size
    }
}

/// Validated workspace snapshot containing presentation state only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSnapshot {
    active_workspace_id: WorkspaceId,
    workspaces: Vec<WorkspaceMetadata>,
    sessions: Vec<SavedSessionMetadata>,
}

impl WorkspaceSnapshot {
    /// Creates and validates a workspace snapshot.
    pub fn new(
        active_workspace_id: WorkspaceId,
        workspaces: Vec<WorkspaceMetadata>,
        sessions: Vec<SavedSessionMetadata>,
    ) -> Result<Self, WorkspaceError> {
        let snapshot = Self {
            active_workspace_id,
            workspaces,
            sessions,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// Active workspace identity.
    pub const fn active_workspace_id(&self) -> WorkspaceId {
        self.active_workspace_id
    }

    /// Workspaces in display order.
    pub fn workspaces(&self) -> &[WorkspaceMetadata] {
        &self.workspaces
    }

    /// Saved session metadata. No process state is represented.
    pub fn sessions(&self) -> &[SavedSessionMetadata] {
        &self.sessions
    }

    /// Validates identifiers, layout references, bounds, and active selections.
    pub fn validate(&self) -> Result<(), WorkspaceError> {
        if self.workspaces.is_empty() || self.workspaces.len() > MAX_WORKSPACES {
            return Err(invalid(
                "workspaces",
                "must contain between 1 and 32 workspaces",
            ));
        }
        if self.sessions.is_empty() || self.sessions.len() > MAX_SESSIONS {
            return Err(invalid(
                "sessions",
                "must contain between 1 and 128 saved sessions",
            ));
        }

        let mut workspace_ids = BTreeSet::new();
        let mut tab_ids = BTreeSet::new();
        let mut session_ids = BTreeSet::new();

        for session in &self.sessions {
            if !session_ids.insert(session.id) {
                return Err(invalid(
                    "sessions.id",
                    "persistent session IDs must be unique",
                ));
            }
            validate_title(&session.title, "sessions.title")?;
            let columns = session.terminal_size.columns();
            let rows = session.terminal_size.rows();
            if columns == 0
                || rows == 0
                || columns > MAX_TERMINAL_COLUMNS
                || rows > MAX_TERMINAL_ROWS
                || usize::from(columns).saturating_mul(usize::from(rows)) > 150_000
            {
                return Err(invalid(
                    "sessions.terminal_size",
                    "dimensions exceed the validated terminal bounds",
                ));
            }
        }

        let mut referenced_sessions = BTreeSet::new();
        for workspace in &self.workspaces {
            if !workspace_ids.insert(workspace.id) {
                return Err(invalid("workspaces.id", "workspace IDs must be unique"));
            }
            validate_title(&workspace.name, "workspaces.name")?;
            if workspace.tabs.is_empty() || workspace.tabs.len() > MAX_TABS_PER_WORKSPACE {
                return Err(invalid(
                    "workspaces.tabs",
                    "each workspace must contain between 1 and 32 tabs",
                ));
            }

            let mut active_tab_found = false;
            for tab in &workspace.tabs {
                if !tab_ids.insert(tab.id) {
                    return Err(invalid("workspaces.tabs.id", "tab IDs must be unique"));
                }
                validate_title(&tab.title, "workspaces.tabs.title")?;
                if tab.id == workspace.active_tab_id {
                    active_tab_found = true;
                }

                let mut layout_sessions = Vec::new();
                let mut node_count = 0usize;
                collect_layout_sessions(&tab.layout, 1, &mut node_count, &mut layout_sessions)?;
                if layout_sessions.is_empty() {
                    return Err(invalid(
                        "workspaces.tabs.layout",
                        "must contain a session pane",
                    ));
                }
                if !layout_sessions.contains(&tab.active_session_id) {
                    return Err(invalid(
                        "workspaces.tabs.active_session_id",
                        "must refer to a session in this tab layout",
                    ));
                }
                for session_id in layout_sessions {
                    if !session_ids.contains(&session_id) {
                        return Err(invalid(
                            "workspaces.tabs.layout",
                            "references unknown saved session metadata",
                        ));
                    }
                    if !referenced_sessions.insert(session_id) {
                        return Err(invalid(
                            "workspaces.tabs.layout",
                            "a saved session may appear in only one pane",
                        ));
                    }
                }
            }
            if !active_tab_found {
                return Err(invalid(
                    "workspaces.active_tab_id",
                    "must refer to a tab in its workspace",
                ));
            }
        }

        if !workspace_ids.contains(&self.active_workspace_id) {
            return Err(invalid(
                "active_workspace_id",
                "must refer to a saved workspace",
            ));
        }
        if referenced_sessions != session_ids {
            return Err(invalid(
                "sessions",
                "every saved session must be referenced by exactly one pane",
            ));
        }
        Ok(())
    }

    /// Encodes the snapshot into the format-neutral ConfigValue tree.
    ///
    /// This method serializes metadata only. It never reads or serializes live
    /// process handles, executable specifications, environment values, or PIDs.
    pub fn to_value(&self) -> Result<ConfigValue, WorkspaceError> {
        self.validate()?;

        let workspaces = self
            .workspaces
            .iter()
            .map(encode_workspace)
            .collect::<Result<Vec<_>, _>>()?;
        let sessions = self
            .sessions
            .iter()
            .map(encode_session)
            .collect::<Result<Vec<_>, _>>()?;

        let mut root = BTreeMap::new();
        root.insert(
            "schema_version".to_owned(),
            ConfigValue::Integer(to_i64(WORKSPACE_SNAPSHOT_SCHEMA_VERSION, "schema_version")?),
        );
        root.insert(
            "active_workspace_id".to_owned(),
            ConfigValue::Integer(to_i64(
                self.active_workspace_id.get(),
                "active_workspace_id",
            )?),
        );
        root.insert("workspaces".to_owned(), ConfigValue::Array(workspaces));
        root.insert("sessions".to_owned(), ConfigValue::Array(sessions));
        Ok(ConfigValue::Table(root))
    }

    /// Decodes and validates workspace metadata without starting any process.
    ///
    /// A caller may restore the workspace/tab/pane presentation from the
    /// returned value. Each saved session remains metadata-only and requires
    /// an explicit, separate user action before any new process is launched.
    pub fn from_value(value: ConfigValue) -> Result<Self, WorkspaceError> {
        let ConfigValue::Table(mut root) = value else {
            return Err(WorkspaceError::RootMustBeTable);
        };

        let version = take_u64(&mut root, "schema_version", "root")?;
        if version != WORKSPACE_SNAPSHOT_SCHEMA_VERSION {
            return Err(WorkspaceError::UnsupportedVersion {
                found: version,
                expected: WORKSPACE_SNAPSHOT_SCHEMA_VERSION,
            });
        }

        let active_workspace_id =
            WorkspaceId::new(take_u64(&mut root, "active_workspace_id", "root")?)
                .ok_or_else(|| invalid("active_workspace_id", "must be non-zero"))?;

        let workspace_values = take_array(&mut root, "workspaces", "root")?;
        if workspace_values.len() > MAX_WORKSPACES {
            return Err(invalid("workspaces", "must not exceed 32 workspaces"));
        }
        let workspaces = workspace_values
            .into_iter()
            .enumerate()
            .map(|(index, value)| decode_workspace(value, index))
            .collect::<Result<Vec<_>, _>>()?;
        let session_values = take_array(&mut root, "sessions", "root")?;
        if session_values.len() > MAX_SESSIONS {
            return Err(invalid("sessions", "must not exceed 128 saved sessions"));
        }
        let sessions = session_values
            .into_iter()
            .enumerate()
            .map(|(index, value)| decode_session(value, index))
            .collect::<Result<Vec<_>, _>>()?;
        ensure_no_unknown(root, "root")?;

        Self::new(active_workspace_id, workspaces, sessions)
    }
}

/// Errors returned by workspace snapshot decoding or validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceError {
    RootMustBeTable,
    MissingField {
        section: String,
        key: String,
    },
    UnknownKey {
        section: String,
        key: String,
    },
    WrongType {
        section: String,
        key: String,
        expected: &'static str,
    },
    UnsupportedVersion {
        found: u64,
        expected: u64,
    },
    InvalidValue {
        field: &'static str,
        reason: &'static str,
    },
}

impl fmt::Display for WorkspaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RootMustBeTable => formatter.write_str("workspace snapshot root must be a table"),
            Self::MissingField { section, key } => {
                write!(formatter, "missing workspace field {section}.{key}")
            }
            Self::UnknownKey { section, key } => {
                write!(formatter, "unknown workspace field {section}.{key}")
            }
            Self::WrongType {
                section,
                key,
                expected,
            } => write!(
                formatter,
                "workspace field {section}.{key} must be {expected}"
            ),
            Self::UnsupportedVersion { found, expected } => write!(
                formatter,
                "workspace snapshot schema version {found} is unsupported; expected {expected}"
            ),
            Self::InvalidValue { field, reason } => {
                write!(formatter, "invalid workspace value for {field}: {reason}")
            }
        }
    }
}

impl Error for WorkspaceError {}

fn invalid(field: &'static str, reason: &'static str) -> WorkspaceError {
    WorkspaceError::InvalidValue { field, reason }
}

fn validate_title(value: &str, field: &'static str) -> Result<(), WorkspaceError> {
    if value.trim().is_empty() {
        return Err(invalid(field, "must not be empty"));
    }
    if value.len() > MAX_TITLE_BYTES {
        return Err(invalid(field, "must not exceed 128 UTF-8 bytes"));
    }
    if value.chars().any(char::is_control) {
        return Err(invalid(field, "must not contain control characters"));
    }
    Ok(())
}

fn collect_layout_sessions(
    layout: &PaneLayout,
    depth: usize,
    node_count: &mut usize,
    sessions: &mut Vec<SavedSessionId>,
) -> Result<(), WorkspaceError> {
    *node_count += 1;
    if depth > MAX_PANE_DEPTH || *node_count > MAX_LAYOUT_NODES {
        return Err(invalid(
            "workspaces.tabs.layout",
            "pane tree exceeds the maximum depth or node count",
        ));
    }

    match layout {
        PaneLayout::Session { session_id } => sessions.push(*session_id),
        PaneLayout::Split {
            ratio_per_mille,
            first,
            second,
            ..
        } => {
            if !(MIN_SPLIT_RATIO_PER_MILLE..=MAX_SPLIT_RATIO_PER_MILLE).contains(ratio_per_mille) {
                return Err(invalid(
                    "workspaces.tabs.layout.ratio_per_mille",
                    "must be between 100 and 900",
                ));
            }
            collect_layout_sessions(first, depth + 1, node_count, sessions)?;
            collect_layout_sessions(second, depth + 1, node_count, sessions)?;
        }
    }
    Ok(())
}

fn encode_workspace(workspace: &WorkspaceMetadata) -> Result<ConfigValue, WorkspaceError> {
    let mut table = BTreeMap::new();
    table.insert(
        "id".to_owned(),
        id_value(workspace.id.get(), "workspaces.id")?,
    );
    table.insert(
        "name".to_owned(),
        ConfigValue::String(workspace.name.clone()),
    );
    table.insert(
        "active_tab_id".to_owned(),
        id_value(workspace.active_tab_id.get(), "workspaces.active_tab_id")?,
    );
    table.insert(
        "tabs".to_owned(),
        ConfigValue::Array(
            workspace
                .tabs
                .iter()
                .map(encode_tab)
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    Ok(ConfigValue::Table(table))
}

fn encode_tab(tab: &TabMetadata) -> Result<ConfigValue, WorkspaceError> {
    let mut table = BTreeMap::new();
    table.insert(
        "id".to_owned(),
        id_value(tab.id.get(), "workspaces.tabs.id")?,
    );
    table.insert("title".to_owned(), ConfigValue::String(tab.title.clone()));
    table.insert(
        "active_session_id".to_owned(),
        id_value(
            tab.active_session_id.get(),
            "workspaces.tabs.active_session_id",
        )?,
    );
    table.insert("layout".to_owned(), encode_layout(&tab.layout)?);
    Ok(ConfigValue::Table(table))
}

fn encode_layout(layout: &PaneLayout) -> Result<ConfigValue, WorkspaceError> {
    let mut table = BTreeMap::new();
    match layout {
        PaneLayout::Session { session_id } => {
            table.insert("kind".to_owned(), ConfigValue::String("session".to_owned()));
            table.insert(
                "session_id".to_owned(),
                id_value(session_id.get(), "workspaces.tabs.layout.session_id")?,
            );
        }
        PaneLayout::Split {
            orientation,
            ratio_per_mille,
            first,
            second,
        } => {
            table.insert("kind".to_owned(), ConfigValue::String("split".to_owned()));
            table.insert(
                "orientation".to_owned(),
                ConfigValue::String(
                    match orientation {
                        SplitOrientation::Horizontal => "horizontal",
                        SplitOrientation::Vertical => "vertical",
                    }
                    .to_owned(),
                ),
            );
            table.insert(
                "ratio_per_mille".to_owned(),
                ConfigValue::Integer(i64::from(*ratio_per_mille)),
            );
            table.insert("first".to_owned(), encode_layout(first)?);
            table.insert("second".to_owned(), encode_layout(second)?);
        }
    }
    Ok(ConfigValue::Table(table))
}

fn encode_session(session: &SavedSessionMetadata) -> Result<ConfigValue, WorkspaceError> {
    let mut table = BTreeMap::new();
    table.insert("id".to_owned(), id_value(session.id.get(), "sessions.id")?);
    table.insert(
        "title".to_owned(),
        ConfigValue::String(session.title.clone()),
    );
    table.insert(
        "kind".to_owned(),
        ConfigValue::String(
            match session.kind {
                SessionKind::ExternalProgram => "external_program",
                SessionKind::DeShell => "de_shell",
            }
            .to_owned(),
        ),
    );
    table.insert(
        "columns".to_owned(),
        ConfigValue::Integer(i64::from(session.terminal_size.columns())),
    );
    table.insert(
        "rows".to_owned(),
        ConfigValue::Integer(i64::from(session.terminal_size.rows())),
    );
    Ok(ConfigValue::Table(table))
}

fn decode_workspace(value: ConfigValue, index: usize) -> Result<WorkspaceMetadata, WorkspaceError> {
    let section = format!("workspaces[{index}]");
    let mut table = into_table(value, &section)?;
    let id = WorkspaceId::new(take_u64(&mut table, "id", &section)?)
        .ok_or_else(|| invalid("workspaces.id", "must be non-zero"))?;
    let name = take_string(&mut table, "name", &section)?;
    let active_tab_id = TabId::new(take_u64(&mut table, "active_tab_id", &section)?)
        .ok_or_else(|| invalid("workspaces.active_tab_id", "must be non-zero"))?;
    let tab_values = take_array(&mut table, "tabs", &section)?;
    if tab_values.len() > MAX_TABS_PER_WORKSPACE {
        return Err(invalid(
            "workspaces.tabs",
            "must not exceed 32 tabs per workspace",
        ));
    }
    let tabs = tab_values
        .into_iter()
        .enumerate()
        .map(|(tab_index, value)| decode_tab(value, index, tab_index))
        .collect::<Result<Vec<_>, _>>()?;
    ensure_no_unknown(table, &section)?;
    Ok(WorkspaceMetadata::new(id, name, active_tab_id, tabs))
}

fn decode_tab(
    value: ConfigValue,
    workspace_index: usize,
    tab_index: usize,
) -> Result<TabMetadata, WorkspaceError> {
    let section = format!("workspaces[{workspace_index}].tabs[{tab_index}]");
    let mut table = into_table(value, &section)?;
    let id = TabId::new(take_u64(&mut table, "id", &section)?)
        .ok_or_else(|| invalid("workspaces.tabs.id", "must be non-zero"))?;
    let title = take_string(&mut table, "title", &section)?;
    let active_session_id =
        SavedSessionId::new(take_u64(&mut table, "active_session_id", &section)?)
            .ok_or_else(|| invalid("workspaces.tabs.active_session_id", "must be non-zero"))?;
    let layout = decode_layout(
        take_required(&mut table, "layout", &section)?,
        &format!("{section}.layout"),
        1,
    )?;
    ensure_no_unknown(table, &section)?;
    Ok(TabMetadata::new(id, title, active_session_id, layout))
}

fn decode_layout(
    value: ConfigValue,
    section: &str,
    depth: usize,
) -> Result<PaneLayout, WorkspaceError> {
    if depth > MAX_PANE_DEPTH {
        return Err(invalid(
            "workspaces.tabs.layout",
            "pane tree exceeds maximum depth",
        ));
    }
    let mut table = into_table(value, section)?;
    let kind = take_string(&mut table, "kind", section)?;
    let layout = match kind.as_str() {
        "session" => {
            let session_id = SavedSessionId::new(take_u64(&mut table, "session_id", section)?)
                .ok_or_else(|| invalid("workspaces.tabs.layout.session_id", "must be non-zero"))?;
            PaneLayout::Session { session_id }
        }
        "split" => {
            let orientation = match take_string(&mut table, "orientation", section)?.as_str() {
                "horizontal" => SplitOrientation::Horizontal,
                "vertical" => SplitOrientation::Vertical,
                _ => {
                    return Err(invalid(
                        "workspaces.tabs.layout.orientation",
                        "must be horizontal or vertical",
                    ));
                }
            };
            let ratio = take_u64(&mut table, "ratio_per_mille", section)?;
            let ratio_per_mille = u16::try_from(ratio)
                .map_err(|_| invalid("workspaces.tabs.layout.ratio_per_mille", "out of range"))?;
            let first = decode_layout(
                take_required(&mut table, "first", section)?,
                &format!("{section}.first"),
                depth + 1,
            )?;
            let second = decode_layout(
                take_required(&mut table, "second", section)?,
                &format!("{section}.second"),
                depth + 1,
            )?;
            PaneLayout::Split {
                orientation,
                ratio_per_mille,
                first: Box::new(first),
                second: Box::new(second),
            }
        }
        _ => {
            return Err(invalid(
                "workspaces.tabs.layout.kind",
                "must be session or split",
            ));
        }
    };
    ensure_no_unknown(table, section)?;
    Ok(layout)
}

fn decode_session(
    value: ConfigValue,
    index: usize,
) -> Result<SavedSessionMetadata, WorkspaceError> {
    let section = format!("sessions[{index}]");
    let mut table = into_table(value, &section)?;
    let id = SavedSessionId::new(take_u64(&mut table, "id", &section)?)
        .ok_or_else(|| invalid("sessions.id", "must be non-zero"))?;
    let title = take_string(&mut table, "title", &section)?;
    let kind = match take_string(&mut table, "kind", &section)?.as_str() {
        "external_program" => SessionKind::ExternalProgram,
        "de_shell" => SessionKind::DeShell,
        _ => return Err(invalid("sessions.kind", "unsupported session kind")),
    };
    let columns = u16::try_from(take_u64(&mut table, "columns", &section)?)
        .map_err(|_| invalid("sessions.columns", "must fit in a non-zero u16"))?;
    let rows = u16::try_from(take_u64(&mut table, "rows", &section)?)
        .map_err(|_| invalid("sessions.rows", "must fit in a non-zero u16"))?;
    let terminal_size = TerminalSize::new(columns, rows)
        .map_err(|_| invalid("sessions.terminal_size", "dimensions must be non-zero"))?;
    ensure_no_unknown(table, &section)?;
    Ok(SavedSessionMetadata::new(id, title, kind, terminal_size))
}

fn id_value(value: u64, field: &'static str) -> Result<ConfigValue, WorkspaceError> {
    Ok(ConfigValue::Integer(to_i64(value, field)?))
}

fn to_i64(value: u64, field: &'static str) -> Result<i64, WorkspaceError> {
    i64::try_from(value).map_err(|_| invalid(field, "must fit in a signed 64-bit integer"))
}

fn take_required(
    table: &mut BTreeMap<String, ConfigValue>,
    key: &str,
    section: &str,
) -> Result<ConfigValue, WorkspaceError> {
    table
        .remove(key)
        .ok_or_else(|| WorkspaceError::MissingField {
            section: section.to_owned(),
            key: key.to_owned(),
        })
}

fn take_u64(
    table: &mut BTreeMap<String, ConfigValue>,
    key: &str,
    section: &str,
) -> Result<u64, WorkspaceError> {
    match take_required(table, key, section)? {
        ConfigValue::Integer(value) if value >= 0 => {
            u64::try_from(value).map_err(|_| invalid("workspace.identifier", "out of range"))
        }
        ConfigValue::Integer(_) => Err(invalid("workspace.identifier", "must be non-negative")),
        _ => Err(wrong_type(section, key, "an integer")),
    }
}

fn take_string(
    table: &mut BTreeMap<String, ConfigValue>,
    key: &str,
    section: &str,
) -> Result<String, WorkspaceError> {
    match take_required(table, key, section)? {
        ConfigValue::String(value) => Ok(value),
        _ => Err(wrong_type(section, key, "a string")),
    }
}

fn take_array(
    table: &mut BTreeMap<String, ConfigValue>,
    key: &str,
    section: &str,
) -> Result<Vec<ConfigValue>, WorkspaceError> {
    match take_required(table, key, section)? {
        ConfigValue::Array(value) => Ok(value),
        _ => Err(wrong_type(section, key, "an array")),
    }
}

fn into_table(
    value: ConfigValue,
    section: &str,
) -> Result<BTreeMap<String, ConfigValue>, WorkspaceError> {
    match value {
        ConfigValue::Table(value) => Ok(value),
        _ => Err(wrong_type(section, "entry", "a table")),
    }
}

fn ensure_no_unknown(
    table: BTreeMap<String, ConfigValue>,
    section: &str,
) -> Result<(), WorkspaceError> {
    if let Some(key) = table.keys().next() {
        return Err(WorkspaceError::UnknownKey {
            section: section.to_owned(),
            key: key.clone(),
        });
    }
    Ok(())
}

fn wrong_type(section: &str, key: &str, expected: &'static str) -> WorkspaceError {
    WorkspaceError::WrongType {
        section: section.to_owned(),
        key: key.to_owned(),
        expected,
    }
}
