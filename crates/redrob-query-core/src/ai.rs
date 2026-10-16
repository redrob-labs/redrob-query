//! Redrob chat-completion client, through the shared Redrob Code daemon.
//!
//! One `redrob serve --register` runs per user and every Redrob app attaches to it (redrob-code
//! `docs/LOCAL-ENGINE-API.md`, section 3). The daemon holds the Redrob console key and calls the
//! console with its own credential; this app sends the daemon's Basic secret, its app name and a
//! session id on every chat request and never sees a console key on that path. There is
//! deliberately no direct path to the console: two paths would hide whether masking applied.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use futures_util::StreamExt;
use redrob_engine_client::{Record, Unavailable, app_headers, openai_base};
use reqwest::{Client, StatusCode, header::HeaderValue};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::time::sleep;
use uuid::Uuid;

use crate::{
    AiAssistantRequest, AiChatRequest, AiChatResponse, AiMessage, AiRole, AiUsage, DataError,
    DatabaseKind, QueryLanguage, Result, secret::SecretStore,
};

/// The name the daemon labels this app's requests with (`x-redrob-app`).
pub const ENGINE_APP: &str = "query";
/// How long a daemon started by this app stays up with nobody attached.
pub const DEFAULT_IDLE_EXIT_MINUTES: u32 = 10;
/// Where a person gets Redrob Code when it is not installed: the launcher installs it.
pub const INSTALL_URL: &str = "https://github.com/redrob-labs/redrob-launcher/releases/latest";
/// The sentence shown when no `redrob` binary exists. The link is [`INSTALL_URL`].
pub const NOT_INSTALLED_MESSAGE: &str =
    "Redrob Code is not installed. Install it with Redrob Launcher.";
/// The provider id the daemon lists the Redrob console under (`GET /provider`, `PUT /auth/redrob`).
pub const REDROB_PROVIDER_ID: &str = "redrob";
/// The daemon's own error code when it holds no console credential (LOCAL-ENGINE-API.md, Errors).
const ENGINE_NOT_AUTHENTICATED: &str = "engine_not_authenticated";
/// The daemon lists a credential-less Redrob provider with this placeholder; it is not a key.
const PUBLIC_SENTINEL: &str = "public";
/// The keyring slot an earlier version of this app saved its own console key in. It is only read
/// to hand that key to the engine once, then deleted; profile storage still reserves the id.
pub const AI_SECRET_ID: Uuid = Uuid::from_u128(0x7265_6472_6f62_4169_0000_0000_0000_0001);
pub const MAX_AI_PROMPT_BYTES: usize = 8 * 1024;
pub const MAX_AI_ACTIVE_QUERY_BYTES: usize = 64 * 1024;
const MAX_ATTEMPTS: usize = 3;
const AI_TEMPERATURE: f32 = 0.2;
const MAX_ERROR_BODY_BYTES: usize = 64 * 1024;

/// What the app renders for a missing engine: the install sentence with its link, or the start
/// failure's own reason. No fallback is named because there is none.
#[must_use]
pub fn unavailable_message(unavailable: &Unavailable) -> String {
    match unavailable {
        Unavailable::NotInstalled => format!("{NOT_INSTALLED_MESSAGE} {INSTALL_URL}"),
        Unavailable::StartFailed(reason) => {
            format!("The Redrob Code engine could not be started: {reason}")
        }
    }
}

/// Builds the provider request from the narrow, renderer-facing assistant contract.
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

/// How the daemon is found: [`redrob_engine_client::require_as`] in the app, a closure in tests.
pub type EngineResolver = Arc<dyn Fn() -> std::result::Result<Record, Unavailable> + Send + Sync>;

/// Async client for the shared Redrob Code daemon.
///
/// The daemon is found (or started) on the first request and its record kept. The record's secret
/// rotates on every daemon start, so a 401 or a refused connection re-reads the record once and
/// retries once; a second failure is reported. Every request carries the daemon's
/// `Authorization`, `x-redrob-app: query` and this client's `x-redrob-session`.
pub struct AiClient {
    client: Client,
    secrets: Arc<dyn SecretStore>,
    /// One id per client, so the console groups this app run's requests together.
    session: String,
    record: Mutex<Option<Record>>,
    resolver: EngineResolver,
}

