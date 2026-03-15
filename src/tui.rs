use std::{collections::HashMap, io, time::Duration};

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};
use tokio::sync::watch;

use crate::model::{GraphState, ScopeMode, SessionNode, SessionStatus};

pub fn run(state_rx: watch::Receiver<GraphState>) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_loop(&mut terminal, state_rx);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    mut state_rx: watch::Receiver<GraphState>,
) -> Result<()> {
    let mut app = TuiApp::new(state_rx.borrow().clone());

    loop {
        if state_rx.has_changed().unwrap_or(false) {
            let _ = state_rx.borrow_and_update();
            app.graph = state_rx.borrow().clone();
            app.clamp_selection();
        }

        terminal.draw(|frame| render(frame, &app))?;

        if event::poll(Duration::from_millis(200))? {
            let Event::Key(key) = event::read()? else {
                continue;
            };
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Char('q') => break,
                KeyCode::Down | KeyCode::Char('j') => app.next(),
                KeyCode::Up | KeyCode::Char('k') => app.previous(),
                KeyCode::Char('a') => {
                    app.scope = app.scope.toggle();
                    app.clamp_selection();
                }
                KeyCode::Char('x') => {
                    app.active_only = !app.active_only;
                    app.clamp_selection();
                }
                KeyCode::Char('z') => {
                    app.show_archived = !app.show_archived;
                    app.clamp_selection();
                }
                _ => {}
            }
        }
    }

    Ok(())
}

struct TuiApp {
    graph: GraphState,
    scope: ScopeMode,
    active_only: bool,
    show_archived: bool,
    selected: usize,
}

impl TuiApp {
    fn new(graph: GraphState) -> Self {
        Self {
            scope: graph.initial_scope,
            graph,
            active_only: false,
            show_archived: true,
            selected: 0,
        }
    }

    fn next(&mut self) {
        let max = self.visible_nodes().len().saturating_sub(1);
        self.selected = (self.selected + 1).min(max);
    }

    fn previous(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    fn clamp_selection(&mut self) {
        let max = self.visible_nodes().len().saturating_sub(1);
        self.selected = self.selected.min(max);
    }

    fn visible_nodes(&self) -> Vec<TreeLine<'_>> {
        let filtered: Vec<&SessionNode> = self
            .graph
            .nodes
            .iter()
            .filter(|node| self.scope == ScopeMode::All || node.workspace_match)
            .filter(|node| self.show_archived || !node.archived)
            .filter(|node| !self.active_only || node.status == SessionStatus::Active)
            .collect();

        let ids: HashMap<_, _> = filtered
            .iter()
            .map(|node| (node.id.as_str(), *node))
            .collect();
        let mut children: HashMap<&str, Vec<&SessionNode>> = HashMap::new();
        for node in &filtered {
            if let Some(parent) = node.parent_id.as_deref() {
                if ids.contains_key(parent) {
                    children.entry(parent).or_default().push(*node);
                }
            }
        }
        for values in children.values_mut() {
            values.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        }

        let mut roots: Vec<&SessionNode> = filtered
            .iter()
            .copied()
            .filter(|node| {
                node.parent_id
                    .as_deref()
                    .and_then(|parent| ids.get(parent))
                    .is_none()
            })
            .collect();
        roots.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));

        let mut lines = Vec::new();
        for root in roots {
            flatten(root, 0, &children, &mut lines);
        }
        lines
    }
}

#[derive(Clone, Copy)]
struct TreeLine<'a> {
    depth: usize,
    node: &'a SessionNode,
}

fn flatten<'a>(
    node: &'a SessionNode,
    depth: usize,
    children: &HashMap<&'a str, Vec<&'a SessionNode>>,
    output: &mut Vec<TreeLine<'a>>,
) {
    output.push(TreeLine { depth, node });
    if let Some(values) = children.get(node.id.as_str()) {
        for child in values {
            flatten(child, depth + 1, children, output);
        }
    }
}

