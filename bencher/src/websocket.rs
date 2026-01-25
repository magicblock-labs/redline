//! WebSocket subscription management with graceful shutdown.
//!
//! Manages multiple WS connections using round-robin distribution.
//! Handles subscription confirmations and routes notifications to
//! appropriate channels. Buffers out-of-order messages until subscription confirmed.

use core::{config::ConnectionSettings, types::Url};
use std::collections::{hash_map::Entry, HashMap};

use fastwebsockets::{handshake, CloseCode, Frame, OpCode, Payload, WebSocket};
use http_body_util::Empty;
use hyper::{
    header::{CONNECTION, UPGRADE},
    upgrade::Upgraded,
    Request,
};
use hyper_util::rt::{TokioExecutor, TokioIo};
use json::{Deserialize, JsonValueTrait, LazyValue};
use tokio::{
    net::TcpStream,
    sync::mpsc::{self, Receiver, Sender},
};

use crate::{BenchResult, ShutDown, ShutDownListener};

/// # WebSocket Worker
///
/// Manages a single WebSocket connection, handling subscriptions, message parsing,
/// and graceful shutdown. It is generic over the extractor function `F` and the
/// extracted value `V`.
pub struct WsWorker<F, V> {
    ws: WebSocket<TokioIo<Upgraded>>,
    rx: ShutDownReceiver<Subscription<V>>,
    subscriptions: HashMap<u64, Subscription<V>>,
    pending: HashMap<u64, Subscription<V>>,
    buffered: HashMap<u64, Payload<'static>>,
    extractor: F,
}

/// # Subscription
///
/// Represents a subscription to a WebSocket feed, including the channel for sending
/// back confirmations, the payload for the subscription request, and other metadata.
pub struct Subscription<V> {
    pub tx: Sender<(u64, V)>,
    pub payload: String,
    pub oneshot: bool,
    pub id: u64,
}

/// # WebSocket Pool
///
/// Manages a pool of `WsWorker` instances to handle multiple concurrent WebSocket connections,
/// distributing the load and providing a simple interface for obtaining a connection.
pub struct WebsocketPool<V> {
    connections: Vec<Sender<Subscription<V>>>,
    next: usize,
}

/// Subscription confirmation message from WebSocket server.
#[derive(Deserialize, Debug)]
struct Confirmation {
    result: u64,
    id: u64,
}