impl AiClient {
    /// Takes no network action: the daemon is looked up when the first request needs it.
    pub fn new(secrets: Arc<dyn SecretStore>) -> Self {
        Self::with_resolver(
            secrets,
            Arc::new(|| redrob_engine_client::require_as(ENGINE_APP, DEFAULT_IDLE_EXIT_MINUTES)),
        )
    }

    /// A client whose daemon lookup is `resolver` (tests point it at a fake daemon's record).
    pub fn with_resolver(secrets: Arc<dyn SecretStore>, resolver: EngineResolver) -> Self {
        // The daemon listens on 127.0.0.1 over plain HTTP; nothing here leaves the machine.
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(45))
            .user_agent("Redrob-Query/0.2")
            .build()
            .expect("static HTTP client configuration is valid");
        Self {
            client,
            secrets,
            session: Uuid::new_v4().to_string(),
            record: Mutex::new(None),
            resolver,
        }
    }

    pub async fn chat(&self, request: AiChatRequest) -> Result<AiChatResponse> {
        validate_request(&request)?;
        let mut last_error = DataError::AiUnavailable;

        for attempt in 0..MAX_ATTEMPTS {
            let response = self
                .send(|record| self.client.post(chat_endpoint(record)).json(&request))
                .await;
            match response {
                Ok(response) if response.status().is_success() => {
                    let bytes = bounded_response(response, 2 * 1024 * 1024).await?;
                    return parse_response(&bytes);
                }
                Ok(response) => {
                    let status = response.status();
                    last_error = classify_status(status);
                    if !retryable_status(status) || attempt + 1 == MAX_ATTEMPTS {
                        return Err(last_error);
                    }
                }
                // A missing engine or a daemon that is not signed in does not get better by asking
                // again; a transport failure might.
                Err(error @ (DataError::EngineUnavailable(_) | DataError::AiAuthentication)) => {
                    return Err(error);
                }
                Err(error) => {
                    last_error = error;
                    if attempt + 1 == MAX_ATTEMPTS {
                        return Err(last_error);
                    }
                }
            }
            sleep(Duration::from_millis(200 * (1_u64 << attempt))).await;
        }
        Err(last_error)
    }

    /// Whether the daemon is signed in to the Redrob console (`GET /provider`), and with whose key.
    /// Read fresh on every call: the person may sign in or out elsewhere while this app is open.
    pub async fn account_status(&self) -> Result<AiKeyStatus> {
        let response = self
            .send(|record| self.client.get(daemon_endpoint(record, "provider")))
            .await?;
        if !response.status().is_success() {
            return Err(classify_status(response.status()));
        }
        let bytes = bounded_response(response, 2 * 1024 * 1024).await?;
        let list: Value =
            serde_json::from_slice(&bytes).map_err(|_| DataError::AiInvalidResponse)?;
        Ok(AiKeyStatus::from_provider_list(&list))
    }

    /// Hands a console key to the daemon (`PUT /auth/redrob`), which keeps it from here on. A second
    /// call rotates it. Nothing in this app holds the key afterwards.
    pub async fn sign_in(&self, key: &str) -> Result<()> {
        if key.trim().is_empty() {
            return Err(DataError::SecretUnavailable);
        }
        let response = self.send(|record| self.auth_request(record, key)).await?;
        accepted_key(response.status())
    }

    /// Finds or starts the daemon, off the async runtime: the client crate shells out to
    /// `redrob service start` and probes with blocking HTTP.
    async fn connect(&self) -> Result<Record> {
        if let Some(record) = self.lock_record().clone() {
            return Ok(record);
        }
        let resolver = Arc::clone(&self.resolver);
        let record = tokio::task::spawn_blocking(move || resolver())
            .await
            .map_err(|_| DataError::AiUnavailable)?
            .map_err(|unavailable| {
                DataError::EngineUnavailable(unavailable_message(&unavailable))
            })?;
        *self.lock_record() = Some(record.clone());
        self.migrate_legacy_key(&record).await;
        Ok(record)
    }

    /// A key an earlier version of this app saved in the OS keyring is handed to the engine once
    /// and deleted locally; the engine keeps it from then on. A failure leaves it for the next run.
    async fn migrate_legacy_key(&self, record: &Record) {
        let Ok(Some(key)) = self.secrets.get(AI_SECRET_ID) else {
            return;
        };
        let accepted = self
            .headers(record, self.auth_request(record, key.expose_secret()))
            .send()
            .await
            .is_ok_and(|response| response.status().is_success());
        if accepted {
            let _ = self.secrets.delete(AI_SECRET_ID);
        }
    }

    fn auth_request(&self, record: &Record, key: &str) -> reqwest::RequestBuilder {
        self.client
            .put(daemon_endpoint(
                record,
                &format!("auth/{REDROB_PROVIDER_ID}"),
            ))
            .json(&serde_json::json!({"type": "api", "key": key}))
    }

    fn lock_record(&self) -> std::sync::MutexGuard<'_, Option<Record>> {
        self.record
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Sends one request to the daemon with its headers, retrying once with a fresh record when the
    /// daemon has evidently restarted (connection refused, or 401 against the secret in hand). A 401
    /// whose code says the daemon itself is not signed in is reported as such, not retried.
    async fn send(
        &self,
        build: impl Fn(&Record) -> reqwest::RequestBuilder,
    ) -> Result<reqwest::Response> {
        let record = self.connect().await?;
        let first = self.headers(&record, build(&record)).send().await;
        match first {
            Ok(response) if response.status() != StatusCode::UNAUTHORIZED => return Ok(response),
            Ok(response) => {
                let body = bounded_response(response, MAX_ERROR_BODY_BYTES)
                    .await
                    .unwrap_or_default();
                if names_engine_not_authenticated(&body) {
                    return Err(DataError::AiAuthentication);
                }
            }
            Err(error) if !error.is_connect() => return Err(transport_error(&error)),
            Err(_) => {}
        }
        *self.lock_record() = None;
        let record = self.connect().await?;
        self.headers(&record, build(&record))
            .send()
            .await
            .map_err(|error| transport_error(&error))
    }

    fn headers(
        &self,
        record: &Record,
        builder: reqwest::RequestBuilder,
    ) -> reqwest::RequestBuilder {
        let mut builder = builder;
        for (name, value) in app_headers(record, ENGINE_APP, Some(&self.session)) {
            let Ok(mut header) = HeaderValue::from_str(&value) else {
                continue;
            };
            if name == "Authorization" {
                header.set_sensitive(true);
            }
            builder = builder.header(name, header);
        }
        builder
    }
}

