//! Tiny loopback HTTP server used as the OAuth redirect target.
//!
//! Spawns a single-shot listener on `127.0.0.1:<random-port>`, returns the URL
//! to use as `redirect_uri`, and resolves once the browser hits the callback.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::time::Duration;

use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

/// Captured query parameters from the OAuth callback.
#[derive(Debug, Clone)]
pub struct CallbackParams {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

pub struct LoopbackServer {
    pub redirect_uri: String,
    pub wait_for_callback:
        Box<dyn std::future::Future<Output = Result<CallbackParams, String>> + Send + Unpin>,
}

/// Bind to a random local port, return the redirect_uri and a future that
/// resolves once the browser hits `/`. The server stops after the first
/// callback.
pub async fn start(timeout: Duration) -> Result<LoopbackServer, String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("bind: {e}"))?;
    let addr: SocketAddr = listener.local_addr().map_err(|e| e.to_string())?;
    let redirect_uri = format!("http://127.0.0.1:{}/oauth/callback", addr.port());

    let (tx, rx) = oneshot::channel::<CallbackParams>();
    let tx_for_handler = std::sync::Mutex::new(Some(tx));

    tokio::spawn(async move {
        // Accept exactly one connection.
        if let Ok((stream, _)) = listener.accept().await {
            let io = TokioIo::new(stream);
            let tx_for_handler = std::sync::Arc::new(tx_for_handler);
            let svc = service_fn(move |req: Request<Incoming>| {
                let tx_for_handler = tx_for_handler.clone();
                async move {
                    let q = req.uri().query().unwrap_or("");
                    let mut code = None;
                    let mut state = None;
                    let mut error = None;
                    for pair in q.split('&') {
                        let mut it = pair.splitn(2, '=');
                        let k = it.next().unwrap_or("");
                        let v = it.next().map(percent_decode).unwrap_or_default();
                        match k {
                            "code" => code = Some(v),
                            "state" => state = Some(v),
                            "error" => error = Some(v),
                            _ => {}
                        }
                    }

                    if let Ok(mut guard) = tx_for_handler.lock() {
                        if let Some(sender) = guard.take() {
                            let _ = sender.send(CallbackParams { code, state, error });
                        }
                    }

                    let body = "<!doctype html><html><body style=\"font-family:-apple-system,system-ui,sans-serif;padding:48px;text-align:center\"><h2>You can close this tab.</h2><p>Qruakpit received the sign-in response.</p></body></html>";
                    Ok::<_, Infallible>(
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "text/html; charset=utf-8")
                            .body(Full::new(Bytes::from(body)))
                            .unwrap(),
                    )
                }
            });

            let _ = hyper::server::conn::http1::Builder::new()
                .serve_connection(io, svc)
                .await;
        }
    });

    // Compose a future that times out after `timeout`.
    let wait = async move {
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(params)) => Ok(params),
            Ok(Err(_)) => Err("OAuth callback channel dropped".into()),
            Err(_) => Err("OAuth callback timed out".into()),
        }
    };
    let wait_for_callback: Box<
        dyn std::future::Future<Output = Result<CallbackParams, String>> + Send + Unpin,
    > = Box::new(Box::pin(wait));

    Ok(LoopbackServer {
        redirect_uri,
        wait_for_callback,
    })
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'+' {
            out.push(b' ');
            i += 1;
        } else if b == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
            out.push(b);
            i += 1;
        } else {
            out.push(b);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(10 + c - b'a'),
        b'A'..=b'F' => Some(10 + c - b'A'),
        _ => None,
    }
}
