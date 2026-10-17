//! One provisioning conversation with a phone: handshake, code proof, then
//! sealed requests. Pure and synchronous so it can be tested without a radio;
//! the async driver in `mod.rs` performs the Wi-Fi requests it emits.

use super::crypto::{self, Channel, Keys, LABEL_DEVICE, LABEL_PHONE};
use super::wifi::{Network, WifiState};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use serde::{Deserialize, Serialize};
use x25519_dalek::{PublicKey, StaticSecret};

pub const PROTO_VERSION: u32 = 1;

/// Plaintext envelope; everything after the handshake travels as `Sealed`.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Outer {
    Hello {
        v: u32,
        #[serde(rename = "pub")]
        key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        salt: Option<String>,
    },
    Auth {
        mac: String,
    },
    Sealed {
        c: String,
    },
    Error {
        code: ErrorCode,
        #[serde(default)]
        msg: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    BadMessage,
    UnsupportedVersion,
    BadCode,
    NotAuthenticated,
    Busy,
    NotConnected,
}

/// Phone → device, inside `Sealed`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Request {
    Scan,
    Connect {
        ssid: String,
        #[serde(default)]
        psk: String,
        #[serde(default)]
        hidden: bool,
    },
    Status,
    /// Phone has what it needs; leave provisioning mode.
    Done,
}

/// Device → phone, inside `Sealed`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Reply {
    Networks {
        list: Vec<Network>,
    },
    State {
        #[serde(flatten)]
        state: WifiState,
    },
    Bye,
    Error {
        code: ErrorCode,
        msg: String,
    },
}

#[derive(Debug, PartialEq)]
pub enum Event {
    /// Wire bytes to fragment and notify.
    Send(Vec<u8>),
    /// A phone started a handshake (time to show the code).
    Hello,
    /// An authenticated request for the driver.
    Request(Request),
    /// The phone proved it knows the code.
    Authenticated,
    /// Wrong code; the phone may start over with a new hello.
    AuthFailed,
    /// Unrecoverable (tampering or desync): drop the connection.
    Close,
}

enum Phase {
    AwaitHello,
    AwaitAuth(Keys),
    Open { rx: Box<Channel>, tx: Box<Channel> },
}

pub struct Session {
    code: String,
    phase: Phase,
    /// Fixed ephemeral key and salt for test vectors; random otherwise.
    fixed: Option<([u8; 32], [u8; 16])>,
}

fn encode(msg: &Outer) -> Vec<u8> {
    serde_json::to_vec(msg).unwrap_or_default()
}

fn error(code: ErrorCode, msg: &str) -> Event {
    Event::Send(encode(&Outer::Error {
        code,
        msg: msg.into(),
    }))
}

impl Session {
    /// `code` must already be normalised (`crypto::normalize_code`).
    pub fn new(code: String) -> Self {
        Self {
            code,
            phase: Phase::AwaitHello,
            fixed: None,
        }
    }

    #[cfg(test)]
    pub fn with_fixed(code: String, secret: [u8; 32], salt: [u8; 16]) -> Self {
        Self {
            code,
            phase: Phase::AwaitHello,
            fixed: Some((secret, salt)),
        }
    }

    /// Applies to the next handshake (after the panel shows a new code).
    pub fn set_code(&mut self, code: String) {
        self.code = code;
    }

    pub fn authenticated(&self) -> bool {
        matches!(self.phase, Phase::Open { .. })
    }

    pub fn handle(&mut self, raw: &[u8]) -> Vec<Event> {
        let msg: Outer = match serde_json::from_slice(raw) {
            Ok(m) => m,
            Err(e) => return vec![self.error_event(ErrorCode::BadMessage, &e.to_string())],
        };
        match msg {
            Outer::Hello { v, key, .. } => self.on_hello(v, &key),
            Outer::Auth { mac } => self.on_auth(&mac),
            Outer::Sealed { c } => self.on_sealed(&c),
            Outer::Error { code, msg } => {
                tracing::info!(?code, %msg, "provisioning client reported an error");
                Vec::new()
            }
        }
    }

