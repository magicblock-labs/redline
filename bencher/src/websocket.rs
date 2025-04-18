use core::{ConnectionSettings, Url};
use std::collections::HashMap;

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

pub struct WsWorker<F, V> {
    ws: WebSocket<TokioIo<Upgraded>>,
    rx: ShutDownReceiver<Subscription<V>>,
    subscriptions: HashMap<u64, Subscription<V>>,
    inflights: HashMap<u64, Subscription<V>>,
    lost: HashMap<u64, Payload<'static>>,
    extractor: F,
}

pub struct Subscription<V> {
    pub tx: Sender<(u64, V)>,
    pub payload: String,
    pub oneshot: bool,
    pub id: u64,
}

pub struct WebsocketPool<V> {
    connections: Vec<Sender<Subscription<V>>>,
    next: usize,
}

impl<F, V> WsWorker<F, V>
where
    F: Fn(LazyValue) -> Option<V> + Send + 'static,
    V: Send + 'static,
{
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
            inflights: HashMap::default(),
            extractor,
            lost: HashMap::default(),
        };

        tokio::task::spawn_local(this.run());
        Ok(tx)
    }

    async fn run(mut self) {
        #[derive(Deserialize, Debug)]
        struct Confirmation {
            result: u64,
            id: u64,
        }
        loop {
            tokio::select! {
                Ok(frame) = self.ws.read_frame() => {
                    if !matches!(frame.opcode, OpCode::Text) {
                        continue;
                    }
                    let mut payload = frame.payload;
                    if let Ok(confirmed) = json::from_slice::<Confirmation>(&payload) {
                        let Some(sub) = self.inflights.remove(&confirmed.id) else {
                            continue;
                        };
                        self.subscriptions.insert(confirmed.result, sub);
                        if let Some(pl) = self.lost.remove(&confirmed.result) {
                            payload = pl;
                        } else {
                            continue;
                        }
                    }
                    let Ok(params) = json::get(&*payload, ["params"]) else {
                        continue;
                    };
                    let Some(id) = params.get("subscription").as_u64() else {
                        continue;
                    };
                    let Some(result) = params.get("result") else {
                        continue;
                    };
                    let Some(extracted) = (self.extractor)(result) else {
                        continue;
                    };
                    let Some(sub) = self.subscriptions.get(&id) else {
                        self.lost.insert(id, payload);
                        continue;
                    };
                    if sub.tx.send((sub.id, extracted)).await.is_err() || sub.oneshot {
                        self.subscriptions.remove(&id);
                    }
                }
                sub = self.rx.recv() => {
                    let Some(mut sub) = sub else {
                        let _ = self.ws
                            .write_frame(Frame::close(CloseCode::Normal.into(), b"")).await;
                        break;
                    };
                    let payload = Payload::Owned(std::mem::take(&mut sub.payload).into_bytes());
                    // TODO: reconnect on error
                    self.ws
                        .write_frame(Frame::text(payload))
                        .await
                        .expect("failed to send data websocket");
                    self.ws.flush().await.expect("failed to flush ws stream");
                    self.inflights.insert(sub.id, sub);
                }
            }
        }
    }
}

impl<V> WebsocketPool<V> {
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

    pub fn connection(&mut self) -> Sender<Subscription<V>> {
        let i = self.next;
        self.next = (self.next + 1) % self.connections.len();
        self.connections[i].clone()
    }
}

struct ShutDownReceiver<V> {
    rx: Receiver<V>,
    shutdown: ShutDownListener,
}

impl<V> ShutDownReceiver<V> {
    async fn recv(&mut self) -> Option<V> {
        if let Some(v) = self.rx.recv().await {
            return Some(v);
        }
        let _ = self.shutdown.recv().await;
        None
    }
}
