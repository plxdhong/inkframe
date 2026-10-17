//! mDNS/DNS-SD advertisement of the HTTP API as `_inkframe._tcp`, so the app
//! finds the frame on the LAN after provisioning. Addresses follow interface
//! changes automatically. IPv4 only: Android before 14 resolves a single
//! address, and an IPv6 link-local one (`fe80::…%wlan0`) is useless in a URL.

use crate::app::App;
use mdns_sd::{IfKind, ServiceDaemon, ServiceInfo};

pub const SERVICE_TYPE: &str = "_inkframe._tcp.local.";

/// Returns the daemon handle, which must be kept alive. Errors are logged
/// only: discovery is a convenience, the panel works without it.
pub fn advertise(app: &App) -> Option<ServiceDaemon> {
    if !app.config().mdns.enabled {
        return None;
    }
    let (_, port) = app.http_endpoint();
    let name = &app.identity.name;
    let props = [
        ("id", app.identity.id.as_str()),
        ("ver", env!("CARGO_PKG_VERSION")),
        ("api", "1"),
    ];
    let result = ServiceDaemon::new().and_then(|d| {
        d.disable_interface(IfKind::IPv6)?;
        let info = ServiceInfo::new(
            SERVICE_TYPE,
            name,
            &format!("{name}.local."),
            (),
            port,
            &props[..],
        )?
        .enable_addr_auto();
        d.register(info)?;
        Ok(d)
    });
    match result {
        Ok(d) => {
            tracing::info!(%name, port, "mDNS advertising {SERVICE_TYPE}");
            Some(d)
        }
        Err(e) => {
            tracing::warn!(error = %e, "mDNS advertisement failed");
            None
        }
    }
}
