//! BLE peripheral over BlueZ (bluer). One service with three characteristics:
//! info (read), RX (write with response) and TX (notify). A phone session
//! begins when it subscribes to TX and ends when it unsubscribes or disconnects.

use super::{LinkIo, INFO_UUID, RX_UUID, SERVICE_UUID, TX_UUID};
use anyhow::Context;
use bluer::adv::Advertisement;
use bluer::gatt::local::{
    Application, Characteristic, CharacteristicNotify, CharacteristicNotifyMethod,
    CharacteristicRead, CharacteristicWrite, CharacteristicWriteMethod, ReqError, Service,
};
use futures_util::FutureExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

/// Where RX writes go: the current session's inbound channel, if any.
type Inbox = Arc<Mutex<Option<mpsc::Sender<Vec<u8>>>>>;

fn uuid(s: &str) -> anyhow::Result<bluer::Uuid> {
    s.parse().with_context(|| format!("bad uuid {s}"))
}

pub async fn serve(name: &str, info: Vec<u8>, links: mpsc::Sender<LinkIo>) -> anyhow::Result<()> {
    let session = tokio::time::timeout(Duration::from_secs(10), bluer::Session::new())
        .await
        .context("bluez session timed out")??;
    let adapter = session
        .default_adapter()
        .await
        .context("no Bluetooth adapter")?;
    adapter
        .set_powered(true)
        .await
        .context("power on adapter")?;
    tracing::info!(adapter = %adapter.name(), %name, "BLE provisioning advertising");

    let inbox: Inbox = Arc::new(Mutex::new(None));
    let max_fragment = Arc::new(AtomicUsize::new(super::frame::MIN_CHUNK + 1));

    let info_char = Characteristic {
        uuid: uuid(INFO_UUID)?,
        read: Some(CharacteristicRead {
            read: true,
            fun: Box::new(move |req| {
                let info = info.clone();
                async move {
                    // Long reads arrive with an offset.
                    let off = (req.offset as usize).min(info.len());
                    Ok(info[off..].to_vec())
                }
                .boxed()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };

    let rx_inbox = inbox.clone();
    let rx_mtu = max_fragment.clone();
    let rx_char = Characteristic {
        uuid: uuid(RX_UUID)?,
        write: Some(CharacteristicWrite {
            write: true,
            write_without_response: false,
            method: CharacteristicWriteMethod::Fun(Box::new(move |value, req| {
                rx_mtu.store(
                    (req.mtu as usize)
                        .saturating_sub(3)
                        .max(super::frame::MIN_CHUNK + 1),
                    Ordering::Relaxed,
                );
                let target = rx_inbox.lock().unwrap_or_else(|e| e.into_inner()).clone();
                async move {
                    // Writing before subscribing to TX is a protocol error.
                    let tx = target.ok_or(ReqError::NotPermitted)?;
                    tokio::time::timeout(Duration::from_secs(2), tx.send(value))
                        .await
                        .map_err(|_| ReqError::Failed)?
                        .map_err(|_| ReqError::Failed)
                }
                .boxed()
            })),
            ..Default::default()
        }),
        ..Default::default()
    };

    let tx_inbox = inbox.clone();
    let tx_mtu = max_fragment.clone();
    let tx_char = Characteristic {
        uuid: uuid(TX_UUID)?,
        notify: Some(CharacteristicNotify {
            notify: true,
            method: CharacteristicNotifyMethod::Fun(Box::new(move |mut notifier| {
                let inbox = tx_inbox.clone();
                let links = links.clone();
                let max_fragment = tx_mtu.clone();
                async move {
                    let (in_tx, in_rx) = mpsc::channel::<Vec<u8>>(16);
                    let (out_tx, mut out_rx) = mpsc::channel::<Vec<u8>>(16);
                    *inbox.lock().unwrap_or_else(|e| e.into_inner()) = Some(in_tx);
                    let link = LinkIo { rx: in_rx, tx: out_tx, max_fragment };
                    // One phone at a time; a second subscriber waits its turn.
                    if links.send(link).await.is_err() {
                        return;
                    }
                    tokio::spawn(async move {
                        loop {
                            tokio::select! {
                                f = out_rx.recv() => {
                                    let Some(f) = f else { break };
                                    let sent = tokio::time::timeout(Duration::from_secs(5), notifier.notify(f)).await;
                                    if !matches!(sent, Ok(Ok(()))) {
                                        break;
                                    }
                                }
                                _ = notifier.stopped() => break,
                            }
                        }
                        // Closing the inbox ends the session on the daemon side.
                        *inbox.lock().unwrap_or_else(|e| e.into_inner()) = None;
                    });
                }
                .boxed()
            })),
            ..Default::default()
        }),
        ..Default::default()
    };

    let app = Application {
        services: vec![Service {
            uuid: uuid(SERVICE_UUID)?,
            primary: true,
            characteristics: vec![info_char, rx_char, tx_char],
            ..Default::default()
        }],
        ..Default::default()
    };
    let _app_handle = adapter
        .serve_gatt_application(app)
        .await
        .context("register GATT application")?;

    let adv = Advertisement {
        advertisement_type: bluer::adv::Type::Peripheral,
        service_uuids: [uuid(SERVICE_UUID)?].into_iter().collect(),
        local_name: Some(name.to_string()),
        discoverable: Some(true),
        ..Default::default()
    };
    let _adv_handle = adapter.advertise(adv).await.context("start advertising")?;

    // The handles unregister everything when this future is dropped (aborted).
    std::future::pending::<()>().await;
    Ok(())
}
