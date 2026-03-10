use futures_core::Stream;
use std::pin::Pin;

#[derive(Debug, Clone)]
pub struct StreamEvent {
    pub content: String,
    pub done: bool,
}

pub type BoxStream = Pin<Box<dyn Stream<Item = StreamEvent> + Send>>;

