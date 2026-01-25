//! HTTP connection pooling with ready/busy queue management.
//!
//! Maintains a pool of persistent HTTP/1 or HTTP/2 connections to the validator.
//! Connections are moved between ready/busy queues based on their state,
//! providing O(1) connection acquisition.

use core::config::ConnectionSettings;
use core::types::{ConnectionType, Url};
use std::collections::VecDeque;
use std::future::Future;
use std::ops::{Deref, DerefMut};
use std::pin::Pin;

use http_body_util::BodyExt;
use hyper::body::{Bytes, Incoming};
use hyper::client::conn::http1::SendRequest as Http1Sender;
use hyper::client::conn::http2::SendRequest as Http2Sender;
use hyper::header::{HeaderValue, CONTENT_TYPE};
use hyper::{Method, Request, Response, Uri};
use hyper_util::rt::{TokioExecutor, TokioIo};
use json::LazyValue;
use tokio::net::TcpStream;

use crate::BenchResult;

/// # Inner Connection
///
/// An enum that abstracts over the different types of HTTP connections (HTTP/1 and HTTP/2).
pub enum InnerConnection {
    Http1(Http1Sender<String>),
    Http2(Http2Sender<String>),
}

/// # Connection
///
/// Represents a single HTTP connection to the target validator, capable of sending requests
/// and managing the underlying connection state.
pub struct Connection {
    inner: InnerConnection,
    uri: Uri,
}

/// # Connection Pool
///
/// Manages a pool of `Connection` instances with ready/busy queues for O(1) acquisition.
/// Ready connections are immediately available; busy connections are being prepared.
pub struct ConnectionPool {
    ready: VecDeque<Connection>,
    busy: VecDeque<Connection>,
}

impl ConnectionPool {
    /// # New Connection Pool
    ///
    /// Creates a new `ConnectionPool` with the specified number of connections.
    /// All connections start in the ready queue.
    pub async fn new(config: &ConnectionSettings) -> BenchResult<Self> {
        let count = config.http_connections_count;
        let mut ready = VecDeque::with_capacity(count);
        for _ in 0..count {
            let con = Connection::new(&config.ephem_url, config.http_connection_type).await?;
            ready.push_back(con);
        }
        Ok(Self {
            ready,
            busy: VecDeque::new(),
        })
    }

    /// # Get Connection
    ///
    /// Obtains a `ConnectionGuard` from the pool with O(1) access from ready queue.
    /// If no ready connections available, waits for a busy connection to become ready.
    pub async fn connection(&mut self) -> BenchResult<ConnectionGuard<'_>> {
        if let Some(con) = self.ready.pop_front() {
            return Ok(ConnectionGuard {
                con: Some(con),
                pool: self,
            });
        }

        // No ready connections - wait for a busy one to become ready
        let mut con = self
            .busy
            .pop_front()
            .expect("connection pool should not be empty");
        con.ready().await?;

        Ok(ConnectionGuard {
            con: Some(con),
            pool: self,
        })
    }
}

impl Connection {
    /// # Send Request
    ///
    /// Sends an HTTP request and returns a `ParsedResponse` that can be used to resolve
    /// the response and extract the desired value.
    pub fn send<F>(&mut self, mut request: Request<String>, extractor: F) -> ParsedResponse<F> {
        *request.uri_mut() = self.uri.clone();
        *request.method_mut() = Method::POST;
        let ct = HeaderValue::from_static("application/json");
        request.headers_mut().insert(CONTENT_TYPE, ct);
        match &mut self.inner {
            InnerConnection::Http1(sender) => ParsedResponse {
                pending: Box::pin(sender.send_request(request)),
                extractor,
            },
            InnerConnection::Http2(sender) => ParsedResponse {
                pending: Box::pin(sender.send_request(request)),
                extractor,
            },
        }
    }

