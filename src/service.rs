use std::{path::PathBuf, sync::Arc, time::Duration};

use anyhow::Result;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use tokio::{sync::watch, task::JoinHandle};

use crate::{model::GraphState, scanner::GraphScanner};

pub struct GraphService {
    state_rx: watch::Receiver<GraphState>,
    _task: JoinHandle<()>,
}

impl GraphService {
    pub fn start(scanner: GraphScanner) -> Result<Self> {
        let initial = scanner.scan()?;
        let (state_tx, state_rx) = watch::channel(initial);
        let scanner = Arc::new(scanner);

        let task = tokio::spawn({
            let scanner = Arc::clone(&scanner);
            async move {
                let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
                let mut watcher = build_watcher(Arc::clone(&scanner), event_tx.clone()).ok();

                if watcher.is_none() {
                    let _ = event_tx.send(());
                }

                let mut interval = tokio::time::interval(Duration::from_secs(2));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

                loop {
                    tokio::select! {
                        _ = interval.tick() => {}
                        maybe_event = event_rx.recv() => {
                            if maybe_event.is_none() {
                                break;
                            }
                            while event_rx.try_recv().is_ok() {}
                        }
                    }

                    if let Ok(graph) = scanner.scan() {
                        let _ = state_tx.send(graph);
                    }
                }

                drop(watcher.take());
            }
        });

        Ok(Self {
            state_rx,
            _task: task,
        })
    }

    pub fn subscribe(&self) -> watch::Receiver<GraphState> {
        self.state_rx.clone()
    }

    pub fn snapshot(&self) -> GraphState {
        self.state_rx.borrow().clone()
    }
}

fn build_watcher(
    scanner: Arc<GraphScanner>,
    event_tx: tokio::sync::mpsc::UnboundedSender<()>,
) -> notify::Result<RecommendedWatcher> {
    let mut watcher = notify::recommended_watcher(move |_| {
        let _ = event_tx.send(());
    })?;

    for path in watch_targets(scanner.as_ref()) {
        if path.exists() {
            let mode = if path.is_dir() {
                RecursiveMode::Recursive
            } else {
                RecursiveMode::NonRecursive
            };
            let _ = watcher.watch(&path, mode);
        }
    }

    Ok(watcher)
}

fn watch_targets(scanner: &GraphScanner) -> Vec<PathBuf> {
    let codex_home = &scanner.config().codex_home;
    vec![
        codex_home.join("state_5.sqlite"),
        codex_home.join("logs_1.sqlite"),
        codex_home.join("session_index.jsonl"),
        codex_home.join("sessions"),
        codex_home.join("archived_sessions"),
    ]
}