impl<F, V> WsWorker<F, V>
where
    F: Fn(LazyValue) -> Option<V> + Send + 'static,
    V: Send + 'static,
{
    /// # Initialize WebSocket Worker
    ///
    /// Establishes a WebSocket connection and spawns a new `WsWorker` to manage it.
    async fn init(
        url: &Url,
        extractor: F,
        shutdown: ShutDownListener,
    ) -> BenchResult<Sender<Subscription<V>>> {
        let stream = TcpStream::connect(url.address(true)).await?;
        let req = Request::builder()
            .method("GET")
            .uri(&url.0)
            .header("Host", url.host())
            .header(UPGRADE, "websocket")
            .header(CONNECTION, "upgrade")
            .header("Sec-WebSocket-Key", handshake::generate_key())
            .header("Sec-WebSocket-Version", "13")
            .body(Empty::<&[u8]>::new())?;
        let (ws, _) = handshake::client(&TokioExecutor::new(), req, stream).await?;
        let (tx, rx) = mpsc::channel(1);
        let rx = ShutDownReceiver { rx, shutdown };

        let this = Self {
            ws,
            rx,
            subscriptions: HashMap::default(),
            pending: HashMap::default(),
            extractor,
            buffered: HashMap::default(),
        };

        tokio::task::spawn_local(this.run());
        Ok(tx)
    }

    /// Handles an incoming WebSocket frame.
    async fn handle_frame(&mut self, frame: Frame<'static>) {
        if !matches!(frame.opcode, OpCode::Text) {
            return;
        }

        let mut payload = frame.payload;

        // Check if this is a subscription confirmation message
        if let Ok(confirmed) = json::from_slice::<Confirmation>(&payload) {
            match self.pending.entry(confirmed.id) {
                Entry::Occupied(e) => {
                    let sub = e.remove();
                    self.subscriptions.insert(confirmed.result, sub);
                    // Use buffered payload if available, otherwise skip processing
                    if let Some(pl) = self.buffered.remove(&confirmed.result) {
                        payload = pl;
                    } else {
                        return;
                    }
                }
                Entry::Vacant(_) => return,
            }
        }

        // Process notification message
        let Ok(params) = json::get(&*payload, ["params"]) else {
            return;
        };
        let Some(id) = params.get("subscription").as_u64() else {
            return;
        };
        let Some(result) = params.get("result") else {
            return;
        };
        let Some(extracted) = (self.extractor)(result) else {
            return;
        };
        let Some(sub) = self.subscriptions.get(&id) else {
            self.buffered.insert(id, payload);
            return;
        };

        if sub.tx.send((sub.id, extracted)).await.is_err() || sub.oneshot {
            self.subscriptions.remove(&id);
        }
    }

    /// Handles a new subscription request. Returns false if shutdown was requested.
    async fn handle_subscription(&mut self, sub: Option<Subscription<V>>) -> bool {
        let Some(mut sub) = sub else {
            let _ = self
                .ws
                .write_frame(Frame::close(CloseCode::Normal.into(), b""))
                .await;
            return false;
        };

        let payload = Payload::Owned(std::mem::take(&mut sub.payload).into_bytes());
        // TODO: reconnect on error
        self.ws
            .write_frame(Frame::text(payload))
            .await
            .expect("failed to send data websocket");
        self.ws.flush().await.expect("failed to flush ws stream");
        self.pending.insert(sub.id, sub);

        true
    }

    /// # Run WebSocket Worker
    ///
    /// The main loop for the `WsWorker`, handling incoming messages, subscriptions,
    /// and shutdown signals.
    async fn run(mut self) {
        loop {
            tokio::select! {
                Ok(frame) = self.ws.read_frame() => {
                    self.handle_frame(frame).await;
                }
                sub = self.rx.recv() => {
                    if !self.handle_subscription(sub).await {
                        break;
                    }
                }
            }
        }
    }
}

impl<V> WebsocketPool<V> {
    /// # New WebSocket Pool
    ///
    /// Creates a new `WebsocketPool` with the specified number of connections.
    pub async fn new<F>(
        config: &ConnectionSettings,
        extractor: F,
        shutdown: ShutDown,
    ) -> BenchResult<Self>
    where
        F: Fn(LazyValue) -> Option<V> + Send + 'static + Clone,
        V: Send + 'static,
    {
        let count = config.ws_connections_count;
        let mut connections = Vec::with_capacity(count);
        for _ in 0..count {
            let tx =
                WsWorker::init(&config.ephem_url, extractor.clone(), shutdown.listener()).await?;
            connections.push(tx);
        }
        Ok(Self {
            connections,
            next: 0,
        })
    }

    /// # Get Connection
    ///
    /// Returns a sender for one of the WebSocket connections in the pool, using a
    /// round-robin strategy to distribute the load.
    pub fn connection(&mut self) -> Sender<Subscription<V>> {
        let i = self.next;
        self.next = (self.next + 1) % self.connections.len();
        self.connections[i].clone()
    }
}

/// # Shutdown Receiver
///
/// A wrapper around a `mpsc::Receiver` that also listens for a shutdown signal,
/// allowing for graceful termination of the worker.
struct ShutDownReceiver<V> {
    rx: Receiver<V>,
    shutdown: ShutDownListener,
}

impl<V> ShutDownReceiver<V> {
    /// # Receive Message
    ///
    /// Asynchronously receives a message from the channel, returning `None` if the
    /// shutdown signal is received.
    async fn recv(&mut self) -> Option<V> {
        tokio::select! {
            Some(v) = self.rx.recv() => Some(v),
            _ = self.shutdown.recv() => None,
        }
    }
}