    fn on_hello(&mut self, v: u32, key: &str) -> Vec<Event> {
        // A new hello always restarts the handshake (e.g. after a mistyped code).
        self.phase = Phase::AwaitHello;
        if v != PROTO_VERSION {
            return vec![error(
                ErrorCode::UnsupportedVersion,
                &format!("device speaks v{PROTO_VERSION}"),
            )];
        }
        let Some(phone_pub) = B64
            .decode(key)
            .ok()
            .and_then(|b| <[u8; 32]>::try_from(b).ok())
        else {
            return vec![error(
                ErrorCode::BadMessage,
                "pub must be 32 bytes of base64",
            )];
        };
        let (secret, salt) = match self.fixed {
            Some((s, salt)) => (StaticSecret::from(s), salt),
            None => (
                StaticSecret::random_from_rng(rand::rngs::OsRng),
                rand::random(),
            ),
        };
        let device_pub = PublicKey::from(&secret).to_bytes();
        let shared = match crypto::agree(&secret, &phone_pub) {
            Ok(s) => s,
            Err(e) => return vec![error(ErrorCode::BadMessage, &e.to_string())],
        };
        let keys = crypto::derive(&shared, &self.code, &salt, &phone_pub, &device_pub);
        self.phase = Phase::AwaitAuth(keys);
        let reply = Outer::Hello {
            v: PROTO_VERSION,
            key: B64.encode(device_pub),
            salt: Some(B64.encode(salt)),
        };
        vec![Event::Send(encode(&reply)), Event::Hello]
    }

    fn on_auth(&mut self, mac: &str) -> Vec<Event> {
        let Phase::AwaitAuth(keys) = &self.phase else {
            return vec![error(ErrorCode::NotAuthenticated, "send hello first")];
        };
        let got = B64.decode(mac).unwrap_or_default();
        if !crypto::verify_proof(&keys.confirm, LABEL_PHONE, &got) {
            self.phase = Phase::AwaitHello;
            return vec![
                error(ErrorCode::BadCode, "setup code does not match"),
                Event::AuthFailed,
            ];
        }
        let reply = Outer::Auth {
            mac: B64.encode(crypto::proof(&keys.confirm, LABEL_DEVICE)),
        };
        let rx = Channel::new(&keys.phone_to_device);
        let tx = Channel::new(&keys.device_to_phone);
        self.phase = Phase::Open {
            rx: Box::new(rx),
            tx: Box::new(tx),
        };
        vec![Event::Send(encode(&reply)), Event::Authenticated]
    }

    fn on_sealed(&mut self, c: &str) -> Vec<Event> {
        let Phase::Open { rx, .. } = &mut self.phase else {
            return vec![error(ErrorCode::NotAuthenticated, "handshake first")];
        };
        let plain = match B64.decode(c).ok().map(|ct| rx.open(&ct)) {
            Some(Ok(p)) => p,
            // The counters are now out of step; nothing more can be trusted.
            _ => return vec![Event::Close],
        };
        match serde_json::from_slice::<Request>(&plain) {
            Ok(req) => vec![Event::Request(req)],
            Err(e) => vec![self.error_event(ErrorCode::BadMessage, &e.to_string())],
        }
    }

    /// Errors are sealed once the channel is open, plaintext before.
    fn error_event(&mut self, code: ErrorCode, msg: &str) -> Event {
        if self.authenticated() {
            match self.seal(&Reply::Error {
                code,
                msg: msg.into(),
            }) {
                Some(b) => Event::Send(b),
                None => Event::Close,
            }
        } else {
            error(code, msg)
        }
    }

    /// Wire bytes for an authenticated reply; `None` before the handshake.
    pub fn seal(&mut self, reply: &Reply) -> Option<Vec<u8>> {
        let Phase::Open { tx, .. } = &mut self.phase else {
            return None;
        };
        let plain = serde_json::to_vec(reply).ok()?;
        let ct = tx.seal(&plain).ok()?;
        Some(encode(&Outer::Sealed { c: B64.encode(ct) }))
    }
}

