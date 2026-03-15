use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScopeMode {
    Current,
    All,
}

impl ScopeMode {
    pub fn toggle(self) -> Self {
        match self {
            Self::Current => Self::All,
            Self::All => Self::Current,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Active,
    Idle,
    Archived,
    Orphaned,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActiveProcess {
    pub pid: u32,
    pub process_uuid: String,
    pub observed_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TmuxLocation {
    pub session: String,
    pub window: String,
    pub pane: String,
}

impl TmuxLocation {
    pub fn label(&self) -> String {
        format!("{} / {} / pane {}", self.session, self.window, self.pane)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionNode {
    pub id: String,
    pub parent_id: Option<String>,
    pub display_name: String,
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub repo_root: Option<String>,
    pub worktree_path: Option<String>,
    pub git_branch: Option<String>,
    pub git_sha: Option<String>,
    pub repo_url: Option<String>,
    pub updated_at: Option<i64>,
    pub archived: bool,
    pub rollout_path: Option<String>,
    pub workspace_match: bool,
    pub status: SessionStatus,
    pub active_process: Option<ActiveProcess>,
    pub tmux_location: Option<TmuxLocation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraphEdge {
    pub parent_id: String,
    pub child_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraphState {
    pub generated_at: i64,
    pub launch_cwd: String,
    pub launch_repo_root: Option<String>,
    pub initial_scope: ScopeMode,
    pub nodes: Vec<SessionNode>,
    pub edges: Vec<GraphEdge>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GraphEvent {
    Snapshot { state: GraphState },
}
