//! Portable backend startup helpers — bind resolution, listener retry, shutdown.

use std::io;
use std::net::SocketAddr;

use socket2::{Domain, SockAddr, Socket, Type};

/// Dev override (`--bind`) or MC devres activate.
pub fn resolve_http_bind(
    app_id: &str,
    dev_override: Option<SocketAddr>,
) -> Result<SocketAddr, String> {
    match dev_override {
        Some(addr) => Ok(addr),
        None => crate::runtime::activate_http_listen(app_id),
    }
}

/// Bind loopback HTTP with SO_REUSEADDR/REUSEPORT; on `AddrInUse`, release devres and re-activate.
pub async fn bind_http_listener(
    bind: SocketAddr,
    app_id: &str,
) -> io::Result<tokio::net::TcpListener> {
    match bind_tcp_listener(bind).await {
        Ok(listener) => Ok(listener),
        Err(e) if e.kind() == io::ErrorKind::AddrInUse => {
            tracing::warn!(
                target: "mc_runtime",
                app_id,
                %bind,
                "bind in use; releasing devres and retrying"
            );
            let app_id = app_id.to_string();
            let release_id = app_id.clone();
            let _ = tokio::task::spawn_blocking(move || crate::runtime::release(&release_id)).await;
            let bind = crate::runtime::activate_http_listen(&app_id).map_err(io::Error::other)?;
            bind_tcp_listener(bind).await
        }
        Err(e) => Err(e),
    }
}

async fn bind_tcp_listener(addr: SocketAddr) -> io::Result<tokio::net::TcpListener> {
    let domain = if addr.is_ipv6() {
        Domain::IPV6
    } else {
        Domain::IPV4
    };
    let socket = Socket::new(domain, Type::STREAM, None)?;
    socket.set_reuse_address(true)?;
    #[cfg(all(unix, not(target_os = "solaris")))]
    socket.set_reuse_port(true)?;
    socket.set_nonblocking(true)?;
    socket.bind(&SockAddr::from(addr))?;
    socket.listen(128)?;
    tokio::net::TcpListener::from_std(socket.into())
}

/// Wait for Ctrl+C or SIGTERM (Unix).
pub async fn wait_terminate_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