/// `<daemon>/v1/chat/completions`: the OpenAI-compatible route.
fn chat_endpoint(record: &Record) -> String {
    format!("{}/chat/completions", openai_base(record))
}

/// `<daemon>/<path>`: the daemon's own routes (`provider`, `auth/<provider>`).
fn daemon_endpoint(record: &Record, path: &str) -> String {
    format!("{}/{path}", record.url.trim_end_matches('/'))
}

fn transport_error(error: &reqwest::Error) -> DataError {
    if error.is_timeout() {
        DataError::Timeout(Duration::from_secs(45))
    } else {
        DataError::AiUnavailable
    }
}

/// The daemon's failure body could echo the payload; only the status is kept.
fn accepted_key(status: StatusCode) -> Result<()> {
    if status.is_success() {
        Ok(())
    } else {
        Err(DataError::AiRejected(
            "the engine did not accept the Redrob key".to_owned(),
        ))
    }
}

/// Branches on the daemon's error `code`, never on its message, which is localized.
fn names_engine_not_authenticated(body: &[u8]) -> bool {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .pointer("/error/code")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .is_some_and(|code| code == ENGINE_NOT_AUTHENTICATED)
}

/// Whether the engine is signed in to the Redrob console, and with whose key. Never a key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiKeyStatus {
    pub source: AiKeySource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiKeySource {
    /// The engine holds a console key handed to it by an app, its environment or its config.
    Engine,
    /// The engine uses Redrob Launcher's shared sign-in. Signing out happens there.
    Launcher,
    /// The engine has no console key.
    None,
}

impl AiKeyStatus {
    #[must_use]
    pub const fn signed_out() -> Self {
        Self {
            source: AiKeySource::None,
        }
    }

