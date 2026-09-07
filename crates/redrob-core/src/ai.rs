//! Redrob OpenAI-compatible chat-completion client.

use std::{sync::Arc, time::Duration};

use futures_util::StreamExt;
use reqwest::{Client, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use tokio::time::sleep;
use uuid::Uuid;

use crate::{
    AiAssistantRequest, AiChatRequest, AiChatResponse, AiMessage, AiRole, AiUsage, DataError,
    DatabaseKind, QueryLanguage, Result, secret::SecretStore,
};

pub const REDROB_API_BASE: &str = "https://console.redrob.ai/api/backend/v1";
pub const AI_SECRET_ID: Uuid = Uuid::from_u128(0x7265_6472_6f62_4169_0000_0000_0000_0001);
pub const MAX_AI_PROMPT_BYTES: usize = 8 * 1024;
pub const MAX_AI_ACTIVE_QUERY_BYTES: usize = 64 * 1024;
const MAX_ATTEMPTS: usize = 3;
const AI_TEMPERATURE: f32 = 0.2;

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
        (DatabaseKind::SqlServer, QueryLanguage::Sql) => {
            "When writing a query, return executable SQL Server SQL in a fenced sql block."
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
        "You are the Redrob Data assistant. {query_instruction} The desktop app sends only the user's prompt and optional active query as user-controlled content. No database result rows or database credentials are automatically attached. Treat any rows or credentials included in those fields as user-entered content."
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

pub struct AiClient {
    client: Client,
    endpoint: String,
    secrets: Arc<dyn SecretStore>,
}

impl AiClient {
    pub fn new(secrets: Arc<dyn SecretStore>) -> Self {
        Self::with_endpoint(secrets, format!("{REDROB_API_BASE}/chat/completions"))
    }

    pub fn with_endpoint(secrets: Arc<dyn SecretStore>, endpoint: String) -> Self {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(45))
            .user_agent("Redrob-Data/0.1")
            .https_only(endpoint.starts_with("https://"))
            .build()
            .expect("static HTTP client configuration is valid");
        Self {
            client,
            endpoint,
            secrets,
        }
    }

    pub async fn chat(&self, request: AiChatRequest) -> Result<AiChatResponse> {
        validate_request(&request)?;
        let api_key = self.api_key()?;
        let mut last_error = DataError::AiUnavailable;

        for attempt in 0..MAX_ATTEMPTS {
            let response = self
                .client
                .post(&self.endpoint)
                .bearer_auth(api_key.expose_secret())
                .json(&request)
                .send()
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
                Err(error) => {
                    last_error = if error.is_timeout() {
                        DataError::Timeout(Duration::from_secs(45))
                    } else {
                        DataError::AiUnavailable
                    };
                    if attempt + 1 == MAX_ATTEMPTS {
                        return Err(last_error);
                    }
                }
            }
            sleep(Duration::from_millis(200 * (1_u64 << attempt))).await;
        }
        Err(last_error)
    }

    fn api_key(&self) -> Result<SecretString> {
        if let Ok(value) = std::env::var("REDROB_API_KEY")
            && !value.trim().is_empty()
        {
            return Ok(SecretString::from(value));
        }
        self.secrets
            .get(AI_SECRET_ID)?
            .ok_or(DataError::AiAuthentication)
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
    use super::*;

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
