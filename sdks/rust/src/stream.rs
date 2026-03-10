use crate::error::MotosanError;

/// A single streaming token event.
#[derive(Debug, Clone)]
pub struct StreamEvent {
    /// Text delta for this event.
    pub content: String,
    /// Whether this is the final event in the stream.
    pub done: bool,
}

/// Boxed async stream of stream events.
pub type BoxStream = std::pin::Pin<
    Box<dyn futures::Stream<Item = Result<StreamEvent, MotosanError>> + Send>,
>;
