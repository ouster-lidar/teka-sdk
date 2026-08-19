//! Hello-sub — demo event subscriber for Jetstream inter-app pub-sub.

pub mod api;
pub mod peer;
pub mod peer_config;
pub mod store;

mc_portable_runtime::declare_mc_runtime!("hello-sub");

use std::sync::Arc;

use mc_portable_runtime::status_stream::SummaryStream;

use crate::store::FeedStore;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<FeedStore>,
    pub card_stream: Arc<SummaryStream>,
}
