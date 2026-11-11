//! Shared app runtime for app backends.
//!
//! Every app HTTP backend must depend on this crate and call
//! [`declare_app_runtime!`](crate::declare_app_runtime) — do not fork devres activate/register/release logic.

pub mod api_error;
pub mod devres;
pub mod hostname_ip;
pub mod hostname_mac;
pub mod resource_stream;
pub mod runtime;
pub mod startup;
pub mod status_stream;
