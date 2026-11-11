//! Teka SDK for application runtimes, device clients, and signed TPK packages.

#[cfg(feature = "app")]
pub mod app;
#[cfg(feature = "client")]
pub mod client;
#[cfg(feature = "tpk")]
pub mod tpk;

#[cfg(feature = "app")]
#[doc(hidden)]
pub use tokio as __tokio;

#[cfg(feature = "app")]
/// Declare a crate-local `app_runtime` module bound to one app id.
#[macro_export]
macro_rules! declare_app_runtime {
    ($id:literal) => {
        /// Host app devres client.
        pub mod app_runtime {
            pub const APP_ID: &str = $id;

            pub fn app_base_url() -> String {
                $crate::app::runtime::app_base_url()
            }

            pub fn sensors_list_url() -> Option<String> {
                $crate::app::runtime::sensors_list_url()
            }

            pub fn dhcp_leases_url() -> Option<String> {
                $crate::app::runtime::dhcp_leases_url()
            }

            /// `None` → poll MC and `POST …/activate` with `{}` (see `devres` tests).
            pub fn resolve_bind(
                dev_override: Option<std::net::SocketAddr>,
            ) -> Result<std::net::SocketAddr, String> {
                $crate::app::startup::resolve_http_bind(APP_ID, dev_override)
            }

            pub fn activate_http_listen() -> Result<std::net::SocketAddr, String> {
                $crate::app::runtime::activate_http_listen(APP_ID)
            }

            pub async fn bind_listener(
                bind: std::net::SocketAddr,
            ) -> std::io::Result<$crate::__tokio::net::TcpListener> {
                $crate::app::startup::bind_http_listener(bind, APP_ID).await
            }

            pub fn notify_systemd_ready() {
                $crate::app::runtime::notify_ready();
            }

            pub fn sync_udp_ports(ports: &[u16]) -> Result<(), String> {
                $crate::app::runtime::sync_udp_ports(APP_ID, ports)
            }

            pub fn sync_tcp_ports(ports: &[u16]) -> Result<(), String> {
                $crate::app::runtime::sync_tcp_ports(APP_ID, ports)
            }

            pub fn sync_nftables(
                leases: &[$crate::app::devres::NftablesLease],
            ) -> Result<(), String> {
                $crate::app::runtime::sync_nftables(APP_ID, leases)
            }

            pub fn resource_availability(
            ) -> $crate::__tokio::sync::watch::Receiver<$crate::app::runtime::ResourceAvailability>
            {
                $crate::app::runtime::resource_availability()
            }

            pub fn release() {
                if let Err(error) = $crate::app::runtime::stop_and_release(APP_ID) {
                    eprintln!("app-runtime: {error}");
                }
            }

            pub async fn release_async() {
                let _ = $crate::__tokio::task::spawn_blocking(release).await;
            }

            pub async fn wait_shutdown() {
                $crate::app::startup::wait_terminate_signal().await;
            }
        }
    };
}
