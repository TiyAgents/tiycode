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
    let raw_addr = std::env::var(ACP_HTTP_LISTEN_ENV).ok();
    let addr = match parse_http_listen_addr(raw_addr.as_deref()) {
        Ok(Some(addr)) => addr,
        Ok(None) => return,
        Err(error) => {
            tracing::warn!(error = %error, "invalid ACP HTTP listen configuration");
            return;
        }
    };

    tokio::spawn(async move {
        if let Err(error) = run_http_server(state, addr).await {
            tracing::error!(error = %error, "ACP HTTP server stopped with error");
        }
    });
}

fn parse_http_listen_addr(raw_addr: Option<&str>) -> Result<Option<SocketAddr>, String> {
    let Some(raw_addr) = raw_addr else {
        return Ok(None);
    };
    let raw_addr = raw_addr.trim();
    if raw_addr.is_empty() || raw_addr.eq_ignore_ascii_case("off") {
        return Ok(None);
    }

    let addr = raw_addr
        .parse::<SocketAddr>()
        .map_err(|error| format!("invalid ACP HTTP listen address '{raw_addr}': {error}"))?;
    if !is_loopback(addr.ip()) {
        return Err(format!(
            "refusing to start ACP HTTP server on non-loopback address {addr}"
        ));
    }

    Ok(Some(addr))
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
            // Axum handles websocket ping/pong control frames; ACP JSON-RPC
            // payloads are carried only by text/binary data frames.
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
        // Be conservative: IPv4-mapped IPv6 is not accepted as an ACP bind target.
        assert!(!is_loopback("::ffff:127.0.0.1".parse().unwrap()));
    }

    #[test]
    fn parse_http_listen_addr_disables_empty_and_off_values() {
        assert_eq!(parse_http_listen_addr(None).unwrap(), None);
        assert_eq!(parse_http_listen_addr(Some("")).unwrap(), None);
        assert_eq!(parse_http_listen_addr(Some("   ")).unwrap(), None);
        assert_eq!(parse_http_listen_addr(Some("off")).unwrap(), None);
        assert_eq!(parse_http_listen_addr(Some("OFF")).unwrap(), None);
        assert_eq!(parse_http_listen_addr(Some("Off")).unwrap(), None);
    }

    #[test]
    fn parse_http_listen_addr_accepts_loopback_addresses() {
        assert_eq!(
            parse_http_listen_addr(Some("127.0.0.1:0")).unwrap(),
            Some("127.0.0.1:0".parse().unwrap())
        );
        assert_eq!(
            parse_http_listen_addr(Some("[::1]:4321")).unwrap(),
            Some("[::1]:4321".parse().unwrap())
        );
    }

    #[test]
    fn parse_http_listen_addr_rejects_invalid_and_non_loopback_addresses() {
        assert!(parse_http_listen_addr(Some("127.0.0.1:not-a-port")).is_err());
        assert!(parse_http_listen_addr(Some("0.0.0.0:3000")).is_err());
        assert!(parse_http_listen_addr(Some("192.168.1.10:3000")).is_err());
        assert!(parse_http_listen_addr(Some("[::ffff:127.0.0.1]:3000")).is_err());
    }
}