    /// # Ready
    ///
    /// Waits until the connection is ready to send another request.
    async fn ready(&mut self) -> BenchResult<()> {
        match &mut self.inner {
            InnerConnection::Http1(sender) => sender.ready().await,
            InnerConnection::Http2(sender) => sender.ready().await,
        }
        .map_err(Into::into)
    }
}

impl Connection {
    /// # New Connection
    ///
    /// Establishes a new HTTP connection to the specified URL.
    pub async fn new(url: &Url, ty: ConnectionType) -> BenchResult<Self> {
        let stream = TcpStream::connect(url.address(false)).await?;
        stream.set_nodelay(true).expect("failed to set TCP nodelay");

        let io = TokioIo::new(stream);

        let inner = match ty {
            ConnectionType::Http1 => {
                let (sender, con) = hyper::client::conn::http1::handshake(io).await?;
                tokio::task::spawn_local(con);
                InnerConnection::Http1(sender)
            }
            ConnectionType::Http2 => {
                let (sender, con) = hyper::client::conn::http2::Builder::new(TokioExecutor::new())
                    .handshake(io)
                    .await?;
                tokio::task::spawn_local(con);
                InnerConnection::Http2(sender)
            }
        };
        Ok(Self {
            inner,
            uri: url.0.clone(),
        })
    }
}

/// # Connection Guard
///
/// A guard that provides exclusive access to a `Connection` and ensures that it is
/// returned to the ready queue when dropped.
pub struct ConnectionGuard<'a> {
    con: Option<Connection>,
    pool: &'a mut ConnectionPool,
}

impl Deref for ConnectionGuard<'_> {
    type Target = Connection;
    fn deref(&self) -> &Self::Target {
        self.con.as_ref().unwrap()
    }
}

impl DerefMut for ConnectionGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.con.as_mut().unwrap()
    }
}

impl Drop for ConnectionGuard<'_> {
    fn drop(&mut self) {
        if let Some(con) = self.con.take() {
            self.pool.ready.push_back(con);
        }
    }
}

/// # Parsed Response
///
/// A future that resolves to the parsed response of an HTTP request, with a generic
/// extractor function `F` to extract the desired value `V`.
pub struct ParsedResponse<F> {
    pending: Pin<Box<dyn Future<Output = hyper::Result<Response<Incoming>>> + Send>>,
    extractor: F,
}

impl<F, V> ParsedResponse<F>
where
    F: FnOnce(LazyValue) -> Option<V>,
{
    /// # Resolve Response
    ///
    /// Asynchronously resolves the HTTP response and applies the extractor function
    /// to parse the response body.
    pub async fn resolve(self) -> BenchResult<Option<V>> {
        let mut response = self.pending.await?;
        let mut data = Data::Empty;
        while let Some(next) = response.frame().await {
            let Ok(chunk) = next?.into_data() else {
                continue;
            };
            match &mut data {
                Data::Empty => data = Data::SingleChunk(chunk),
                Data::SingleChunk(first) => {
                    let mut buffer = Vec::with_capacity(first.len() + chunk.len());
                    buffer.extend_from_slice(first);
                    buffer.extend_from_slice(&chunk);
                    data = Data::MultiChunk(buffer);
                }
                Data::MultiChunk(buffer) => {
                    buffer.extend_from_slice(&chunk);
                }
            }
        }
        let result = json::get(data.as_ref(), ["result"]).inspect_err(|_| {
            tracing::error!("failed to parse response: {}", unsafe {
                std::str::from_utf8_unchecked(data.as_ref())
            })
        })?;
        Ok((self.extractor)(result))
    }
}
enum Data {
    Empty,
    SingleChunk(Bytes),
    MultiChunk(Vec<u8>),
}

impl AsRef<[u8]> for Data {
    fn as_ref(&self) -> &[u8] {
        match self {
            Data::Empty => &[],
            Data::SingleChunk(chunk) => &chunk,
            Data::MultiChunk(chunk) => &chunk,
        }
    }
}
