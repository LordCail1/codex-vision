use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result};
use dirs::home_dir;
use rusqlite::Connection;
use serde_json::Value;
use sysinfo::{Pid, ProcessesToUpdate, System};

use crate::model::{
    ActiveProcess, GraphEdge, GraphState, ScopeMode, SessionNode, SessionStatus, TmuxLocation,
};

#[derive(Debug, Clone)]
pub struct ScanConfig {
    pub codex_home: PathBuf,
    pub launch_cwd: PathBuf,
    pub launch_repo_root: Option<PathBuf>,
    pub launch_repo_name: Option<String>,
    pub launch_repo_url: Option<String>,
    pub initial_scope: ScopeMode,
}

impl ScanConfig {
    pub fn discover(initial_scope: ScopeMode) -> Result<Self> {
        let launch_cwd =
            std::env::current_dir().context("failed to determine current directory")?;
        let codex_home = std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .or_else(|| home_dir().map(|path| path.join(".codex")))
            .context("failed to determine Codex home")?;
        let launch_repo_root = resolve_git_toplevel(&launch_cwd);
        let launch_repo_name = launch_repo_root
            .as_deref()
            .and_then(repo_family_name)
            .or_else(|| repo_family_name(&launch_cwd));
        let launch_repo_url = launch_repo_root
            .as_deref()
            .and_then(resolve_git_origin_url)
            .or_else(|| resolve_git_origin_url(&launch_cwd));

        Ok(Self {
            codex_home,
            launch_cwd,
            launch_repo_root,
            launch_repo_name,
            launch_repo_url,
            initial_scope,
        })
    }
}

