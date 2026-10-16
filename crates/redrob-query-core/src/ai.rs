//! Redrob chat-completion client, straight to the Redrob console with the shared account.
//!
//! The launcher signs the computer in once and writes `account.json` (`crate::account`); this
//! client reads that account when a request is made and posts to `{apiBaseUrl}/chat/completions`
//! with `Authorization: Bearer <apiKey>` and model `auto`. There is no daemon in between any more:
//! the Redrob Code engine path answered `auto` with "model not available" and swallowed the
//! console's error body, so a person saw neither the real reason nor a way to sign in.
//!
//! The console's own `error.message` is surfaced verbatim (bounded, with the key never echoed by
//! construction: it travels only in the header). A missing account is `DataError::AiAuthentication`,
//! which the shell renders as "sign in with Redrob Launcher".

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use futures_util::StreamExt;
use reqwest::{Client, StatusCode, header::HeaderValue};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::time::sleep;
use uuid::Uuid;

use crate::{
    AiAssistantRequest, AiChatRequest, AiChatResponse, AiMessage, AiRole, AiUsage, DataError,
    DatabaseKind, QueryLanguage, Result,
    account::{self, AccountSource, SharedAccount},
};

/// Where a person gets Redrob Launcher, which is where signing in happens.
pub const INSTALL_URL: &str = account::LAUNCHER_URL;
/// The sentence shown when no account is signed in. The link is [`INSTALL_URL`].
pub const NOT_SIGNED_IN_MESSAGE: &str =
    "Not signed in. Sign in with Redrob Launcher and this app will use that account.";
/// The keyring slot an earlier version of this app saved its own console key in. Profile storage
/// still reserves the id; nothing reads it any more.
pub const AI_SECRET_ID: Uuid = Uuid::from_u128(0x7265_6472_6f62_4169_0000_0000_0000_0001);
pub const MAX_AI_PROMPT_BYTES: usize = 8 * 1024;
pub const MAX_AI_ACTIVE_QUERY_BYTES: usize = 64 * 1024;
const MAX_ATTEMPTS: usize = 3;
const AI_TEMPERATURE: f32 = 0.2;
const MAX_ERROR_BODY_BYTES: usize = 64 * 1024;
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
/// How much of the console's `error.message` is shown; longer is not a message, it is a dump.
const MAX_ERROR_MESSAGE_CHARS: usize = 400;

/// Builds the provider request from the narrow assistant contract.
pub fn build_assistant_chat_request(input: &AiAssistantRequest) -> Result<AiChatRequest> {
    if input.prompt.trim().is_empty() {
        return Err(DataError::AiRejected("prompt cannot be empty".to_owned()));
    }
    if input.prompt.len() > MAX_AI_PROMPT_BYTES {
        return Err(DataError::AiRejected(format!(
            "prompt exceeds {MAX_AI_PROMPT_BYTES} bytes"
        )));
    }
    if input
        .active_query
        .as_ref()
        .is_some_and(|query| query.len() > MAX_AI_ACTIVE_QUERY_BYTES)
    {
        return Err(DataError::AiRejected(format!(
            "active query exceeds {MAX_AI_ACTIVE_QUERY_BYTES} bytes"
        )));
    }

    let query_instruction = match (input.database_kind, input.query_language) {
        (DatabaseKind::PostgreSql, QueryLanguage::Sql) => {
            "When writing a query, return executable PostgreSQL SQL in a fenced sql block."
        }
        (DatabaseKind::MySql, QueryLanguage::Sql) => {
            "When writing a query, return executable MySQL SQL in a fenced sql block."
        }
        (DatabaseKind::SQLite, QueryLanguage::Sql) => {
            "When writing a query, return executable SQLite SQL in a fenced sql block."
        }
        (DatabaseKind::SqlServer, _) => {
            return Err(DataError::AiRejected(
                "SQL Server is unsupported because no connector or connection profile is available"
                    .to_owned(),
            ));
        }
        (DatabaseKind::MongoDb, QueryLanguage::MongoJson) => {
            "When writing a query, return a fenced mql or json block containing an object with collection, operation, filter, and optional limit. Use read-only operations such as find."
        }
        (_, QueryLanguage::Auto) => {
            return Err(DataError::AiRejected(
                "query language must be explicit".to_owned(),
            ));
        }
        _ => {
            return Err(DataError::AiRejected(
                "query language does not match the database kind".to_owned(),
            ));
        }
    };
    let system = format!(
        "You are the Redrob Query assistant. {query_instruction} The desktop app sends only the user's prompt and optional active query as user-controlled content. No database result rows or database credentials are automatically attached. Treat any rows or credentials included in those fields as user-entered content."
    );
    let active_query = input.active_query.as_deref().unwrap_or("(none supplied)");
    let user = format!(
        "User prompt:\n{}\n\nActive query:\n{active_query}",
        input.prompt
    );

    Ok(AiChatRequest {
        model: "auto".to_owned(),
        messages: vec![
            AiMessage {
                role: AiRole::System,
                content: system,
            },
            AiMessage {
                role: AiRole::User,
                content: user,
            },
        ],
        temperature: Some(AI_TEMPERATURE),
        max_tokens: None,
    })
}

