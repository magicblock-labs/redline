use std::error::Error;

use bytes::BytesMut;
use json::Value;
use solana::{pubkey::Pubkey, signature::Signature};
use tokio::{net::TcpStream, sync::mpsc::Receiver};
use tokio_native_tls::{native_tls, TlsConnector, TlsStream};
use url::Url;
use websocket::{Message, NoExtEncoder};

pub struct WebsocketClient {
    writer: websocket::Sender<TlsStream<TcpStream>, NoExtEncoder>,
    rx: Receiver<BytesMut>,
}

pub struct WsNotification {
    pub id: u64,
    pub ty: WsNotificationType,
}
pub enum WsNotificationType {
    Signature,
    Account,
    Result(u64),
}

pub struct WsSubscription {
    id: u64,
    ty: WsSubscriptionType,
}

pub enum WsSubscriptionType {
    Signature(Signature),
    Account(Pubkey),
}

impl WsSubscription {
    pub fn signature(sig: Signature, id: u64) -> Self {
        Self {
            id,
            ty: WsSubscriptionType::Signature(sig),
        }
    }
    pub fn account(acc: Pubkey, id: u64) -> Self {
        Self {
            id,
            ty: WsSubscriptionType::Account(acc),
        }
    }
}

macro_rules! crash {
    ($msg: literal $(,$arg: expr)*) => {
        eprintln!($msg);
        std::process::exit(1);
    };
}
impl WebsocketClient {
    pub async fn connect(url: Url) -> Result<Self, Box<dyn Error>> {
        let port = url.port_or_known_default().unwrap_or_default();
        let host = url.host_str().unwrap_or_default();
        let host_and_port = format!("{host}:{port}");
        let stream = TcpStream::connect(host_and_port).await?;
        let connector = native_tls::TlsConnector::new().unwrap();
        let connector = TlsConnector::from(connector);
        let stream = connector.connect(host, stream).await.unwrap();
        let client = websocket::subscribe(Default::default(), stream, url.as_str()).await?;
        let inner = client.into_websocket();
        let (writer, mut reader) = inner.split().unwrap();
        let (tx, rx) = tokio::sync::mpsc::channel(1024);
        tokio::task::spawn(async move {
            loop {
                let mut buffer = BytesMut::with_capacity(1024);
                match reader.read(&mut buffer).await {
                    Err(e) => {
                        crash!("error reading from ws: {e}");
                    }
                    Ok(Message::Text) => {
                        if tx.send(buffer).await.is_err() {
                            return;
                        }
                    }
                    _ => (),
                }
            }
        });
        Ok(Self { writer, rx })
    }

    pub async fn subscribe(&mut self, sub: WsSubscription) {
        let id = sub.id;
        let payload = match sub.ty {
            WsSubscriptionType::Signature(sig) => {
                format!(
                    r#"{{"jsonrpc":"2.0","id":{id},"method":"signatureSubscribe","params":["{sig}"]}}"#
                )
            }
            WsSubscriptionType::Account(key) => {
                let key = key.to_string();
                format!(
                    r#"{{
                        "jsonrpc":"2.0","id":{id},"method":"accountSubscribe",
                        "params":["{key}",{{"encoding":"base64"}}]
                    }}"#
                )
            }
        };
        if let Err(e) = self.writer.write_text(payload).await {
            crash!("failed to send ws subscription: {e}");
        }
    }

    pub async fn next(&mut self) -> WsNotification {
        let buffer = self.rx.recv().await.unwrap();
        let Ok(payload) = json::from_slice::<Value>(&buffer) else {
            let payload = String::from_utf8_lossy(&buffer);
            crash!("received garbage on websocket: {payload}");
        };
        if let Some(rid) = payload.get("result").and_then(Value::as_u64) {
            let Some(id) = payload.get("id").and_then(Value::as_u64) else {
                crash!("received garbage json on websocket: {payload:?}");
            };
            let ty = WsNotificationType::Result(rid);
            return WsNotification { id, ty };
        }
        let Some(method) = payload.get("method").and_then(Value::as_str) else {
            crash!("received garbage json on websocket: {payload:?}");
        };
        let Some(id) = payload
            .get("params")
            .and_then(|v| v.get("subscription"))
            .and_then(Value::as_u64)
        else {
            crash!("received garbage json on websocket: {payload:?}");
        };
        let ty = match method {
            "signatureSubscribe" => WsNotificationType::Signature,
            "accountSubscribe" => {
                //let Some(data) = payload
                //    .get("params")
                //    .and_then(|v| v.get("result"))
                //    .and_then(|v| v.get("value"))
                //    .and_then(|v| v.get("data"))
                //    .and_then(|v| v.get(0))
                //    .and_then(Value::as_str)
                //else {
                //    crash!("received garbage account notification on websocket: {payload:?}");
                //};
                //let Ok(data) = BASE64_STANDARD.decode(data) else {
                //    crash!("received undecodable account data: {data:?}");
                //};
                WsNotificationType::Account
            }
            _ => {
                crash!("received garbage method on websocket: {payload:?}");
            }
        };
        WsNotification { id, ty }
    }
}