/// The phone side, for tests and the cross-language test vector.
#[cfg(test)]
pub mod client {
    use super::*;

    pub struct Client {
        secret: StaticSecret,
        code: String,
        keys: Option<Keys>,
        pub tx: Option<Channel>,
        pub rx: Option<Channel>,
    }

    impl Client {
        pub fn new(secret: [u8; 32], code: &str) -> Self {
            Self {
                secret: StaticSecret::from(secret),
                code: code.into(),
                keys: None,
                tx: None,
                rx: None,
            }
        }

        pub fn public(&self) -> [u8; 32] {
            PublicKey::from(&self.secret).to_bytes()
        }

        pub fn hello(&self) -> Vec<u8> {
            encode(&Outer::Hello {
                v: PROTO_VERSION,
                key: B64.encode(self.public()),
                salt: None,
            })
        }

        /// Processes the device hello and returns the auth message.
        pub fn on_hello(&mut self, raw: &[u8]) -> Option<Vec<u8>> {
            let Outer::Hello { key, salt, .. } = serde_json::from_slice(raw).ok()? else {
                return None;
            };
            let device_pub: [u8; 32] = B64.decode(key).ok()?.try_into().ok()?;
            let salt = B64.decode(salt?).ok()?;
            let shared = crypto::agree(&self.secret, &device_pub).ok()?;
            let keys = crypto::derive(&shared, &self.code, &salt, &self.public(), &device_pub);
            let mac = B64.encode(crypto::proof(&keys.confirm, LABEL_PHONE));
            self.keys = Some(keys);
            Some(encode(&Outer::Auth { mac }))
        }

        pub fn on_auth(&mut self, raw: &[u8]) -> bool {
            let Ok(Outer::Auth { mac }) = serde_json::from_slice(raw) else {
                return false;
            };
            let Some(keys) = &self.keys else { return false };
            let ok = crypto::verify_proof(
                &keys.confirm,
                LABEL_DEVICE,
                &B64.decode(mac).unwrap_or_default(),
            );
            if ok {
                self.tx = Some(Channel::new(&keys.phone_to_device));
                self.rx = Some(Channel::new(&keys.device_to_phone));
            }
            ok
        }

        pub fn seal(&mut self, req: &Request) -> Option<Vec<u8>> {
            let ct = self
                .tx
                .as_mut()?
                .seal(&serde_json::to_vec(req).ok()?)
                .ok()?;
            Some(encode(&Outer::Sealed { c: B64.encode(ct) }))
        }

