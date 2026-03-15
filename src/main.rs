use std::sync::Arc;

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
        #[arg(long)]
        port: Option<u16>,
        #[arg(long)]
        no_open: bool,
    },
    Tui {
        #[arg(long)]
        all: bool,
    },
    Serve {
        #[arg(long)]
        port: Option<u16>,
    },
    Snapshot {
        #[arg(long)]
        json: bool,
        #[arg(long)]
        all: bool,
    },
    Doctor {
        #[arg(long)]
        json: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Web { all, port, no_open } => {
            let scanner = GraphScanner::new(ScanConfig::discover(scope(all))?);
            let service = Arc::new(GraphService::start(scanner)?);
            let address = web::run_server(Arc::clone(&service), port).await?;
            let url = web::launch_url(address);
            println!("codex-vision web ready at {url}");
            if !no_open {
                let _ = webbrowser::open(&url);
            }
            tokio::signal::ctrl_c().await?;
        }
        Command::Tui { all } => {
            let scanner = GraphScanner::new(ScanConfig::discover(scope(all))?);
            let service = GraphService::start(scanner)?;
            tokio::task::spawn_blocking(move || tui::run(service.subscribe())).await??;
        }
        Command::Serve { port } => {
            let scanner = GraphScanner::new(ScanConfig::discover(ScopeMode::Current)?);
            let service = Arc::new(GraphService::start(scanner)?);
            let address = web::run_server(service, port).await?;
            println!(
                "codex-vision serve listening at {}",
                web::launch_url(address)
            );
            tokio::signal::ctrl_c().await?;
        }
        Command::Snapshot { json: _, all } => {
            let scanner = GraphScanner::new(ScanConfig::discover(scope(all))?);
            let graph = scanner.scan()?;
            println!("{}", serde_json::to_string_pretty(&graph)?);
        }
        Command::Doctor { json } => {
            doctor::run(json)?;
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