#[derive(Debug, Clone, Default)]
struct ThreadRecord {
    id: String,
    rollout_path: Option<String>,
    cwd: Option<String>,
    title: Option<String>,
    updated_at: Option<i64>,
    archived: bool,
    git_branch: Option<String>,
    git_sha: Option<String>,
    repo_url: Option<String>,
    first_user_message: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct RolloutMeta {
    id: String,
    parent_id: Option<String>,
    cwd: Option<String>,
    git_branch: Option<String>,
    git_sha: Option<String>,
    repo_url: Option<String>,
    rollout_path: Option<String>,
}

#[derive(Debug, Clone)]
struct PaneRecord {
    pane_pid: u32,
    location: TmuxLocation,
}

#[derive(Debug)]
pub struct GraphScanner {
    config: ScanConfig,
}

impl GraphScanner {
    pub fn new(config: ScanConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &ScanConfig {
        &self.config
    }

    pub fn scan(&self) -> Result<GraphState> {
        let mut warnings = Vec::new();
        let threads = self.load_threads(&mut warnings)?;
        let session_names = self.load_session_names(&mut warnings)?;
        let active_threads = self.load_active_threads(&mut warnings)?;
        let pane_locations = self.load_tmux_locations(&mut warnings);

        let mut repo_cache = HashMap::new();
        let mut nodes = Vec::with_capacity(threads.len());
        let mut ids = HashSet::with_capacity(threads.len());

        for thread in threads {
            ids.insert(thread.id.clone());
            let rollout = thread
                .rollout_path
                .as_deref()
                .and_then(|path| self.load_rollout_meta(path, &mut warnings));
            let id = rollout
                .as_ref()
                .map(|meta| meta.id.clone())
                .filter(|id| !id.is_empty())
                .unwrap_or_else(|| thread.id.clone());
            let cwd = thread
                .cwd
                .clone()
                .or_else(|| rollout.as_ref().and_then(|meta| meta.cwd.clone()));
            let repo_root = cwd
                .as_deref()
                .and_then(|path| resolve_cached_repo_root(path, &mut repo_cache));
            let repo_url = thread
                .repo_url
                .clone()
                .or_else(|| rollout.as_ref().and_then(|meta| meta.repo_url.clone()));
            let display_name = thread
                .title
                .clone()
                .filter(|value| !value.trim().is_empty())
                .or_else(|| session_names.get(&id).cloned())
                .or_else(|| short_text(thread.first_user_message.as_deref()))
                .map(|value| compact_label(&value))
                .unwrap_or_else(|| short_id(&id));

            let active_process = active_threads.get(&id).cloned();
            let status = if active_process.is_some() {
                SessionStatus::Active
            } else if thread.archived {
                SessionStatus::Archived
            } else {
                SessionStatus::Idle
            };

            let tmux_location = active_process.as_ref().and_then(|proc_info| {
                pane_locations
                    .as_ref()
                    .and_then(|panes| match_pid_to_tmux(proc_info.pid, panes))
            });

            let workspace_match = match_workspace(
                cwd.as_deref(),
                repo_url.as_deref(),
                &self.config.launch_cwd,
                self.config.launch_repo_root.as_deref(),
                self.config.launch_repo_name.as_deref(),
                self.config.launch_repo_url.as_deref(),
            );

            nodes.push(SessionNode {
                id,
                parent_id: rollout.as_ref().and_then(|meta| meta.parent_id.clone()),
                display_name,
                title: thread.title.clone(),
                cwd,
                repo_root: repo_root.clone(),
                worktree_path: repo_root,
                git_branch: thread
                    .git_branch
                    .clone()
                    .or_else(|| rollout.as_ref().and_then(|meta| meta.git_branch.clone())),
                git_sha: thread
                    .git_sha
                    .clone()
                    .or_else(|| rollout.as_ref().and_then(|meta| meta.git_sha.clone())),
                repo_url,
                updated_at: thread.updated_at,
                archived: thread.archived,
                rollout_path: thread
                    .rollout_path
                    .clone()
                    .or_else(|| rollout.as_ref().and_then(|meta| meta.rollout_path.clone())),
                workspace_match,
                status,
                active_process,
                tmux_location,
            });
        }

        let mut edges = Vec::new();
        let id_set: HashSet<_> = nodes.iter().map(|node| node.id.clone()).collect();
        for node in &mut nodes {
            if let Some(parent_id) = node.parent_id.clone() {
                if id_set.contains(&parent_id) {
                    edges.push(GraphEdge {
                        parent_id,
                        child_id: node.id.clone(),
                    });
                } else {
                    node.status = SessionStatus::Orphaned;
                    warnings.push(format!(
                        "session {} references missing parent {}",
                        short_id(&node.id),
                        short_id(&parent_id)
                    ));
                }
            }
            if node.rollout_path.is_none() {
                warnings.push(format!(
                    "session {} is missing rollout metadata",
                    short_id(&node.id)
                ));
            }
        }

        nodes.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        edges.sort_by(|left, right| {
            left.parent_id
                .cmp(&right.parent_id)
                .then_with(|| left.child_id.cmp(&right.child_id))
        });
        warnings.sort();
        warnings.dedup();

        Ok(GraphState {
            generated_at: unix_now(),
            launch_cwd: self.config.launch_cwd.display().to_string(),
            launch_repo_root: self
                .config
                .launch_repo_root
                .as_ref()
                .map(|path| path.display().to_string()),
            initial_scope: self.config.initial_scope,
            nodes,
            edges,
            warnings,
        })
    }

    fn load_threads(&self, warnings: &mut Vec<String>) -> Result<Vec<ThreadRecord>> {
        let db_path = self.config.codex_home.join("state_5.sqlite");
        if !db_path.exists() {
            warnings.push(format!("missing {}", db_path.display()));
            return Ok(Vec::new());
        }

        let conn = Connection::open(&db_path)
            .with_context(|| format!("failed to open {}", db_path.display()))?;
        let mut stmt = conn.prepare(
            r#"
            SELECT
              id,
              rollout_path,
              cwd,
              title,
              updated_at,
              archived,
              git_branch,
              git_sha,
              git_origin_url,
              first_user_message
            FROM threads
            ORDER BY updated_at DESC
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ThreadRecord {
                id: row.get::<_, String>(0)?,
                rollout_path: row.get::<_, Option<String>>(1)?,
                cwd: row.get::<_, Option<String>>(2)?,
                title: row.get::<_, Option<String>>(3)?,
                updated_at: row.get::<_, Option<i64>>(4)?,
                archived: row.get::<_, i64>(5)? == 1,
                git_branch: row.get::<_, Option<String>>(6)?,
                git_sha: row.get::<_, Option<String>>(7)?,
                repo_url: row.get::<_, Option<String>>(8)?,
                first_user_message: row.get::<_, Option<String>>(9)?,
            })
        })?;

