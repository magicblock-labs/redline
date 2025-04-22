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

pub enum InnerConnection {
    Http1(Http1Sender<String>),
    Http2(Http2Sender<String>),
}

pub struct Connection {
    inner: InnerConnection,
    uri: Uri,
}

pub struct ConnectionPool {
    connections: VecDeque<Connection>,
}

impl ConnectionPool {
    pub async fn new(config: &ConnectionSettings) -> BenchResult<Self> {
        let count = config.http_connections_count;
        let mut connections = VecDeque::with_capacity(count);
        for _ in 0..count {
            let con = Connection::new(&config.ephem_url, config.http_connection_type).await?;
            connections.push_back(con);
        }
        Ok(Self { connections })
    }

    pub async fn connection(&mut self) -> BenchResult<ConnectionGuard<'_>> {
        let mut i = 0;
        loop {
            if let Some(mut con) = self.connections.pop_front() {
                if con.is_ready() {
                    return Ok(ConnectionGuard {
                        con: Some(con),
                        pool: &mut self.connections,
                    });
                }
                i += 1;
                if i >= self.connections.len() {
                    con.ready().await?;
                }
                self.connections.push_back(con);
            }
        }
    }
}

impl Connection {
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

    fn is_ready(&self) -> bool {
        match &self.inner {
            InnerConnection::Http1(sender) => sender.is_ready(),
            InnerConnection::Http2(sender) => sender.is_ready(),
        }
    }

    async fn ready(&mut self) -> BenchResult<()> {
        match &mut self.inner {
            InnerConnection::Http1(sender) => sender.ready().await,
            InnerConnection::Http2(sender) => sender.ready().await,
        }
        .map_err(Into::into)
    }
}

impl Connection {
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
            ConnectionType::Http2 { streams } => {
                let (sender, con) = hyper::client::conn::http2::Builder::new(TokioExecutor::new())
                    .max_concurrent_streams(streams)
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

pub struct ConnectionGuard<'a> {
    con: Option<Connection>,
    pool: &'a mut VecDeque<Connection>,
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
            self.pool.push_back(con);
        }
    }
}

pub struct ParsedResponse<F> {
    pending: Pin<Box<dyn Future<Output = hyper::Result<Response<Incoming>>> + Send>>,
    extractor: F,
}

impl<F, V> ParsedResponse<F>
where
    F: FnOnce(LazyValue) -> Option<V>,
{
    pub async fn resolve(self) -> BenchResult<Option<V>> {
        enum Data {
            Empty,
            SingleChunk(Bytes),
            MultiChunk(Vec<u8>),
        }
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
        let result = match &data {
            Data::Empty => return Ok(None),
            Data::SingleChunk(chunk) => json::get(chunk, ["result"]).inspect_err(|_| {
                eprintln!("failed to parse response: {}", unsafe {
                    std::str::from_utf8_unchecked(chunk)
                })
            })?,
            Data::MultiChunk(chunk) => json::get(chunk.as_slice(), ["result"])?,
        };
        Ok((self.extractor)(result))
    }
}
