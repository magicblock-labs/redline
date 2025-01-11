use std::error::Error;

use bytes::BytesMut;
use json::Value;
use solana::{pubkey::Pubkey, signature::Signature};
use tokio::net::TcpStream;
use websocket::{Message, NoExt};

pub struct WebsocketClient {
    inner: websocket::WebSocket<TcpStream, NoExt>,
    buffer: BytesMut,
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
    pub async fn connect(host: &str) -> Result<Self, Box<dyn Error>> {
        let stream = TcpStream::connect(host).await?;
        let client = websocket::subscribe(Default::default(), stream, "/").await?;
        let inner = client.into_websocket();
        let buffer = BytesMut::with_capacity(1024);
        Ok(Self { inner, buffer })
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
        if let Err(e) = self.inner.write_text(payload).await {
            crash!("failed to send ws subscription: {e}");
        }
    }

    pub async fn next(&mut self) -> WsNotification {
        self.buffer.clear();
        loop {
            match self.inner.read(&mut self.buffer).await {
                Err(e) => {
                    crash!("error reading from ws: {e}");
                }
                Ok(Message::Text) => break,
                _ => (),
            }
        }
        let Ok(payload) = json::from_slice::<Value>(&self.buffer) else {
            let payload = String::from_utf8_lossy(&self.buffer);
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