        let mut records = Vec::new();
        for row in rows {
            records.push(row?);
        }
        Ok(records)
    }

    fn load_session_names(&self, warnings: &mut Vec<String>) -> Result<HashMap<String, String>> {
        let path = self.config.codex_home.join("session_index.jsonl");
        if !path.exists() {
            return Ok(HashMap::new());
        }

        let file =
            File::open(&path).with_context(|| format!("failed to open {}", path.display()))?;
        let reader = BufReader::new(file);
        let mut map = HashMap::new();
        for line in reader.lines() {
            match line {
                Ok(line) if !line.trim().is_empty() => match serde_json::from_str::<Value>(&line) {
                    Ok(value) => {
                        let id = value.get("id").and_then(Value::as_str);
                        let name = value.get("thread_name").and_then(Value::as_str);
                        if let (Some(id), Some(name)) = (id, name) {
                            map.insert(id.to_string(), name.to_string());
                        }
                    }
                    Err(err) => warnings.push(format!("failed to parse session index line: {err}")),
                },
                Ok(_) => {}
                Err(err) => warnings.push(format!("failed to read session index: {err}")),
            }
        }
        Ok(map)
    }

    fn load_active_threads(
        &self,
        warnings: &mut Vec<String>,
    ) -> Result<HashMap<String, ActiveProcess>> {
        let db_path = self.config.codex_home.join("logs_1.sqlite");
        if !db_path.exists() {
            warnings.push(format!("missing {}", db_path.display()));
            return Ok(HashMap::new());
        }

        let conn = Connection::open(&db_path)
            .with_context(|| format!("failed to open {}", db_path.display()))?;
        let mut stmt = conn.prepare(
            r#"
            SELECT process_uuid, thread_id, MAX(ts) AS max_ts
            FROM logs
            WHERE thread_id IS NOT NULL
            GROUP BY process_uuid, thread_id
            ORDER BY max_ts DESC
            "#,
        )?;

        let mut system = System::new_all();
        system.refresh_processes(ProcessesToUpdate::All, true);
        let mut map = HashMap::new();
        let mut seen_processes = HashSet::new();
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;

        for row in rows {
            let (process_uuid, thread_id, observed_at) = row?;
            if seen_processes.contains(&process_uuid) {
                continue;
            }
            let Some(pid) = parse_process_uuid(&process_uuid) else {
                warnings.push(format!("failed to parse process uuid {process_uuid}"));
                continue;
            };
            if system.process(Pid::from_u32(pid)).is_some() {
                seen_processes.insert(process_uuid.clone());
                map.insert(
                    thread_id,
                    ActiveProcess {
                        pid,
                        process_uuid,
                        observed_at,
                    },
                );
            }
        }

        Ok(map)
    }

