pub use crate::types::StreamEvent;
use futures_core::Stream;
use std::pin::Pin;

pub type BoxStream = Pin<Box<dyn Stream<Item = StreamEvent> + Send>>;
