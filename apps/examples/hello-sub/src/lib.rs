//! Hello-sub — demo event subscriber for Jetstream inter-app pub-sub.

pub mod api;
pub mod peer;
pub mod peer_config;
pub mod store;

::app_runtime::declare_app_runtime!("hello-sub");

use std::sync::Arc;

use ::app_runtime::status_stream::SummaryStream;

use crate::store::FeedStore;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<FeedStore>,
    pub card_stream: Arc<SummaryStream>,
}