        pub fn open(&mut self, raw: &[u8]) -> Option<Reply> {
            let Outer::Sealed { c } = serde_json::from_slice(raw).ok()? else {
                return None;
            };
            let plain = self.rx.as_mut()?.open(&B64.decode(c).ok()?).ok()?;
            serde_json::from_slice(&plain).ok()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::client::Client;
    use super::*;
    use crate::provision::wifi::FailReason;

    const CODE: &str = "7K3M9QXR2D4F";

    fn sent(ev: &[Event]) -> Vec<u8> {
        ev.iter()
            .find_map(|e| match e {
                Event::Send(b) => Some(b.clone()),
                _ => None,
            })
            .unwrap_or_default()
    }

    fn handshake(code_typed: &str) -> (Session, Client, Vec<Event>) {
        let mut dev = Session::with_fixed(CODE.into(), [2u8; 32], [3u8; 16]);
        let mut phone = Client::new([1u8; 32], code_typed);
        let hello = sent(&dev.handle(&phone.hello()));
        let auth = phone.on_hello(&hello).unwrap_or_default();
        let ev = dev.handle(&auth);
        (dev, phone, ev)
    }

    #[test]
    fn full_exchange() {
        let (mut dev, mut phone, ev) = handshake(CODE);
        assert!(ev.contains(&Event::Authenticated));
        assert!(phone.on_auth(&sent(&ev)));
        let req = Request::Connect {
            ssid: "Home".into(),
            psk: "secret pass".into(),
            hidden: false,
        };
        let wire = phone.seal(&req).unwrap_or_default();
        // The password never appears on the air in clear.
        assert!(!String::from_utf8_lossy(&wire).contains("secret"));
        assert_eq!(dev.handle(&wire), vec![Event::Request(req)]);
        let state = WifiState::Failed {
            ssid: "Home".into(),
            reason: FailReason::AuthFailed,
            detail: String::new(),
        };
        let reply = dev
            .seal(&Reply::State {
                state: state.clone(),
            })
            .unwrap_or_default();
        assert_eq!(phone.open(&reply), Some(Reply::State { state }));
    }

    #[test]
    fn wrong_code_fails_and_can_retry() {
        let (mut dev, _, ev) = handshake("7K3M9QXR2D4G");
        assert!(ev.contains(&Event::AuthFailed));
        assert!(!dev.authenticated());
        // Sealed traffic is refused until a successful handshake.
        let reply = dev.handle(br#"{"t":"sealed","c":"AAAA"}"#);
        assert!(
            matches!(&reply[..], [Event::Send(b)] if String::from_utf8_lossy(b).contains("not_authenticated"))
        );
        let mut phone = Client::new([5u8; 32], CODE);
        let auth = phone
            .on_hello(&sent(&dev.handle(&phone.hello())))
            .unwrap_or_default();
        assert!(dev.handle(&auth).contains(&Event::Authenticated));
    }

    #[test]
    fn tampering_closes() {
        let (mut dev, mut phone, ev) = handshake(CODE);
        phone.on_auth(&sent(&ev));
        let mut wire = phone.seal(&Request::Scan).unwrap_or_default();
        let n = wire.len();
        wire[n - 4] = if wire[n - 4] == b'A' { b'B' } else { b'A' };
        assert_eq!(dev.handle(&wire), vec![Event::Close]);
    }

    #[test]
    fn rejects_garbage_and_wrong_version() {
        let mut dev = Session::new(CODE.into());
        assert!(String::from_utf8_lossy(&sent(&dev.handle(b"nope"))).contains("bad_message"));
        let v2 = br#"{"t":"hello","v":2,"pub":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="}"#;
        assert!(String::from_utf8_lossy(&sent(&dev.handle(v2))).contains("unsupported_version"));
    }

    /// Fixed inputs → fixed outputs. The Kotlin client checks the same values
    /// (app/android/.../ProtocolVectorTest.kt); update both if this changes.
    #[test]
    fn cross_language_vector() {
        let mut dev = Session::with_fixed(CODE.into(), [2u8; 32], [3u8; 16]);
        let mut phone = Client::new([1u8; 32], CODE);
        let phone_hello = phone.hello();
        let device_hello = sent(&dev.handle(&phone_hello));
        let phone_auth = phone.on_hello(&device_hello).unwrap_or_default();
        let device_auth = sent(&dev.handle(&phone_auth));
        assert!(phone.on_auth(&device_auth));
        let sealed_scan = phone.seal(&Request::Scan).unwrap_or_default();
        let sealed_bye = dev.seal(&Reply::Bye).unwrap_or_default();
        let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
        let dump = format!(
            "code={CODE}\nphone_secret=0101..01\ndevice_secret=0202..02\nsalt=0303..03\nphone_hello={}\ndevice_hello={}\nphone_auth={}\ndevice_auth={}\nsealed_scan={}\nsealed_bye={}",
            s(&phone_hello),
            s(&device_hello),
            s(&phone_auth),
            s(&device_auth),
            s(&sealed_scan),
            s(&sealed_bye),
        );
        println!("{dump}");
        assert_eq!(dump, include_str!("testdata/vector.txt").trim_end());
    }
}
