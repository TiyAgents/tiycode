use std::io;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use futures::{Sink, SinkExt, Stream, StreamExt};

use crate::acp::agent_handlers;
use crate::acp::AcpServerState;

const ACP_HTTP_LISTEN_ENV: &str = "TIY_ACP_HTTP_LISTEN";

pub async fn run_stdio(state: AcpServerState) -> Result<(), agent_client_protocol::Error> {
    agent_handlers::serve_connection(state, agent_client_protocol_tokio::Stdio::new()).await
}

pub fn spawn_http_server_if_configured(state: AcpServerState) {
    let Ok(raw_addr) = std::env::var(ACP_HTTP_LISTEN_ENV) else {
        return;
    };
    let raw_addr = raw_addr.trim();
    if raw_addr.is_empty() || raw_addr.eq_ignore_ascii_case("off") {
        return;
    }

    let addr = match raw_addr.parse::<SocketAddr>() {
        Ok(addr) if is_loopback(addr.ip()) => addr,
        Ok(addr) => {
            tracing::warn!(
                %addr,
                "refusing to start ACP HTTP server on a non-loopback address"
            );
            return;
        }
        Err(error) => {
            tracing::warn!(%raw_addr, error = %error, "invalid ACP HTTP listen address");
            return;
        }
    };

    tokio::spawn(async move {
        if let Err(error) = run_http_server(state, addr).await {
            tracing::error!(error = %error, "ACP HTTP server stopped with error");
        }
    });
}

async fn run_http_server(state: AcpServerState, addr: SocketAddr) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/acp", get(acp_ws_handler))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let actual_addr = listener.local_addr()?;
    tracing::info!(%actual_addr, "ACP HTTP WebSocket server listening on /acp");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn acp_ws_handler(
    State(state): State<AcpServerState>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| async move {
        if let Err(error) = serve_websocket(state, socket).await {
            tracing::warn!(error = %error, "ACP websocket connection ended with error");
        }
    })
}

async fn serve_websocket(
    state: AcpServerState,
    socket: WebSocket,
) -> Result<(), agent_client_protocol::Error> {
    let (sender, receiver) = socket.split();

    let incoming = receiver.filter_map(|message| async move {
        match message {
            Ok(Message::Text(text)) => Some(Ok(text.to_string())),
            Ok(Message::Binary(bytes)) => match String::from_utf8(bytes.to_vec()) {
                Ok(text) => Some(Ok(text)),
                Err(error) => Some(Err(io::Error::new(io::ErrorKind::InvalidData, error))),
            },
            Ok(Message::Close(_)) => None,
            Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => None,
            Err(error) => Some(Err(io::Error::new(io::ErrorKind::BrokenPipe, error))),
        }
    });

    let outgoing = futures::sink::unfold(sender, |mut sender, line: String| async move {
        sender
            .send(Message::Text(line.into()))
            .await
            .map_err(|error| io::Error::new(io::ErrorKind::BrokenPipe, error))?;
        Ok::<_, io::Error>(sender)
    });

    let outgoing: Pin<Box<dyn Sink<String, Error = io::Error> + Send>> = Box::pin(outgoing);
    let incoming: Pin<Box<dyn Stream<Item = io::Result<String>> + Send>> = Box::pin(incoming);

    agent_handlers::serve_connection(state, agent_client_protocol::Lines::new(outgoing, incoming))
        .await
}

fn is_loopback(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ip.is_loopback(),
        IpAddr::V6(ip) => ip.is_loopback(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_filter_accepts_only_local_addresses() {
        assert!(is_loopback("127.0.0.1".parse().unwrap()));
        assert!(is_loopback("::1".parse().unwrap()));
        assert!(!is_loopback("0.0.0.0".parse().unwrap()));
        assert!(!is_loopback("192.168.1.10".parse().unwrap()));
    }
}