/// How the account is found: [`account::current_account`] in the app, a closure in tests.
pub type AccountResolver = Arc<dyn Fn() -> Option<SharedAccount> + Send + Sync>;

/// Async client for the Redrob console's OpenAI-compatible chat route.
///
/// The account is read on every request (the person may sign in or out while the app is open), so
/// nothing here caches a key. Every request carries `Authorization: Bearer <apiKey>` (marked
/// sensitive), `x-redrob-app: query` and this client's `x-redrob-session`.
pub struct AiClient {
    client: Client,
    /// One id per client, so the console groups this app run's requests together.
    session: String,
    resolver: AccountResolver,
}

impl AiClient {
    /// Reads the account by the contract's precedence; `own_file` is this app's own `account.json`.
    pub fn new(own_file: Option<PathBuf>) -> Self {
        Self::with_resolver(Arc::new(move || {
            account::current_account(own_file.as_deref().map(Path::new))
        }))
    }

    /// A client whose account lookup is `resolver` (tests point it at a fake console).
    pub fn with_resolver(resolver: AccountResolver) -> Self {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(90))
            .user_agent(concat!("Redrob-Query/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("static HTTP client configuration is valid");
        Self {
            client,
            session: Uuid::new_v4().to_string(),
            resolver,
        }
    }

    /// The account in force, read fresh. `None` means not signed in.
    #[must_use]
    pub fn account(&self) -> Option<SharedAccount> {
        (self.resolver)()
    }

    /// Whether an account is signed in, and whose it is. Never a key.
    #[must_use]
    pub fn account_status(&self) -> AiKeyStatus {
        self.account()
            .as_ref()
            .map_or_else(AiKeyStatus::signed_out, AiKeyStatus::from_account)
    }

    pub async fn chat(&self, request: AiChatRequest) -> Result<AiChatResponse> {
        validate_request(&request)?;
        let account = self.account().ok_or(DataError::AiAuthentication)?;
        let url = account.chat_completions_url();
        let wire = WireRequest::from(&request);
        let mut last_error = DataError::AiUnavailable;

        for attempt in 0..MAX_ATTEMPTS {
            let response = self
                .authorised(&account, self.client.post(&url).json(&wire))
                .send()
                .await;
            match response {
                Ok(response) if response.status().is_success() => {
                    let bytes = bounded_response(response, MAX_RESPONSE_BYTES).await?;
                    return parse_response(&bytes);
                }
                Ok(response) => {
                    let status = response.status();
                    let body = bounded_response(response, MAX_ERROR_BODY_BYTES)
                        .await
                        .unwrap_or_default();
                    last_error = classify_failure(status, &body);
                    if !retryable_status(status) || attempt + 1 == MAX_ATTEMPTS {
                        return Err(last_error);
                    }
                }
                Err(error) => {
                    last_error = transport_error(&error);
                    if attempt + 1 == MAX_ATTEMPTS {
                        return Err(last_error);
                    }
                }
            }
            sleep(Duration::from_millis(200 * (1_u64 << attempt))).await;
        }
        Err(last_error)
    }

    fn authorised(
        &self,
        account: &SharedAccount,
        builder: reqwest::RequestBuilder,
    ) -> reqwest::RequestBuilder {
        let mut bearer = HeaderValue::from_str(&format!(
            "Bearer {}",
            account.api_key.expose_secret()
        ))
        .unwrap_or_else(|_| HeaderValue::from_static("Bearer"));
        bearer.set_sensitive(true);
        builder
            .header(reqwest::header::AUTHORIZATION, bearer)
            .header("x-redrob-app", "query")
            .header("x-redrob-session", &self.session)
    }
}

fn transport_error(error: &reqwest::Error) -> DataError {
    if error.is_timeout() {
        DataError::Timeout(Duration::from_secs(90))
    } else {
        DataError::AiUnavailable
    }
}

/// The console's `error.message`, when its body carries one, trimmed to a sentence or two.
fn error_message(body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    let message = value
        .pointer("/error/message")
        .or_else(|| value.get("message"))
        .and_then(Value::as_str)?
        .trim();
    if message.is_empty() {
        return None;
    }
    let mut text: String = message.chars().take(MAX_ERROR_MESSAGE_CHARS).collect();
    if text.len() < message.len() {
        text.push('…');
    }
    Some(text)
}

/// Whether an account is signed in, and whose it is. Never a key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiKeyStatus {
    pub source: AiKeySource,
    /// `accountName` and `apiKeyName` as the status line shows them; empty when signed out.
    #[serde(default)]
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiKeySource {
    /// `REDROB_API_KEY` in the environment, or this app's own `account.json`.
    Own,
    /// Redrob Launcher's shared sign-in. Signing out happens there.
    Launcher,
    /// No account is signed in.
    None,
}

impl AiKeyStatus {
    #[must_use]
    pub fn signed_out() -> Self {
        Self {
            source: AiKeySource::None,
            label: String::new(),
        }
    }