    fn load_rollout_meta(
        &self,
        rollout_path: &str,
        warnings: &mut Vec<String>,
    ) -> Option<RolloutMeta> {
        let path = PathBuf::from(rollout_path);
        let file = match File::open(&path) {
            Ok(file) => file,
            Err(err) => {
                warnings.push(format!("failed to open {}: {err}", path.display()));
                return None;
            }
        };

        let reader = BufReader::new(file);
        for line in reader.lines() {
            let Ok(line) = line else {
                warnings.push(format!("failed to read {}", path.display()));
                return None;
            };
            if line.trim().is_empty() {
                continue;
            }
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if value.get("type").and_then(Value::as_str) != Some("session_meta") {
                continue;
            }

            let payload = value.get("payload")?;
            let git = payload.get("git");
            return Some(RolloutMeta {
                id: payload
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                parent_id: payload
                    .get("forked_from_id")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                cwd: payload
                    .get("cwd")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                git_branch: git
                    .and_then(|git| git.get("branch"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                git_sha: git
                    .and_then(|git| git.get("commit_hash"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                repo_url: git
                    .and_then(|git| git.get("repository_url"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                rollout_path: Some(path.display().to_string()),
            });
        }

        None
    }

    fn load_tmux_locations(&self, warnings: &mut Vec<String>) -> Option<Vec<PaneRecord>> {
        let output = Command::new("tmux")
            .args([
                "list-panes",
                "-a",
                "-F",
                "#{session_name}\t#{window_name}\t#{pane_index}\t#{pane_pid}",
            ])
            .output()
            .ok()?;

        if !output.status.success() {
            warnings.push("tmux metadata unavailable".to_string());
            return None;
        }

        let mut panes = Vec::new();
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let mut parts = line.split('\t');
            let Some(session) = parts.next() else {
                continue;
            };
            let Some(window) = parts.next() else {
                continue;
            };
            let Some(pane) = parts.next() else {
                continue;
            };
            let Some(pid) = parts.next().and_then(|value| value.parse::<u32>().ok()) else {
                continue;
            };
            panes.push(PaneRecord {
                pane_pid: pid,
                location: TmuxLocation {
                    session: session.to_string(),
                    window: window.to_string(),
                    pane: pane.to_string(),
                },
            });
        }
        Some(panes)
    }
}

fn match_pid_to_tmux(pid: u32, panes: &[PaneRecord]) -> Option<TmuxLocation> {
    let pane_map: HashMap<u32, &TmuxLocation> = panes
        .iter()
        .map(|pane| (pane.pane_pid, &pane.location))
        .collect();
    let mut system = System::new_all();
    system.refresh_processes(ProcessesToUpdate::All, true);

    let mut current = Some(Pid::from_u32(pid));
    let mut hops = 0;
    while let Some(active_pid) = current {
        if hops > 64 {
            break;
        }
        if let Some(location) = pane_map.get(&active_pid.as_u32()) {
            return Some((*location).clone());
        }
        current = system
            .process(active_pid)
            .and_then(|process| process.parent());
        hops += 1;
    }

    None
}

fn parse_process_uuid(value: &str) -> Option<u32> {
    value.split(':').nth(1)?.parse().ok()
}

fn match_workspace(
    cwd: Option<&str>,
    repo_url: Option<&str>,
    launch_cwd: &Path,
    launch_repo_root: Option<&Path>,
    launch_repo_name: Option<&str>,
    launch_repo_url: Option<&str>,
) -> bool {
    if let (Some(session_url), Some(current_url)) = (repo_url, launch_repo_url)
        && session_url == current_url
    {
        return true;
    }

    let Some(cwd) = cwd else {
        return false;
    };
    let session_path = Path::new(cwd);
    if let (Some(session_repo_name), Some(current_repo_name)) =
        (repo_family_name(session_path), launch_repo_name)
        && session_repo_name == current_repo_name
    {
        return true;
    }
    if session_path == launch_cwd {
        return true;
    }
    if let Some(root) = launch_repo_root {
        return session_path.starts_with(root);
    }
    session_path.starts_with(launch_cwd)
}

fn resolve_cached_repo_root(
    path: &str,
    cache: &mut HashMap<String, Option<String>>,
) -> Option<String> {
    if let Some(value) = cache.get(path) {
        return value.clone();
    }
    let resolved = resolve_git_toplevel(Path::new(path)).map(|value| value.display().to_string());
    cache.insert(path.to_string(), resolved.clone());
    resolved
}

fn resolve_git_toplevel(path: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

fn resolve_git_origin_url(path: &Path) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["config", "--get", "remote.origin.url"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn repo_family_name(path: &Path) -> Option<String> {
    let components: Vec<String> = path
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect();
    if let Some(index) = components
        .iter()
        .position(|component| component == ".worktrees")
    {
        return components.get(index + 1).cloned();
    }
    path.file_name()
        .map(|value| value.to_string_lossy().to_string())
}

fn short_text(value: Option<&str>) -> Option<String> {
    let text = value?
        .split_whitespace()
        .take(12)
        .collect::<Vec<_>>()
        .join(" ");
    if text.is_empty() { None } else { Some(text) }
}

fn compact_label(value: &str) -> String {
    let flattened = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = flattened.chars();
    let compact: String = chars.by_ref().take(92).collect();
    if chars.next().is_some() {
        format!("{compact}...")
    } else {
        compact
    }
}

fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    use anyhow::Result;
    use rusqlite::Connection;
    use tempfile::tempdir;

    use crate::model::ScopeMode;

    use super::{GraphScanner, ScanConfig, match_workspace};

    #[test]
    fn scan_reconstructs_lineage_and_active_state() -> Result<()> {
        let temp = tempdir()?;
        let codex_home = temp.path().join(".codex");
        fs::create_dir_all(codex_home.join("sessions"))?;
        create_state_db(&codex_home.join("state_5.sqlite"))?;
        create_logs_db(&codex_home.join("logs_1.sqlite"))?;
        fs::write(
            codex_home.join("session_index.jsonl"),
            "{\"id\":\"child-thread\",\"thread_name\":\"Child session\",\"updated_at\":1}\n",
        )?;

        let rollout_path = codex_home.join("sessions/rollout.jsonl");
        fs::write(
            &rollout_path,
            r#"{"type":"session_meta","payload":{"id":"child-thread","forked_from_id":"root-thread","cwd":"/tmp/work","git":{"branch":"feature/test","commit_hash":"abc123","repository_url":"https://example.com/repo.git"}}}"#,
        )?;

        let config = ScanConfig {
            codex_home,
            launch_cwd: PathBuf::from("/tmp/work"),
            launch_repo_root: Some(PathBuf::from("/tmp/work")),
            launch_repo_name: Some("work".to_string()),
            launch_repo_url: None,
            initial_scope: ScopeMode::Current,
        };
        let scanner = GraphScanner::new(config);
        let graph = scanner.scan()?;

        assert_eq!(graph.nodes.len(), 1);
        let node = &graph.nodes[0];
        assert_eq!(node.id, "child-thread");
        assert_eq!(node.parent_id.as_deref(), Some("root-thread"));
        assert!(node.workspace_match);
        assert_eq!(node.git_branch.as_deref(), Some("feature/test"));
        assert_eq!(graph.warnings.len(), 1);

        Ok(())
    }

    #[test]
    fn workspace_match_accepts_same_repo_family_paths() {
        assert!(match_workspace(
            Some("/home/user/gitclones/codex-vision"),
            None,
            Path::new("/home/user/gitclones/.worktrees/codex-vision/feature/live-graph"),
            Some(Path::new(
                "/home/user/gitclones/.worktrees/codex-vision/feature/live-graph"
            )),
            Some("codex-vision"),
            None,
        ));
    }

    fn create_state_db(path: &Path) -> Result<()> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            r#"
            CREATE TABLE threads (
              id TEXT PRIMARY KEY,
              rollout_path TEXT,
              created_at INTEGER,
              updated_at INTEGER,
              source TEXT,
              model_provider TEXT,
              cwd TEXT,
              title TEXT,
              sandbox_policy TEXT,
              approval_mode TEXT,
              tokens_used INTEGER,
              has_user_event INTEGER,
              archived INTEGER,
              archived_at INTEGER,
              git_sha TEXT,
              git_branch TEXT,
              git_origin_url TEXT,
              cli_version TEXT,
              first_user_message TEXT,
              agent_nickname TEXT,
              agent_role TEXT,
              memory_mode TEXT
            );
            "#,
        )?;
        conn.execute(
            "INSERT INTO threads (id, rollout_path, updated_at, cwd, title, archived, first_user_message) VALUES (?1, ?2, 100, '/tmp/work', '', 0, 'hello world')",
            ("child-thread", "/tmp/fake"),
        )?;
        conn.execute(
            "UPDATE threads SET rollout_path = ?1 WHERE id = 'child-thread'",
            [path
                .parent()
                .unwrap()
                .join("sessions/rollout.jsonl")
                .display()
                .to_string()],
        )?;
        Ok(())
    }

    fn create_logs_db(path: &Path) -> Result<()> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            r#"
            CREATE TABLE logs (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              ts INTEGER,
              ts_nanos INTEGER,
              level TEXT,
              target TEXT,
              message TEXT,
              module_path TEXT,
              file TEXT,
              line INTEGER,
              thread_id TEXT,
              process_uuid TEXT,
              estimated_bytes INTEGER
            );
            "#,
        )?;
        conn.execute(
            "INSERT INTO logs (ts, thread_id, process_uuid) VALUES (1, 'child-thread', 'pid:999999:test')",
            [],
        )?;
        Ok(())
    }
}
