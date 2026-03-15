use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result};
use serde::Serialize;
use sysinfo::{ProcessesToUpdate, System};

use crate::{
    model::SessionStatus,
    scanner::{GraphScanner, ScanConfig},
};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum CheckLevel {
    Ok,
    Warn,
    Error,
}

impl CheckLevel {
    fn label(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorCheck {
    pub name: String,
    pub level: CheckLevel,
    pub summary: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphSummary {
    pub total_sessions: usize,
    pub active_sessions: usize,
    pub archived_sessions: usize,
    pub orphaned_sessions: usize,
    pub warnings: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorReport {
    pub overall: CheckLevel,
    pub cwd: String,
    pub repo_root: Option<String>,
    pub repo_url: Option<String>,
    pub codex_home: String,
    pub tmux_attached: bool,
    pub codex_processes: usize,
    pub graph: Option<GraphSummary>,
    pub checks: Vec<DoctorCheck>,
}

pub fn run(json: bool, cwd_override: Option<PathBuf>) -> Result<()> {
    let report = generate_report(cwd_override)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_report(&report);
    }
    Ok(())
}

pub fn generate_report(cwd_override: Option<PathBuf>) -> Result<DoctorReport> {
    let config = ScanConfig::discover_in(crate::model::ScopeMode::Current, cwd_override)?;
    let graph = GraphScanner::new(config.clone()).scan();

    let mut checks = vec![
        path_check(
            "Codex home",
            &config.codex_home,
            true,
            "Codex home directory found",
            "Codex home directory is missing",
        ),
        path_check(
            "state_5.sqlite",
            &config.codex_home.join("state_5.sqlite"),
            false,
            "Thread metadata database found",
            "Thread metadata database is missing",
        ),
        path_check(
            "logs_1.sqlite",
            &config.codex_home.join("logs_1.sqlite"),
            false,
            "Logs database found",
            "Logs database is missing",
        ),
        path_check(
            "session_index.jsonl",
            &config.codex_home.join("session_index.jsonl"),
            false,
            "Session index found",
            "Session index is missing",
        ),
        path_check(
            "sessions/",
            &config.codex_home.join("sessions"),
            true,
            "Live session rollout directory found",
            "Live session rollout directory is missing",
        ),
        path_check(
            "archived_sessions/",
            &config.codex_home.join("archived_sessions"),
            true,
            "Archived session rollout directory found",
            "Archived session rollout directory is missing",
        ),
        command_check(
            "git",
            "git",
            &["--version"],
            CheckLevel::Error,
            "Git is required to infer repo context",
        ),
        command_check(
            "tmux",
            "tmux",
            &["-V"],
            CheckLevel::Warn,
            "tmux is optional and only used for pane labels",
        ),
        command_check(
            "cc",
            "cc",
            &["--version"],
            CheckLevel::Warn,
            "A C toolchain is only needed for local source builds; release binaries do not need it",
        ),
    ];

    let tmux_attached = env::var_os("TMUX").is_some();
    checks.push(DoctorCheck {
        name: "tmux session".to_string(),
        level: if tmux_attached {
            CheckLevel::Ok
        } else {
            CheckLevel::Warn
        },
        summary: if tmux_attached {
            "Current shell is attached to tmux".to_string()
        } else {
            "Current shell is not attached to tmux".to_string()
        },
        detail: Some(
            "tmux integration remains optional; this only affects pane/session labels".to_string(),
        ),
    });

    let codex_processes = count_codex_processes();
    checks.push(DoctorCheck {
        name: "codex processes".to_string(),
        level: if codex_processes > 0 {
            CheckLevel::Ok
        } else {
            CheckLevel::Warn
        },
        summary: if codex_processes > 0 {
            format!("Detected {codex_processes} live Codex processes")
        } else {
            "No live Codex processes detected".to_string()
        },
        detail: Some(
            "Active-session badges depend on live Codex processes plus local logs metadata"
                .to_string(),
        ),
    });

    let graph_summary = match graph {
        Ok(graph) => {
            let total_sessions = graph.nodes.len();
            let active_sessions = graph
                .nodes
                .iter()
                .filter(|node| node.status == SessionStatus::Active)
                .count();
            let archived_sessions = graph.nodes.iter().filter(|node| node.archived).count();
            let orphaned_sessions = graph
                .nodes
                .iter()
                .filter(|node| node.status == SessionStatus::Orphaned)
                .count();
            let summary = GraphSummary {
                total_sessions,
                active_sessions,
                archived_sessions,
                orphaned_sessions,
                warnings: graph.warnings.len(),
            };
            let level = if graph.warnings.is_empty() {
                CheckLevel::Ok
            } else {
                CheckLevel::Warn
            };
            checks.push(DoctorCheck {
                name: "graph scan".to_string(),
                level,
                summary: format!(
                    "Graph scan succeeded: {total_sessions} sessions, {active_sessions} active, {} warnings",
                    graph.warnings.len()
                ),
                detail: if graph.warnings.is_empty() {
                    None
                } else {
                    Some(graph.warnings.join(" | "))
                },
            });
            Some(summary)
        }
        Err(err) => {
            checks.push(DoctorCheck {
                name: "graph scan".to_string(),
                level: CheckLevel::Error,
                summary: "Graph scan failed".to_string(),
                detail: Some(err.to_string()),
            });
            None
        }
    };

    let overall = checks
        .iter()
        .map(|check| check.level)
        .max()
        .unwrap_or(CheckLevel::Ok);

    Ok(DoctorReport {
        overall,
        cwd: config.launch_cwd.display().to_string(),
        repo_root: config
            .launch_repo_root
            .as_ref()
            .map(|path| path.display().to_string()),
        repo_url: config.launch_repo_url.clone(),
        codex_home: config.codex_home.display().to_string(),
        tmux_attached,
        codex_processes,
        graph: graph_summary,
        checks,
    })
}

fn path_check(
    name: &str,
    path: &Path,
    expect_dir: bool,
    ok_summary: &str,
    missing_summary: &str,
) -> DoctorCheck {
    let exists = if expect_dir {
        path.is_dir()
    } else {
        path.is_file()
    };
    DoctorCheck {
        name: name.to_string(),
        level: if exists {
            CheckLevel::Ok
        } else {
            CheckLevel::Warn
        },
        summary: if exists {
            ok_summary.to_string()
        } else {
            missing_summary.to_string()
        },
        detail: Some(path.display().to_string()),
    }
}

fn command_check(
    name: &str,
    command: &str,
    args: &[&str],
    missing_level: CheckLevel,
    missing_detail: &str,
) -> DoctorCheck {
    match command_output(command, args) {
        Ok(output) => DoctorCheck {
            name: name.to_string(),
            level: CheckLevel::Ok,
            summary: format!("{name} is available"),
            detail: Some(output),
        },
        Err(err) => DoctorCheck {
            name: name.to_string(),
            level: missing_level,
            summary: format!("{name} is not available"),
            detail: Some(format!("{missing_detail}. {err}")),
        },
    }
}

fn command_output(command: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(command)
        .args(args)
        .output()
        .with_context(|| format!("failed to run {command}"))?;
    if !output.status.success() {
        anyhow::bail!("{command} exited with status {}", output.status);
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let text = if stdout.is_empty() { stderr } else { stdout };
    Ok(text)
}

fn count_codex_processes() -> usize {
    let mut system = System::new_all();
    system.refresh_processes(ProcessesToUpdate::All, true);
    system
        .processes()
        .values()
        .filter(|process| process.name().to_string_lossy().contains("codex"))
        .count()
}

fn print_report(report: &DoctorReport) {
    println!("Overall: {}", report.overall.label());
    println!("CWD: {}", report.cwd);
    if let Some(repo_root) = &report.repo_root {
        println!("Repo root: {repo_root}");
    }
    if let Some(repo_url) = &report.repo_url {
        println!("Repo URL: {repo_url}");
    }
    println!("Codex home: {}", report.codex_home);
    println!(
        "tmux attached: {}",
        if report.tmux_attached { "yes" } else { "no" }
    );
    println!("Live Codex processes: {}", report.codex_processes);
    if let Some(graph) = &report.graph {
        println!(
            "Graph: {} sessions, {} active, {} archived, {} orphaned, {} warnings",
            graph.total_sessions,
            graph.active_sessions,
            graph.archived_sessions,
            graph.orphaned_sessions,
            graph.warnings
        );
    }
    println!();
    println!("Checks:");
    for check in &report.checks {
        println!(
            "- {} {}: {}",
            check.level.label(),
            check.name,
            check.summary
        );
        if let Some(detail) = &check.detail {
            println!("  {}", detail);
        }
    }
}
