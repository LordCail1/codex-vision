use std::{net::SocketAddr, sync::Arc};

use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::get,
};
use futures_util::StreamExt;
use serde_json::json;
use tokio::{net::TcpListener, sync::watch};

use crate::{
    model::{GraphEvent, GraphState},
    service::GraphService,
};

const INDEX_HTML: &str = include_str!("../assets/index.html");
const APP_JS: &str = include_str!("../assets/app.js");

#[derive(Clone)]
struct WebState {
    graph_service: Arc<GraphService>,
}

pub async fn run_server(graph_service: Arc<GraphService>, port: Option<u16>) -> Result<SocketAddr> {
    let bind_port = port.unwrap_or(0);
    let listener = TcpListener::bind(("127.0.0.1", bind_port))
        .await
        .with_context(|| format!("failed to bind 127.0.0.1:{bind_port}"))?;
    let address = listener.local_addr()?;

    let app = Router::new()
        .route("/", get(index))
        .route("/app.js", get(script))
        .route("/favicon.ico", get(favicon))
        .route("/api/snapshot", get(snapshot))
        .route("/ws", get(websocket))
        .with_state(WebState { graph_service });

    tokio::spawn(async move {
        if let Err(err) = axum::serve(listener, app).await {
            eprintln!("server error: {err}");
        }
    });

    Ok(address)
}

pub fn launch_url(address: SocketAddr) -> String {
    format!("http://127.0.0.1:{}/", address.port())
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn script() -> impl IntoResponse {
    (
        [("content-type", "application/javascript; charset=utf-8")],
        APP_JS,
    )
}

async fn favicon() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn snapshot(State(state): State<WebState>) -> Json<GraphState> {
    Json(state.graph_service.snapshot())
}

async fn websocket(ws: WebSocketUpgrade, State(state): State<WebState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| stream_graph(socket, state.graph_service.subscribe()))
}

async fn stream_graph(mut socket: WebSocket, mut state_rx: watch::Receiver<GraphState>) {
    let initial = GraphEvent::Snapshot {
        state: state_rx.borrow().clone(),
    };
    if send_event(&mut socket, &initial).await.is_err() {
        return;
    }

    loop {
        tokio::select! {
            result = state_rx.changed() => {
                if result.is_err() {
                    break;
                }
                let event = GraphEvent::Snapshot {
                    state: state_rx.borrow().clone(),
                };
                if send_event(&mut socket, &event).await.is_err() {
                    break;
                }
            }
            maybe_message = socket.next() => {
                match maybe_message {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Text(_))) => {
                        let _ = socket.send(Message::Text(json!({"type":"ack"}).to_string().into())).await;
                    }
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
        }
    }
}

async fn send_event(socket: &mut WebSocket, event: &GraphEvent) -> Result<()> {
    let payload = serde_json::to_string(event)?;
    socket.send(Message::Text(payload.into())).await?;
    Ok(())
}
