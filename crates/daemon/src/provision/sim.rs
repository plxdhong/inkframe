//! Development stand-in for the BLE peripheral: the same fragments over TCP,
//! so the phone app (or an emulator, via 10.0.2.2) can run the full protocol
//! before any radio exists.
//!
//! Each record is `kind: u8, len: u16 BE, payload`:
//! `1` read info (request empty, response JSON), `2` write to RX, `3` notify from TX.

use super::LinkIo;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

pub const KIND_INFO: u8 = 1;
pub const KIND_WRITE: u8 = 2;
pub const KIND_NOTIFY: u8 = 3;
/// Behaves like a phone that negotiated a 185-byte ATT MTU.
pub const SIM_MAX_FRAGMENT: usize = 182;

const READ_TIMEOUT: Duration = Duration::from_secs(200);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

pub async fn serve(listen: &str, info: Vec<u8>, links: mpsc::Sender<LinkIo>) -> anyhow::Result<()> {
    let listener = TcpListener::bind(listen).await?;
    tracing::info!(addr = %listen, "provisioning simulator listening");
    loop {
        let (sock, peer) = listener.accept().await?;
        tracing::info!(%peer, "simulator client connected");
        let (rx_tx, rx) = mpsc::channel::<Vec<u8>>(16);
        let (tx, mut tx_rx) = mpsc::channel::<Vec<u8>>(16);
        let link = LinkIo {
            rx,
            tx,
            max_fragment: Arc::new(AtomicUsize::new(SIM_MAX_FRAGMENT)),
        };
        // Like a peripheral, serve one phone at a time: wait for the slot.
        if links.send(link).await.is_err() {
            return Ok(());
        }
        let (mut rd, mut wr) = sock.into_split();
        let (out_tx, mut out_rx) = mpsc::channel::<(u8, Vec<u8>)>(16);
        let writer = tokio::spawn(async move {
            loop {
                let rec = tokio::select! {
                    r = out_rx.recv() => r,
                    n = tx_rx.recv() => n.map(|f| (KIND_NOTIFY, f)),
                };
                let Some((kind, payload)) = rec else { break };
                let len = u16::try_from(payload.len()).unwrap_or(u16::MAX);
                let mut buf = Vec::with_capacity(payload.len() + 3);
                buf.push(kind);
                buf.extend_from_slice(&len.to_be_bytes());
                buf.extend_from_slice(&payload[..len as usize]);
                if !matches!(
                    tokio::time::timeout(WRITE_TIMEOUT, wr.write_all(&buf)).await,
                    Ok(Ok(()))
                ) {
                    break;
                }
            }
        });
        let info = info.clone();
        tokio::spawn(async move {
            loop {
                let mut hdr = [0u8; 3];
                if !matches!(
                    tokio::time::timeout(READ_TIMEOUT, rd.read_exact(&mut hdr)).await,
                    Ok(Ok(_))
                ) {
                    break;
                }
                let len = u16::from_be_bytes([hdr[1], hdr[2]]) as usize;
                let mut payload = vec![0u8; len];
                if !matches!(
                    tokio::time::timeout(WRITE_TIMEOUT, rd.read_exact(&mut payload)).await,
                    Ok(Ok(_))
                ) {
                    break;
                }
                let ok = match hdr[0] {
                    KIND_INFO => out_tx.send((KIND_INFO, info.clone())).await.is_ok(),
                    KIND_WRITE => rx_tx.send(payload).await.is_ok(),
                    _ => false,
                };
                if !ok {
                    break;
                }
            }
            // Dropping rx_tx ends the session; the writer ends with it.
            writer.abort();
            tracing::info!(%peer, "simulator client gone");
        });
    }
}
