//! Hello-pub — demo event publisher for Jetstream inter-app pub-sub.

pub mod api;
pub mod store;

mc_portable_runtime::declare_mc_runtime!("hello-pub");

use std::sync::Arc;

use mc_portable_runtime::status_stream::SummaryStream;

use crate::store::EventStore;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<EventStore>,
    pub card_stream: Arc<SummaryStream>,
}
