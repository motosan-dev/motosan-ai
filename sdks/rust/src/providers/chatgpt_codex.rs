use crate::retry::RetryPolicy;
use reqwest::Client;

/// Default endpoint for the ChatGPT-backend Responses API.
const CHATGPT_CODEX_URL: &str = "https://chatgpt.com/backend-api/codex/responses";
/// `originator` header value codex's CLI sends; settled GREEN by the spike.
const ORIGINATOR: &str = "codex_cli_rs";

// NOTE: `#[allow(dead_code)]` is for Task 1 ONLY (no `ProviderImpl` impl yet, so
// the fields/methods are not all read). It is removed in Task 4 when `stream`/`chat`
// land and consume them.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ChatGptCodexProvider {
    http: Client,
    access_token: String,
    account_id: String,
    model: String,
    base_url: String,
    retry_policy: RetryPolicy,
}

#[allow(dead_code)]
impl ChatGptCodexProvider {
    pub fn new(
        access_token: impl Into<String>,
        account_id: impl Into<String>,
        model: impl Into<String>,
        base_url: Option<String>,
    ) -> Self {
        Self {
            http: Client::new(),
            access_token: access_token.into(),
            account_id: account_id.into(),
            model: model.into(),
            base_url: base_url.unwrap_or_else(|| CHATGPT_CODEX_URL.to_string()),
            retry_policy: RetryPolicy::default(),
        }
    }

    pub fn with_retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    fn apply_auth(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        request
            .header("authorization", format!("Bearer {}", self.access_token))
            .header("chatgpt-account-id", &self.account_id)
            .header("originator", ORIGINATOR)
            .header("openai-beta", "responses=experimental")
            .header("accept", "text/event-stream")
    }

    fn url(&self) -> String {
        self.base_url.clone()
    }
}