    #[must_use]
    pub fn from_account(account: &SharedAccount) -> Self {
        Self {
            source: match account.source {
                AccountSource::Launcher => AiKeySource::Launcher,
                AccountSource::Environment | AccountSource::Own => AiKeySource::Own,
            },
            label: account.label(),
        }
    }

    #[must_use]
    pub fn signed_in(&self) -> bool {
        self.source != AiKeySource::None
    }
}

async fn bounded_response(response: reqwest::Response, limit: usize) -> Result<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > u64::try_from(limit).unwrap_or(u64::MAX))
    {
        return Err(DataError::AiInvalidResponse);
    }
    let mut body = Vec::with_capacity(response.content_length().map_or(0, |length| {
        usize::try_from(length).unwrap_or(limit).min(limit)
    }));
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| DataError::AiInvalidResponse)?;
        if body.len().saturating_add(chunk.len()) > limit {
            return Err(DataError::AiInvalidResponse);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn validate_request(request: &AiChatRequest) -> Result<()> {
    if request.messages.is_empty() || request.messages.len() > 100 {
        return Err(DataError::AiRejected(
            "provide between 1 and 100 messages".to_owned(),
        ));
    }
    if request.model.trim().is_empty() || request.model.len() > 100 {
        return Err(DataError::AiRejected("model is invalid".to_owned()));
    }
    if request
        .messages
        .iter()
        .any(|message| message.content.len() > 100_000)
    {
        return Err(DataError::AiRejected("message is too large".to_owned()));
    }
    if request
        .temperature
        .is_some_and(|value| !(0.0..=2.0).contains(&value))
    {
        return Err(DataError::AiRejected(
            "temperature must be between 0 and 2".to_owned(),
        ));
    }
    Ok(())
}

fn retryable_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

/// The console's own sentence when it gave one; the status class otherwise.
fn classify_failure(status: StatusCode, body: &[u8]) -> DataError {
    match (status, error_message(body)) {
        (StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN, _) => DataError::AiAuthentication,
        (StatusCode::TOO_MANY_REQUESTS, _) => DataError::AiUnavailable,
        (value, Some(message)) if value.is_server_error() => {
            DataError::AiRejected(format!("the console answered HTTP {status}: {message}"))
        }
        (value, None) if value.is_server_error() => DataError::AiUnavailable,
        (_, Some(message)) => DataError::AiRejected(message),
        (_, None) => DataError::AiRejected(format!("service returned HTTP {status}")),
    }
}

/// The OpenAI-compatible request body: snake_case on the wire, whatever the models use in-app.
#[derive(Serialize)]
struct WireRequest<'a> {
    model: &'a str,
    messages: &'a [AiMessage],
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

impl<'a> From<&'a AiChatRequest> for WireRequest<'a> {
    fn from(request: &'a AiChatRequest) -> Self {
        Self {
            model: &request.model,
            messages: &request.messages,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
        }
    }
}

