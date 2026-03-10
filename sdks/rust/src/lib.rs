//! # motosan-ai
//!
//! Multi-provider AI SDK with a unified interface for Anthropic, OpenAI, and MiniMax.
//!
//! ## Quick Start
//!
//! ```toml
//! [dependencies]
//! motosan-ai = { version = "0.1", features = ["anthropic"] }
//! tokio = { version = "1", features = ["full"] }
//! ```
//!
//! ```rust,no_run
//! use motosan_ai::{Client, Message, Provider};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let client = Client::builder()
//!         .provider(Provider::Anthropic)
//!         .api_key(std::env::var("ANTHROPIC_API_KEY")?)
//!         .build()?;
//!
//!     let response = client
//!         .chat(vec![Message::user("What is the capital of France?")])
//!         .await?;
//!
//!     println!("{}", response.content);
//!     Ok(())
//! }
//! ```

pub mod client;
pub mod error;
pub mod providers;
pub mod stream;
pub mod types;

pub use client::{Client, ClientBuilder};
pub use error::MotosanError;
pub use types::{ChatRequest, ChatResponse, Message, Role, StopReason, Tool, Usage};

/// Supported AI providers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Provider {
    #[cfg(feature = "anthropic")]
    Anthropic,
    #[cfg(feature = "openai")]
    OpenAI,
    #[cfg(feature = "minimax")]
    MiniMax,
}