    /// Reads `GET /provider`: a `redrob` entry with a credential means signed in. The shape is the
    /// one the other Redrob apps read (`{"all": [{"id", "source", "key", "options": {"apiKey"}}]}`).
    #[must_use]
    pub fn from_provider_list(list: &Value) -> Self {
        let Some(entry) = list.get("all").and_then(Value::as_array).and_then(|all| {
            all.iter()
                .find(|entry| entry.get("id").and_then(Value::as_str) == Some(REDROB_PROVIDER_ID))
        }) else {
            return Self::signed_out();
        };
        if entry
            .pointer("/options/apiKey")
            .and_then(Value::as_str)
            .is_some_and(|key| key == PUBLIC_SENTINEL)
        {
            return Self::signed_out();
        }
        let source = entry.get("source").and_then(Value::as_str);
        let has_key = entry
            .get("key")
            .and_then(Value::as_str)
            .is_some_and(|key| !key.trim().is_empty());
        match source {
            Some("launcher") => Self {
                source: AiKeySource::Launcher,
            },
            Some("api" | "env") => Self {
                source: AiKeySource::Engine,
            },
            _ if has_key => Self {
                source: AiKeySource::Engine,
            },
            _ => Self::signed_out(),
        }
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

fn classify_status(status: StatusCode) -> DataError {
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => DataError::AiAuthentication,
        StatusCode::TOO_MANY_REQUESTS => DataError::AiUnavailable,
        value if value.is_server_error() => DataError::AiUnavailable,
        _ => DataError::AiRejected(format!("service returned HTTP {status}")),
    }
}

#[derive(Deserialize)]
struct WireResponse {
    id: String,
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
    content: String,
}

#[derive(Deserialize)]
struct WireUsage {
    #[serde(rename = "prompt_tokens")]
    prompt: u32,
    #[serde(rename = "completion_tokens")]
    completion: u32,
    #[serde(rename = "total_tokens")]
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
    if choice.message.content.is_empty() {
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
            content: choice.message.content,
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
        collections::{HashMap, VecDeque},
        io::{BufRead, BufReader, Read, Write},
        net::{TcpListener, TcpStream},
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        thread,
        time::Duration as StdDuration,
    };

    use serde_json::json;

    use crate::secret::MemorySecretStore;

    use super::*;

    /// One request as the fake daemon saw it.
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

    /// What the fake daemon answers with. `declared_length` lies in `Content-Length` when set.
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

    /// A tiny stand-in for the shared daemon on 127.0.0.1: the wrong secret is 401 everywhere, as
    /// the real daemon does; `/api/health` answers; everything else is up to `answer`.
    struct FakeDaemon {
        url: String,
        secret: String,
        seen: Arc<Mutex<Vec<Seen>>>,
    }

    impl FakeDaemon {
        fn start(secret: &str, answer: impl Fn(&Seen) -> Answer + Send + Sync + 'static) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let seen = Arc::new(Mutex::new(Vec::new()));
            let expected = redrob_engine_client::authorization(secret);
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
                            json!({"error": {"message": "unauthorized"}}).to_string(),
                        )
                    } else if request.path == "/api/health" {
                        reply(200, json!({"healthy": true}).to_string())
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
                url: format!("http://127.0.0.1:{port}"),
                secret: secret.into(),
                seen,
            }
        }

        fn record(&self) -> Record {
            self.record_with_secret(&self.secret)
        }

