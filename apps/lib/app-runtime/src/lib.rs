//! Shared app runtime for app backends.
//!
//! Every app HTTP backend must depend on this crate and call
//! [`declare_app_runtime!`] — do not fork devres activate/register/release logic.

pub mod api_error;
pub mod devres;
pub mod hostname_ip;
pub mod hostname_mac;
pub mod resource_stream;
pub mod runtime;
pub mod startup;
pub mod status_stream;

/// Declare a crate-local `app_runtime` module bound to one app id.
#[macro_export]
macro_rules! declare_app_runtime {
    ($id:literal) => {
        /// Host app devres client.
        pub mod app_runtime {
            pub const APP_ID: &str = $id;

            pub fn app_base_url() -> String {
                $crate::runtime::app_base_url()
            }

            pub fn sensors_list_url() -> Option<String> {
                $crate::runtime::sensors_list_url()
            }

            pub fn dhcp_leases_url() -> Option<String> {
                $crate::runtime::dhcp_leases_url()
            }

            /// `None` → poll MC and `POST …/activate` with `{}` (see `devres` tests).
            pub fn resolve_bind(
                dev_override: Option<std::net::SocketAddr>,
            ) -> Result<std::net::SocketAddr, String> {
                $crate::startup::resolve_http_bind(APP_ID, dev_override)
            }

            pub fn activate_http_listen() -> Result<std::net::SocketAddr, String> {
                $crate::runtime::activate_http_listen(APP_ID)
            }

            pub async fn bind_listener(
                bind: std::net::SocketAddr,
            ) -> std::io::Result<tokio::net::TcpListener> {
                $crate::startup::bind_http_listener(bind, APP_ID).await
            }

            pub fn notify_systemd_ready() {
                $crate::runtime::notify_ready();
            }

            pub fn sync_udp_ports(ports: &[u16]) {
                $crate::runtime::sync_udp_ports(APP_ID, ports)
            }

            pub fn sync_tcp_ports(ports: &[u16]) {
                $crate::runtime::sync_tcp_ports(APP_ID, ports)
            }

            pub fn release() {
                $crate::runtime::release(APP_ID)
            }

            pub async fn release_async() {
                let _ = tokio::task::spawn_blocking(release).await;
            }

            pub async fn wait_shutdown() {
                $crate::startup::wait_terminate_signal().await;
            }
        }
    };
}
