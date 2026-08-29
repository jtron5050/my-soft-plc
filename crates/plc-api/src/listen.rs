//! HTTP / HTTPS bind.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::connect_info::ConnectInfo;
use axum::http::Request;
use axum::Router;
use hyper::body::Incoming;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder;
use rustls::ServerConfig;
use sha2::{Digest, Sha256};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tower::Service;

use crate::auth::ClientCertFp;
use crate::error::ApiError;
use crate::state::AppState;
use crate::tls::{listen_mode, ListenMode};

/// Bind `cfg.rest.bind` and serve until the process is cancelled.
pub async fn serve(state: AppState) -> Result<(), ApiError> {
    let cfg = state.config.read().expect("config").clone();
    let addr: SocketAddr = cfg
        .rest
        .bind
        .parse()
        .map_err(|e| ApiError::bad_request("config", format!("rest.bind: {e}")))?;
    let mode = listen_mode(&cfg)?;
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let router = crate::router(state);
    match mode {
        ListenMode::Http => {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
        }
        ListenMode::Https(tls) => {
            serve_tls(listener, router, tls).await?;
        }
    }
    Ok(())
}

async fn serve_tls(
    listener: TcpListener,
    router: Router,
    tls: Arc<ServerConfig>,
) -> Result<(), ApiError> {
    let acceptor = TlsAcceptor::from(tls);
    loop {
        let (sock, addr) = listener
            .accept()
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let acceptor = acceptor.clone();
        let router = router.clone();
        tokio::spawn(async move {
            let Ok(stream) = acceptor.accept(sock).await else {
                return;
            };
            let fp = stream
                .get_ref()
                .1
                .peer_certificates()
                .and_then(|certs| certs.first())
                .map(|c| {
                    let d = Sha256::digest(c.as_ref());
                    let mut out = [0u8; 32];
                    out.copy_from_slice(&d);
                    ClientCertFp(out)
                });
            let io = TokioIo::new(stream);
            let svc = ConnService { router, addr, fp };
            let _ = Builder::new(TokioExecutor::new())
                .serve_connection(io, hyper_util::service::TowerToHyperService::new(svc))
                .await;
        });
    }
}

#[derive(Clone)]
struct ConnService {
    router: Router,
    addr: SocketAddr,
    fp: Option<ClientCertFp>,
}

impl Service<Request<Incoming>> for ConnService {
    type Response = axum::response::Response;
    type Error = std::convert::Infallible;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Incoming>) -> Self::Future {
        let (mut parts, body) = req.into_parts();
        parts.extensions.insert(ConnectInfo(self.addr));
        if let Some(fp) = self.fp {
            parts.extensions.insert(fp);
        }
        let req = Request::from_parts(parts, axum::body::Body::new(body));
        let mut router = self.router.clone();
        Box::pin(async move { Ok(router.call(req).await.expect("infallible")) })
    }
}