fn render(frame: &mut Frame, app: &TuiApp) {
    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(frame.area());
    let lines = app.visible_nodes();
    let selected = lines.get(app.selected).copied();

    let title = format!(
        "Sessions [{}] {} {}",
        match app.scope {
            ScopeMode::Current => "current",
            ScopeMode::All => "all",
        },
        if app.active_only {
            "active-only"
        } else {
            "all-statuses"
        },
        if app.show_archived {
            "archived-on"
        } else {
            "archived-off"
        }
    );

    let items: Vec<ListItem> = lines
        .iter()
        .map(|line| {
            let indent = "  ".repeat(line.depth);
            let status = status_tag(line.node.status);
            let branch = line
                .node
                .git_branch
                .as_deref()
                .map(|value| format!("  [{value}]"))
                .unwrap_or_default();
            ListItem::new(Line::from(vec![
                Span::raw(format!("{indent}└─ ")),
                Span::styled(status, status_style(line.node.status)),
                Span::raw(" "),
                Span::styled(
                    line.node.display_name.clone(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(branch),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().title(title).borders(Borders::ALL))
        .highlight_style(Style::default().bg(Color::Rgb(32, 52, 71)));
    frame.render_stateful_widget(list, layout[0], &mut list_state(app.selected));

    let detail_lines = if let Some(line) = selected {
        detail_text(line.node)
    } else {
        vec![Line::from("No sessions match the current filters.")]
    };

    let detail = Paragraph::new(detail_lines)
        .block(Block::default().title("Details").borders(Borders::ALL))
        .wrap(Wrap { trim: false });
    frame.render_widget(detail, layout[1]);
}

fn detail_text(node: &SessionNode) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Name: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(node.display_name.clone()),
        ]),
        Line::from(vec![
            Span::styled("Status: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(status_tag(node.status), status_style(node.status)),
        ]),
        Line::from(format!("Thread: {}", node.id)),
        Line::from(format!(
            "Parent: {}",
            node.parent_id.as_deref().unwrap_or("root")
        )),
        Line::from(format!(
            "Branch: {}",
            node.git_branch.as_deref().unwrap_or("unknown")
        )),
        Line::from(format!(
            "Updated: {}",
            node.updated_at
                .map_or_else(|| "unknown".to_string(), |value| value.to_string())
        )),
        Line::from(format!(
            "Workspace match: {}",
            if node.workspace_match { "yes" } else { "no" }
        )),
        Line::from(format!("CWD: {}", node.cwd.as_deref().unwrap_or("unknown"))),
    ];

    if let Some(repo_root) = &node.repo_root {
        lines.push(Line::from(format!("Repo root: {repo_root}")));
    }
    if let Some(rollout_path) = &node.rollout_path {
        lines.push(Line::from(format!("Rollout: {rollout_path}")));
    }
    if let Some(proc_info) = &node.active_process {
        lines.push(Line::from(format!("PID: {}", proc_info.pid)));
    }
    if let Some(tmux) = &node.tmux_location {
        lines.push(Line::from(format!("tmux: {}", tmux.label())));
    }
    if let Some(repo_url) = &node.repo_url {
        lines.push(Line::from(format!("Repo URL: {repo_url}")));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(
        "Keys: q quit, a toggle scope, x active-only, z archived",
    ));
    lines
}

fn list_state(selected: usize) -> ratatui::widgets::ListState {
    let mut state = ratatui::widgets::ListState::default();
    state.select(Some(selected));
    state
}

fn status_tag(status: SessionStatus) -> &'static str {
    match status {
        SessionStatus::Active => "ACTIVE",
        SessionStatus::Idle => "IDLE",
        SessionStatus::Archived => "ARCH",
        SessionStatus::Orphaned => "ORPH",
    }
}

fn status_style(status: SessionStatus) -> Style {
    match status {
        SessionStatus::Active => Style::default().fg(Color::Rgb(64, 196, 140)),
        SessionStatus::Idle => Style::default().fg(Color::Rgb(244, 181, 98)),
        SessionStatus::Archived => Style::default().fg(Color::Rgb(153, 153, 153)),
        SessionStatus::Orphaned => Style::default().fg(Color::Rgb(230, 86, 86)),
    }
}