#[derive(Deserialize)]
struct WireResponse {
    #[serde(default)]
    id: String,
    #[serde(default)]
    model: String,
    choices: Vec<WireChoice>,
    usage: Option<WireUsage>,
}

#[derive(Deserialize)]
struct WireChoice {
    message: WireMessage,
}

#[derive(Deserialize)]
struct WireMessage {
    role: String,
    /// Some providers answer `null` content alongside tool calls; that is not an answer here.
    #[serde(default)]
    content: Option<String>,
}

#[derive(Deserialize)]
struct WireUsage {
    #[serde(rename = "prompt_tokens", default)]
    prompt: u32,
    #[serde(rename = "completion_tokens", default)]
    completion: u32,
    #[serde(rename = "total_tokens", default)]
    total: u32,
}

fn parse_response(bytes: &[u8]) -> Result<AiChatResponse> {
    let wire: WireResponse =
        serde_json::from_slice(bytes).map_err(|_| DataError::AiInvalidResponse)?;
    let choice = wire
        .choices
        .into_iter()
        .next()
        .ok_or(DataError::AiInvalidResponse)?;
    let content = choice.message.content.unwrap_or_default();
    if content.is_empty() {
        return Err(DataError::AiInvalidResponse);
    }
    let role = match choice.message.role.as_str() {
        "assistant" => AiRole::Assistant,
        "system" => AiRole::System,
        "user" => AiRole::User,
        _ => return Err(DataError::AiInvalidResponse),
    };
    Ok(AiChatResponse {
        id: wire.id,
        model: wire.model,
        message: AiMessage {
            role,
            content,
        },
        usage: wire.usage.map(|usage| AiUsage {
            prompt_tokens: usage.prompt,
            completion_tokens: usage.completion,
            total_tokens: usage.total,
        }),
    })
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        io::{BufRead, BufReader, Read, Write},
        net::{TcpListener, TcpStream},
        sync::{Arc, Mutex},
        thread,
        time::Duration as StdDuration,
    };

    use secrecy::SecretString;
    use serde_json::json;

    use super::*;

    /// One request as the fake console saw it.
    #[derive(Clone, Debug)]
    struct Seen {
        method: String,
        path: String,
        headers: HashMap<String, String>,
        body: String,
    }

    impl Seen {
        fn header(&self, name: &str) -> Option<&str> {
            self.headers
                .get(&name.to_ascii_lowercase())
                .map(String::as_str)
        }
    }

    /// What the fake console answers with. `declared_length` lies in `Content-Length` when set.
    struct Answer {
        status: u16,
        body: String,
        declared_length: Option<usize>,
    }

    fn reply(status: u16, body: impl Into<String>) -> Answer {
        Answer {
            status,
            body: body.into(),
            declared_length: None,
        }
    }

    fn completion(id: &str, content: &str) -> Answer {
        reply(
            200,
            json!({
                "id": id, "model": "mock-model",
                "choices": [{"message": {"role": "assistant", "content": content}}],
                "usage": {"prompt_tokens": 4, "completion_tokens": 3, "total_tokens": 7}
            })
            .to_string(),
        )
    }

    /// A tiny stand-in for the console on 127.0.0.1: the wrong bearer is 401 everywhere, as the
    /// real console does; everything else is up to `answer`.
    struct FakeConsole {
        base_url: String,
        key: String,
        seen: Arc<Mutex<Vec<Seen>>>,
    }

    impl FakeConsole {
        fn start(key: &str, answer: impl Fn(&Seen) -> Answer + Send + Sync + 'static) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let seen = Arc::new(Mutex::new(Vec::new()));
            let expected = format!("Bearer {key}");
            let log = Arc::clone(&seen);
            thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else { break };
                    let Some(request) = read_request(&mut stream) else {
                        continue;
                    };
                    log.lock().unwrap().push(request.clone());
                    let answer = if request.header("authorization") != Some(expected.as_str()) {
                        reply(
                            401,
                            json!({"error": {"message": "invalid api key"}}).to_string(),
                        )
                    } else {
                        answer(&request)
                    };
                    let length = answer.declared_length.unwrap_or(answer.body.len());
                    let _ = write!(
                        stream,
                        "HTTP/1.1 {} X\r\nContent-Type: application/json\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n{}",
                        answer.status, answer.body
                    );
                    let _ = stream.flush();
                }
            });
            Self {
                base_url: format!("http://127.0.0.1:{port}/api/backend/v1"),
                key: key.into(),
                seen,
            }
        }

        fn account(&self) -> SharedAccount {
            self.account_with_key(&self.key)
        }

        fn account_with_key(&self, key: &str) -> SharedAccount {
            SharedAccount {
                api_key: SecretString::from(key.to_owned()),
                api_key_name: "Redrob Launcher".to_owned(),
                account_name: "Acme".to_owned(),
                api_base_url: self.base_url.clone(),
                source: AccountSource::Launcher,
            }
        }

        fn seen(&self) -> Vec<Seen> {
            self.seen.lock().unwrap().clone()
        }
    }

    fn read_request(stream: &mut TcpStream) -> Option<Seen> {
        stream
            .set_read_timeout(Some(StdDuration::from_secs(5)))
            .ok()?;
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        let mut parts = line.split_whitespace();
        let method = parts.next()?.to_owned();
        let path = parts.next()?.to_owned();
        let mut headers = HashMap::new();
        loop {
            let mut header = String::new();
            reader.read_line(&mut header).ok()?;
            let header = header.trim_end();
            if header.is_empty() {
                break;
            }
            let (name, value) = header.split_once(':')?;
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
        }
        let length: usize = headers
            .get("content-length")
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        let mut body = vec![0; length];
        reader.read_exact(&mut body).ok()?;
        Some(Seen {
            method,
            path,
            headers,
            body: String::from_utf8_lossy(&body).into_owned(),
        })
    }

    fn client_with(account: Option<SharedAccount>) -> AiClient {
        AiClient::with_resolver(Arc::new(move || account.clone()))
    }

    fn test_chat_request() -> AiChatRequest {
        AiChatRequest {
            model: "auto".to_owned(),
            messages: vec![AiMessage {
                role: AiRole::User,
                content: "show indexes".to_owned(),
            }],
            temperature: Some(0.2),
            max_tokens: Some(128),
        }
    }

    #[tokio::test]
    async fn chat_hits_the_console_route_with_the_bearer_app_and_session() {
        let console = FakeConsole::start("rk_s3cret", |request| {
            assert_eq!(request.method, "POST");
            completion("chat-1", "Use an index.")
        });
        let client = client_with(Some(console.account()));

        let response = client.chat(test_chat_request()).await.unwrap();
        assert_eq!(response.id, "chat-1");
        assert_eq!(response.model, "mock-model");
        assert_eq!(response.message.content, "Use an index.");
        assert_eq!(response.usage.unwrap().total_tokens, 7);

        let seen = console.seen();
        assert_eq!(seen.len(), 1, "{seen:?}");
        let request = &seen[0];
        assert_eq!(request.path, "/api/backend/v1/chat/completions");
        assert_eq!(request.header("authorization"), Some("Bearer rk_s3cret"));
        assert_eq!(request.header("x-redrob-app"), Some("query"));
        let session = request.header("x-redrob-session").unwrap();
        assert!(Uuid::parse_str(session).is_ok(), "{session}");
        assert!(
            request
                .header("content-type")
                .is_some_and(|value| value.starts_with("application/json"))
        );

        // The OpenAI-compatible body: snake_case, the key nowhere in it.
        let body: Value = serde_json::from_str(&request.body).unwrap();
        assert_eq!(body["model"], "auto");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "show indexes");
        assert_eq!(body["temperature"], 0.2);
        assert_eq!(body["max_tokens"], 128);
        assert!(body.get("maxTokens").is_none());
        assert!(!request.body.contains("rk_s3cret"));
    }

    #[tokio::test]
    async fn the_account_is_read_on_every_request_so_a_sign_in_counts_at_once() {
        let console = FakeConsole::start("rk_fresh", |_| completion("chat-fresh", "ok"));
        let slot: Arc<Mutex<Option<SharedAccount>>> = Arc::new(Mutex::new(None));
        let read = Arc::clone(&slot);
        let client = AiClient::with_resolver(Arc::new(move || read.lock().unwrap().clone()));

        assert!(matches!(
            client.chat(test_chat_request()).await,
            Err(DataError::AiAuthentication)
        ));
        assert_eq!(client.account_status(), AiKeyStatus::signed_out());
        assert!(console.seen().is_empty(), "no request without an account");

        *slot.lock().unwrap() = Some(console.account());
        let response = client.chat(test_chat_request()).await.unwrap();
        assert_eq!(response.id, "chat-fresh");
        let status = client.account_status();
        assert_eq!(status.source, AiKeySource::Launcher);
        assert_eq!(status.label, "Acme (Redrob Launcher)");
        assert!(status.signed_in());
    }

    #[tokio::test]
    async fn a_401_is_not_signed_in_and_is_not_retried() {
        let console = FakeConsole::start("rk_fresh", |_| completion("never", "no"));
        let client = client_with(Some(console.account_with_key("rk_stale")));

        assert!(matches!(
            client.chat(test_chat_request()).await,
            Err(DataError::AiAuthentication)
        ));
        assert_eq!(console.seen().len(), 1);
    }

    #[tokio::test]
    async fn the_console_error_message_is_surfaced_without_the_key() {
        let console = FakeConsole::start("rk_s", |_| {
            reply(
                400,
                json!({"error": {"message": "model auto is not available for this workspace", "type": "invalid_request_error"}})
                    .to_string(),
            )
        });
        let client = client_with(Some(console.account()));

        let error = client.chat(test_chat_request()).await.unwrap_err();
        assert_eq!(
            error.to_string(),
            "AI request was rejected: model auto is not available for this workspace"
        );
        assert!(!error.to_string().contains("rk_s"));
        assert_eq!(console.seen().len(), 1, "a 400 is not retried");
    }

    #[tokio::test]
    async fn a_server_error_is_retried_then_reported_with_its_message() {
        let console = FakeConsole::start("rk_s", |_| {
            reply(
                503,
                json!({"error": {"message": "upstream provider timed out"}}).to_string(),
            )
        });
        let client = client_with(Some(console.account()));

        let error = client.chat(test_chat_request()).await.unwrap_err();
        assert!(
            matches!(error, DataError::AiRejected(ref message) if message.contains("HTTP 503") && message.contains("upstream provider timed out")),
            "{error}"
        );
        assert_eq!(console.seen().len(), MAX_ATTEMPTS);
    }

    #[test]
    fn a_missing_account_renders_the_sign_in_sentence_with_the_launcher_link() {
        let client = client_with(None);
        assert_eq!(client.account_status(), AiKeyStatus::signed_out());
        assert!(!client.account_status().signed_in());
        assert!(NOT_SIGNED_IN_MESSAGE.contains("Redrob Launcher"));
        assert!(INSTALL_URL.starts_with("https://github.com/redrob-labs/redrob-launcher"));
        assert_eq!(
            serde_json::to_value(AiKeyStatus::signed_out()).unwrap(),
            json!({"source": "none", "label": ""})
        );
    }

    #[tokio::test]
    async fn ai_client_rejects_declared_oversized_response() {
        let console = FakeConsole::start("rk_s", |_| Answer {
            status: 200,
            body: String::new(),
            declared_length: Some(MAX_RESPONSE_BYTES + 1),
        });
        let client = client_with(Some(console.account()));

        assert!(matches!(
            client.chat(test_chat_request()).await,
            Err(DataError::AiInvalidResponse)
        ));
        assert_eq!(console.seen().len(), 1);
    }

    #[test]
    fn the_error_message_is_read_from_the_body_and_bounded() {
        assert_eq!(
            error_message(br#"{"error":{"message":"localized","code":"x"}}"#).as_deref(),
            Some("localized")
        );
        assert_eq!(
            error_message(br#"{"message":"flat"}"#).as_deref(),
            Some("flat")
        );
        assert_eq!(error_message(b"not json"), None);
        assert_eq!(error_message(br#"{"error":{"message":"   "}}"#), None);
        let long = format!(r#"{{"error":{{"message":"{}"}}}}"#, "x".repeat(1000));
        let shown = error_message(long.as_bytes()).unwrap();
        assert_eq!(shown.chars().count(), MAX_ERROR_MESSAGE_CHARS + 1);
        assert!(shown.ends_with('…'));
    }

    #[test]
    fn parses_openai_response() {
        let response = parse_response(
            br#"{
            "id":"chat-1","model":"auto",
            "choices":[{"message":{"role":"assistant","content":"Use an index."}}],
            "usage":{"prompt_tokens":5,"completion_tokens":4,"total_tokens":9}
        }"#,
        )
        .unwrap();
        assert_eq!(response.message.content, "Use an index.");
        assert_eq!(response.usage.unwrap().total_tokens, 9);
    }

    #[test]
    fn rejects_malformed_or_empty_responses() {
        assert!(matches!(
            parse_response(br#"{"id":"x","model":"m","choices":[]}"#),
            Err(DataError::AiInvalidResponse)
        ));
        assert!(matches!(
            parse_response(br#"{"choices":[{"message":{"role":"assistant","content":null}}]}"#),
            Err(DataError::AiInvalidResponse)
        ));
        assert!(matches!(
            parse_response(b"not json"),
            Err(DataError::AiInvalidResponse)
        ));
    }

    #[test]
    fn classifies_auth_client_and_retryable_errors() {
        assert!(matches!(
            classify_failure(StatusCode::UNAUTHORIZED, b""),
            DataError::AiAuthentication
        ));
        assert!(matches!(
            classify_failure(StatusCode::BAD_REQUEST, b""),
            DataError::AiRejected(_)
        ));
        assert!(matches!(
            classify_failure(StatusCode::SERVICE_UNAVAILABLE, b""),
            DataError::AiUnavailable
        ));
        assert!(retryable_status(StatusCode::TOO_MANY_REQUESTS));
        assert!(!retryable_status(StatusCode::BAD_REQUEST));
    }

    #[test]
    fn builds_fixed_chat_request_from_minimal_sql_context() {
        let input = AiAssistantRequest {
            prompt: "write the customer query".to_owned(),
            active_query: Some("SELECT id FROM customers;".to_owned()),
            database_kind: DatabaseKind::SQLite,
            query_language: QueryLanguage::Sql,
        };

        let request = build_assistant_chat_request(&input).unwrap();
        assert_eq!(request.model, "auto");
        assert_eq!(request.temperature, Some(0.2));
        assert_eq!(request.max_tokens, None);
        assert_eq!(request.messages.len(), 2);
        assert_eq!(request.messages[0].role, AiRole::System);
        assert!(request.messages[0].content.contains("SQLite SQL"));
        assert!(
            request.messages[0]
                .content
                .contains("only the user's prompt and optional active query")
        );
        assert!(request.messages[0].content.contains(
            "No database result rows or database credentials are automatically attached"
        ));
        assert!(!request.messages[0].content.contains(&input.prompt));
        assert!(!request.messages[0].content.contains("SELECT id"));
        assert_eq!(request.messages[1].role, AiRole::User);
        assert!(request.messages[1].content.contains(&input.prompt));
        assert!(
            request.messages[1]
                .content
                .contains("SELECT id FROM customers;")
        );
    }

    #[test]
    fn builds_mongo_instruction_and_rejects_mismatched_language() {
        let mongo = AiAssistantRequest {
            prompt: "find customers".to_owned(),
            active_query: None,
            database_kind: DatabaseKind::MongoDb,
            query_language: QueryLanguage::MongoJson,
        };
        let request = build_assistant_chat_request(&mongo).unwrap();
        assert!(request.messages[0].content.contains("fenced mql or json"));
        assert!(request.messages[1].content.contains("(none supplied)"));

        let mismatched = AiAssistantRequest {
            query_language: QueryLanguage::Sql,
            ..mongo
        };
        assert!(matches!(
            build_assistant_chat_request(&mismatched),
            Err(DataError::AiRejected(message)) if message.contains("does not match")
        ));

        let sql_server = AiAssistantRequest {
            prompt: "write a query".to_owned(),
            active_query: None,
            database_kind: DatabaseKind::SqlServer,
            query_language: QueryLanguage::Sql,
        };
        assert!(matches!(
            build_assistant_chat_request(&sql_server),
            Err(DataError::AiRejected(message))
                if message.contains("no connector or connection profile")
        ));
    }

    #[test]
    fn enforces_context_limits_and_rejects_extra_renderer_fields() {
        let oversized_prompt = AiAssistantRequest {
            prompt: "x".repeat(MAX_AI_PROMPT_BYTES + 1),
            active_query: None,
            database_kind: DatabaseKind::PostgreSql,
            query_language: QueryLanguage::Sql,
        };
        assert!(matches!(
            build_assistant_chat_request(&oversized_prompt),
            Err(DataError::AiRejected(message)) if message.contains("prompt exceeds")
        ));

        let oversized_query = AiAssistantRequest {
            prompt: "explain".to_owned(),
            active_query: Some("q".repeat(MAX_AI_ACTIVE_QUERY_BYTES + 1)),
            database_kind: DatabaseKind::PostgreSql,
            query_language: QueryLanguage::Sql,
        };
        assert!(matches!(
            build_assistant_chat_request(&oversized_query),
            Err(DataError::AiRejected(message)) if message.contains("active query exceeds")
        ));

        for extra in ["messages", "resultRows", "credentials", "model", "baseUrl"] {
            let mut value = serde_json::json!({
                "prompt": "help",
                "databaseKind": "postgre_sql",
                "queryLanguage": "sql"
            });
            value
                .as_object_mut()
                .unwrap()
                .insert(extra.to_owned(), serde_json::json!([]));
            assert!(serde_json::from_value::<AiAssistantRequest>(value).is_err());
        }
    }
}
