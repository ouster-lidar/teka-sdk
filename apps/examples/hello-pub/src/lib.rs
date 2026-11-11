//! Hello-pub — demo event publisher for Teka inter-app pub-sub.

pub mod api;
pub mod store;

::teka::declare_app_runtime!("hello-pub");

use std::sync::Arc;

use ::teka::app::status_stream::SummaryStream;

use crate::store::EventStore;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<EventStore>,
    pub card_stream: Arc<SummaryStream>,
}
