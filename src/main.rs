use std::{path::PathBuf, sync::Arc};

use anyhow::Result;
use clap::{Parser, Subcommand};
use codex_vision::{
    doctor,
    model::ScopeMode,
    scanner::{GraphScanner, ScanConfig},
    service::GraphService,
    tui, web,
};

#[derive(Debug, Parser)]
#[command(author, version, about = "Live Codex CLI session visualizer")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Web {
        #[arg(long)]
        all: bool,
        #[arg(long, value_name = "PATH")]
        cwd: Option<PathBuf>,
        #[arg(long)]
        port: Option<u16>,
        #[arg(long)]
        no_open: bool,
    },
    Tui {
        #[arg(long)]
        all: bool,
        #[arg(long, value_name = "PATH")]
        cwd: Option<PathBuf>,
    },
    Serve {
        #[arg(long, value_name = "PATH")]
        cwd: Option<PathBuf>,
        #[arg(long)]
        port: Option<u16>,
    },
    Snapshot {
        #[arg(long)]
        json: bool,
        #[arg(long)]
        all: bool,
        #[arg(long, value_name = "PATH")]
        cwd: Option<PathBuf>,
    },
    Doctor {
        #[arg(long)]
        json: bool,
        #[arg(long, value_name = "PATH")]
        cwd: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Web {
            all,
            cwd,
            port,
            no_open,
        } => {
            let scanner = GraphScanner::new(ScanConfig::discover_in(scope(all), cwd)?);
            let service = Arc::new(GraphService::start(scanner)?);
            let address = web::run_server(Arc::clone(&service), port).await?;
            let url = web::launch_url(address);
            println!("codex-vision web ready at {url}");
            if !no_open {
                let _ = webbrowser::open(&url);
            }
            tokio::signal::ctrl_c().await?;
        }
        Command::Tui { all, cwd } => {
            let scanner = GraphScanner::new(ScanConfig::discover_in(scope(all), cwd)?);
            let service = GraphService::start(scanner)?;
            tokio::task::spawn_blocking(move || tui::run(service.subscribe())).await??;
        }
        Command::Serve { cwd, port } => {
            let scanner = GraphScanner::new(ScanConfig::discover_in(ScopeMode::Current, cwd)?);
            let service = Arc::new(GraphService::start(scanner)?);
            let address = web::run_server(service, port).await?;
            println!(
                "codex-vision serve listening at {}",
                web::launch_url(address)
            );
            tokio::signal::ctrl_c().await?;
        }
        Command::Snapshot { json: _, all, cwd } => {
            let scanner = GraphScanner::new(ScanConfig::discover_in(scope(all), cwd)?);
            let graph = scanner.scan()?;
            println!("{}", serde_json::to_string_pretty(&graph)?);
        }
        Command::Doctor { json, cwd } => {
            doctor::run(json, cwd)?;
        }
    }

    Ok(())
}

fn scope(all: bool) -> ScopeMode {
    if all {
        ScopeMode::All
    } else {
        ScopeMode::Current
    }
}