        fn record_with_secret(&self, secret: &str) -> Record {
            Record {
                id: format!("id-{secret}"),
                version: "0.6.3".into(),
                url: self.url.clone(),
                port: self.url.rsplit(':').next().unwrap().parse().unwrap(),
                pid: std::process::id(),
                secret: secret.into(),
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

    /// A resolver that answers from a script, repeating its last entry, and counts its calls.
    fn scripted(
        script: Vec<std::result::Result<Record, Unavailable>>,
    ) -> (EngineResolver, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&calls);
        let script = Mutex::new(VecDeque::from(script));
        let resolver: EngineResolver = Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            let mut script = script.lock().unwrap();
            if script.len() > 1 {
                script.pop_front().unwrap()
            } else {
                script.front().cloned().unwrap()
            }
        });
        (resolver, calls)
    }

    fn client_for(daemon: &FakeDaemon) -> (AiClient, Arc<AtomicUsize>) {
        let (resolver, calls) = scripted(vec![Ok(daemon.record())]);
        (
            AiClient::with_resolver(Arc::new(MemorySecretStore::default()), resolver),
            calls,
        )
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
    async fn chat_hits_the_daemon_v1_route_with_its_authorization_app_and_session() {
        let daemon = FakeDaemon::start("s3cret", |request| {
            assert_eq!(request.method, "POST");
            completion("chat-local", "Use an index.")
        });
        let (client, calls) = client_for(&daemon);

        let response = client.chat(test_chat_request()).await.unwrap();
        assert_eq!(response.id, "chat-local");
        assert_eq!(response.model, "mock-model");
        assert_eq!(response.message.content, "Use an index.");
        assert_eq!(response.usage.unwrap().total_tokens, 7);
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let seen = daemon.seen();
        assert_eq!(seen.len(), 1, "{seen:?}");
        let request = &seen[0];
        assert_eq!(request.path, "/v1/chat/completions");
        assert_eq!(
            request.header("authorization"),
            Some(redrob_engine_client::authorization("s3cret").as_str())
        );
        assert_eq!(request.header("x-redrob-app"), Some("query"));
        let session = request.header("x-redrob-session").unwrap();
        assert!(Uuid::parse_str(session).is_ok(), "{session}");
        assert!(
            request
                .header("content-type")
                .is_some_and(|value| value.starts_with("application/json"))
        );
        // No console key travels on this path: the daemon's Basic secret is the only credential.
        assert!(
            !request
                .header("authorization")
                .unwrap()
                .starts_with("Bearer")
        );

        let body: Value = serde_json::from_str(&request.body).unwrap();
        assert_eq!(body["model"], "auto");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "show indexes");
        assert_eq!(body["temperature"], 0.2);
        assert_eq!(body["maxTokens"], 128);
    }

    #[tokio::test]
    async fn a_401_against_a_stale_secret_re_reads_the_record_once_and_retries() {
        let daemon = FakeDaemon::start("fresh", |_| completion("chat-fresh", "ok"));
        // The record in hand names the secret of a daemon that has since restarted.
        let (resolver, calls) = scripted(vec![
            Ok(daemon.record_with_secret("stale")),
            Ok(daemon.record()),
        ]);
        let client = AiClient::with_resolver(Arc::new(MemorySecretStore::default()), resolver);

        let response = client.chat(test_chat_request()).await.unwrap();
        assert_eq!(response.id, "chat-fresh");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let seen = daemon.seen();
        assert_eq!(seen.len(), 2, "{seen:?}");
        assert_eq!(
            seen[0].header("authorization"),
            Some(redrob_engine_client::authorization("stale").as_str())
        );
        assert_eq!(
            seen[1].header("authorization"),
            Some(redrob_engine_client::authorization("fresh").as_str())
        );

        // The fresh record is kept: the next request needs no lookup.
        client.chat(test_chat_request()).await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(daemon.seen().len(), 3);
    }

    #[tokio::test]
    async fn a_401_that_persists_is_reported_not_retried_forever() {
        let daemon = FakeDaemon::start("fresh", |_| completion("never", "no"));
        let (resolver, calls) = scripted(vec![Ok(daemon.record_with_secret("stale"))]);
        let client = AiClient::with_resolver(Arc::new(MemorySecretStore::default()), resolver);

        assert!(matches!(
            client.chat(test_chat_request()).await,
            Err(DataError::AiAuthentication)
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(daemon.seen().len(), 2);
    }

    #[tokio::test]
    async fn the_daemon_without_a_console_key_is_told_apart_by_its_code() {
        let daemon = FakeDaemon::start("s", |_| {
            reply(
                401,
                json!({"error": {"message": "로그인이 필요합니다", "code": "engine_not_authenticated"}})
                    .to_string(),
            )
        });
        let (client, calls) = client_for(&daemon);

        assert!(matches!(
            client.chat(test_chat_request()).await,
            Err(DataError::AiAuthentication)
        ));
        // Not a stale secret: no record re-read, no second request.
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(daemon.seen().len(), 1);
    }

    #[tokio::test]
    async fn a_missing_engine_renders_the_install_sentence_with_the_launcher_link() {
        let (resolver, calls) = scripted(vec![Err(Unavailable::NotInstalled)]);
        let client = AiClient::with_resolver(Arc::new(MemorySecretStore::default()), resolver);

        let error = client.chat(test_chat_request()).await.unwrap_err();
        assert_eq!(
            error.to_string(),
            "Redrob Code is not installed. Install it with Redrob Launcher. https://github.com/redrob-labs/redrob-launcher/releases/latest"
        );
        assert!(matches!(error, DataError::EngineUnavailable(_)));
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "no retry against a missing engine"
        );
        assert_eq!(
            client.account_status().await.unwrap_err().to_string(),
            format!("{NOT_INSTALLED_MESSAGE} {INSTALL_URL}")
        );

        let (resolver, _) = scripted(vec![Err(Unavailable::StartFailed("port busy".into()))]);
        let client = AiClient::with_resolver(Arc::new(MemorySecretStore::default()), resolver);
        assert_eq!(
            client
                .chat(test_chat_request())
                .await
                .unwrap_err()
                .to_string(),
            "The Redrob Code engine could not be started: port busy"
        );
    }

    /// The real lookup against a real (empty) environment: `REDROB_CODE_BIN` names a file that does
    /// not exist and `REDROB_RUN_DIR` an empty directory. The process environment is shared and the
    /// crate forbids `unsafe`, so the check runs in a child test process with that environment.
    #[test]
    fn not_installed_in_a_real_environment_reports_the_install_sentence() {
        let dir = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "ai::tests::child_without_an_engine_sees_the_install_sentence",
                "--ignored",
                "--nocapture",
            ])
            .env("REDROB_CODE_BIN", dir.path().join("missing-redrob"))
            .env("REDROB_RUN_DIR", dir.path())
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stdout}\n{stderr}");
        assert!(
            stdout.contains("test result: ok. 1 passed"),
            "{stdout}\n{stderr}"
        );
    }

    #[tokio::test]
    #[ignore = "run by not_installed_in_a_real_environment_reports_the_install_sentence"]
    async fn child_without_an_engine_sees_the_install_sentence() {
        assert!(std::env::var_os("REDROB_CODE_BIN").is_some());
        let client = AiClient::new(Arc::new(MemorySecretStore::default()));
        let error = client.chat(test_chat_request()).await.unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("{NOT_INSTALLED_MESSAGE} {INSTALL_URL}")
        );
    }

    #[tokio::test]
    async fn sign_in_puts_the_key_to_the_daemon_and_status_reads_the_provider_list() {
        let daemon = FakeDaemon::start("s", |request| {
            match (request.method.as_str(), request.path.as_str()) {
                ("PUT", "/auth/redrob") => {
                    let body: Value = serde_json::from_str(&request.body).unwrap();
                    assert_eq!(body, json!({"type": "api", "key": "rrk_test_key"}));
                    reply(200, "true")
                }
                ("GET", "/provider") => reply(
                    200,
                    json!({"all": [{"id": "openai"}, {"id": "redrob", "source": "launcher"}]})
                        .to_string(),
                ),
                other => panic!("unexpected {other:?}"),
            }
        });
        let (client, _) = client_for(&daemon);

        client.sign_in("rrk_test_key").await.unwrap();
        assert!(matches!(
            client.sign_in("  ").await,
            Err(DataError::SecretUnavailable)
        ));
        assert_eq!(
            client.account_status().await.unwrap(),
            AiKeyStatus {
                source: AiKeySource::Launcher
            }
        );
        let seen = daemon.seen();
        assert_eq!(seen.len(), 2, "{seen:?}");
        assert_eq!(
            (seen[0].method.as_str(), seen[0].path.as_str()),
            ("PUT", "/auth/redrob")
        );
        assert_eq!(seen[0].header("x-redrob-app"), Some("query"));
        assert_eq!(
            (seen[1].method.as_str(), seen[1].path.as_str()),
            ("GET", "/provider")
        );
        assert!(!seen[1].body.contains("rrk_test_key"));
    }

    #[tokio::test]
    async fn the_engine_refusing_a_key_is_reported_without_echoing_it() {
        let daemon = FakeDaemon::start("s", |_| {
            reply(
                400,
                json!({"error": {"message": "rrk_bad echoed"}}).to_string(),
            )
        });
        let (client, _) = client_for(&daemon);
        let error = client.sign_in("rrk_bad").await.unwrap_err();
        assert!(
            matches!(error, DataError::AiRejected(ref message) if !message.contains("rrk_bad")),
            "{error}"
        );
    }

    #[tokio::test]
    async fn a_legacy_keyring_key_is_handed_to_the_engine_once_and_deleted() {
        let daemon = FakeDaemon::start("s", |request| match request.path.as_str() {
            "/auth/redrob" => reply(200, "true"),
            _ => completion("chat-after-migration", "ok"),
        });
        let secrets = Arc::new(MemorySecretStore::default());
        secrets.set(AI_SECRET_ID, "rrk_legacy").unwrap();
        let (resolver, _) = scripted(vec![Ok(daemon.record())]);
        let client =
            AiClient::with_resolver(Arc::clone(&secrets) as Arc<dyn SecretStore>, resolver);

        client.chat(test_chat_request()).await.unwrap();
        let seen = daemon.seen();
        assert_eq!(seen.len(), 2, "{seen:?}");
        assert_eq!(
            (seen[0].method.as_str(), seen[0].path.as_str()),
            ("PUT", "/auth/redrob")
        );
        assert_eq!(
            serde_json::from_str::<Value>(&seen[0].body).unwrap(),
            json!({"type": "api", "key": "rrk_legacy"})
        );
        assert_eq!(seen[0].header("x-redrob-app"), Some("query"));
        assert_eq!(seen[1].path, "/v1/chat/completions");
        assert!(!seen[1].body.contains("rrk_legacy"));
        assert!(
            secrets.get(AI_SECRET_ID).unwrap().is_none(),
            "the migrated key is gone from the keyring"
        );

        // A second chat migrates nothing: the record is cached and the slot is empty.
        client.chat(test_chat_request()).await.unwrap();
        assert_eq!(daemon.seen().len(), 3);
    }

    #[test]
    fn the_provider_list_tells_signed_in_from_public_and_absent() {
        assert_eq!(
            AiKeyStatus::from_provider_list(&json!({"all": []})),
            AiKeyStatus::signed_out()
        );
        assert_eq!(
            AiKeyStatus::from_provider_list(
                &json!({"all": [{"id": "redrob", "options": {"apiKey": "public"}}]})
            ),
            AiKeyStatus::signed_out()
        );
        assert_eq!(
            AiKeyStatus::from_provider_list(
                &json!({"all": [{"id": "redrob", "source": "config"}]})
            ),
            AiKeyStatus::signed_out(),
            "a config entry without a key is not a sign-in"
        );
        assert_eq!(
            AiKeyStatus::from_provider_list(
                &json!({"all": [{"id": "redrob", "source": "launcher"}]})
            )
            .source,
            AiKeySource::Launcher
        );
        assert_eq!(
            AiKeyStatus::from_provider_list(
                &json!({"all": [{"id": "openai"}, {"id": "redrob", "source": "api", "key": "rrk_x"}]})
            )
            .source,
            AiKeySource::Engine
        );
        assert_eq!(
            AiKeyStatus::from_provider_list(&json!({"all": [{"id": "redrob", "key": "rrk_x"}]}))
                .source,
            AiKeySource::Engine
        );
        assert_eq!(
            serde_json::to_value(AiKeyStatus::signed_out()).unwrap(),
            json!({"source": "none"})
        );
    }

    #[tokio::test]
    async fn ai_client_rejects_declared_oversized_response() {
        let daemon = FakeDaemon::start("s", |_| Answer {
            status: 200,
            body: String::new(),
            declared_length: Some(2 * 1024 * 1024 + 1),
        });
        let (client, _) = client_for(&daemon);

        assert!(matches!(
            client.chat(test_chat_request()).await,
            Err(DataError::AiInvalidResponse)
        ));
        assert_eq!(daemon.seen().len(), 1);
    }

    #[test]
    fn the_daemon_sign_in_code_is_read_from_the_error_code_not_the_message() {
        assert!(names_engine_not_authenticated(
            br#"{"error":{"message":"localized","code":"engine_not_authenticated"}}"#
        ));
        assert!(!names_engine_not_authenticated(
            br#"{"error":{"message":"engine_not_authenticated"}}"#
        ));
        assert!(!names_engine_not_authenticated(b"not json"));
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
            parse_response(b"not json"),
            Err(DataError::AiInvalidResponse)
        ));
    }

    #[test]
    fn classifies_auth_client_and_retryable_errors() {
        assert!(matches!(
            classify_status(StatusCode::UNAUTHORIZED),
            DataError::AiAuthentication
        ));
        assert!(matches!(
            classify_status(StatusCode::BAD_REQUEST),
            DataError::AiRejected(_)
        ));
        assert!(matches!(
            classify_status(StatusCode::SERVICE_UNAVAILABLE),
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
