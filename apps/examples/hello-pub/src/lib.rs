//! Hello-pub — demo event publisher for Jetstream inter-app pub-sub.

pub mod api;
pub mod store;

::app_runtime::declare_app_runtime!("hello-pub");

use std::sync::Arc;

use ::app_runtime::status_stream::SummaryStream;

use crate::store::EventStore;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<EventStore>,
    pub card_stream: Arc<SummaryStream>,
}
