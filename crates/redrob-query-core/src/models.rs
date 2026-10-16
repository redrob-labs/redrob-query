//! Serializable domain models shared by the desktop transport and backend.

use std::{collections::BTreeMap, fmt, net::IpAddr, time::Duration};

use chrono::{DateTime, Utc};
use mongodb::bson::{self, Bson};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{DataError, Result};

/// Database engines presented by Redrob Query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DatabaseKind {
    PostgreSql,
    MySql,
    SQLite,
    MongoDb,
    SqlServer,
}

impl DatabaseKind {
    #[must_use]
    pub const fn default_port(self) -> Option<u16> {
        match self {
            Self::PostgreSql => Some(5432),
            Self::MySql => Some(3306),
            Self::MongoDb => Some(27017),
            Self::SqlServer => Some(1433),
            Self::SQLite => None,
        }
    }

    #[must_use]
    pub fn capabilities(self) -> ConnectionCapabilities {
        match self {
            Self::PostgreSql | Self::MySql | Self::SQLite => ConnectionCapabilities {
                connect: true,
                metadata: true,
                query: true,
                mutation: false,
                transactions: true,
                document_query: false,
                unsupported_reason: None,
            },
            Self::MongoDb => ConnectionCapabilities {
                connect: true,
                metadata: true,
                query: true,
                mutation: false,
                transactions: false,
                document_query: true,
                unsupported_reason: None,
            },
            Self::SqlServer => ConnectionCapabilities {
                connect: false,
                metadata: false,
                query: false,
                mutation: false,
                transactions: false,
                document_query: false,
                unsupported_reason: Some(
                    "SQL Server is unsupported in this preview; no connection will be attempted"
                        .to_owned(),
                ),
            },
        }
    }
}

/// Feature flags used by the UI to avoid advertising unavailable operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(clippy::struct_excessive_bools)]
pub struct ConnectionCapabilities {
    pub connect: bool,
    pub metadata: bool,
    pub query: bool,
    pub mutation: bool,
    pub transactions: bool,
    pub document_query: bool,
    pub unsupported_reason: Option<String>,
}

impl Default for ConnectionCapabilities {
    fn default() -> Self {
        DatabaseKind::SQLite.capabilities()
    }
}

/// Non-secret connection settings. Passwords and API keys never belong here.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConnectionConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    /// Uses `MongoDB` DNS seed-list discovery (`mongodb+srv`) when enabled.
    #[serde(default)]
    pub srv: bool,
    #[serde(default)]
    pub tls: bool,
    #[serde(default)]
    pub options: BTreeMap<String, String>,
}

impl fmt::Debug for ConnectionConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConnectionConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("database", &self.database)
            .field(
                "username",
                &self.username.as_ref().map(|_| "[redacted identity]"),
            )
            .field("file_path", &self.file_path)
            .field("srv", &self.srv)
            .field("tls", &self.tls)
            .field("options", &RedactedOptions(&self.options))
            .finish()
    }
}

impl fmt::Display for ConnectionConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "host={}, port={}, database={}, username={}, file={}, srv={}, tls={}, options=[redacted]",
            self.host.as_deref().unwrap_or("local"),
            self.port
                .map_or_else(|| "default".to_owned(), |value| value.to_string()),
            self.database.as_deref().unwrap_or("default"),
            if self.username.is_some() {
                "[redacted]"
            } else {
                "none"
            },
            self.file_path.as_deref().unwrap_or("none"),
            self.srv,
            self.tls,
        )
    }
}

struct RedactedOptions<'a>(&'a BTreeMap<String, String>);

impl fmt::Debug for RedactedOptions<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_set().entries(self.0.keys()).finish()
    }
}

/// Persistable connection profile; credentials are referenced by `id` in the secret store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConnectionProfile {
    #[serde(default = "Uuid::new_v4")]
    pub id: Uuid,
    pub name: String,
    pub kind: DatabaseKind,
    #[serde(default)]
    pub config: ConnectionConfig,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub built_in: bool,
    #[serde(default)]
    pub capabilities: ConnectionCapabilities,
}

impl ConnectionProfile {
    pub fn validate(&self) -> Result<()> {
        let name = self.name.trim();
        if name.is_empty() || name.len() > 120 {
            return Err(DataError::InvalidProfile(
                "name must contain 1 to 120 characters".to_owned(),
            ));
        }
        if self.config.options.iter().any(|(key, value)| {
            key != "authSource" || self.kind != DatabaseKind::MongoDb || !is_safe_auth_source(value)
        }) {
            return Err(DataError::InvalidProfile(
                "only a non-secret MongoDB authSource option is supported".to_owned(),
            ));
        }
        if self.config.srv && self.kind != DatabaseKind::MongoDb {
            return Err(DataError::InvalidProfile(
                "SRV discovery is supported only for MongoDB".to_owned(),
            ));
        }
        match self.kind {
            DatabaseKind::SQLite => {
                if self.config.file_path.as_deref().is_none_or(str::is_empty) {
                    return Err(DataError::InvalidProfile(
                        "SQLite requires a file path or :memory:".to_owned(),
                    ));
                }
            }
            DatabaseKind::PostgreSql
            | DatabaseKind::MySql
            | DatabaseKind::MongoDb
            | DatabaseKind::SqlServer => {
                let host = self
                    .config
                    .host
                    .as_deref()
                    .ok_or_else(|| DataError::InvalidProfile("host is required".to_owned()))?;
                let valid = if self.kind == DatabaseKind::MongoDb && self.config.srv {
                    is_valid_srv_host(host)
                } else {
                    is_valid_server_host(host)
                };
                if !valid {
                    return Err(DataError::InvalidProfile(if self.config.srv {
                        "SRV host must be a DNS hostname without a URL, credentials, or port"
                            .to_owned()
                    } else {
                        "host must be a hostname or IP address without a URL, credentials, or port"
                            .to_owned()
                    }));
                }
            }
        }
        Ok(())
    }

    pub(crate) fn normalize(&mut self) {
        self.name = self.name.trim().to_owned();
        self.capabilities = self.kind.capabilities();
    }
}

fn is_safe_auth_source(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn is_valid_server_host(host: &str) -> bool {
    if host.is_empty() || host != host.trim() || host.len() > 253 {
        return false;
    }
    host.parse::<IpAddr>().is_ok() || is_valid_dns_name(host)
}

fn is_valid_srv_host(host: &str) -> bool {
    host.parse::<IpAddr>().is_err() && host.contains('.') && is_valid_dns_name(host)
}

fn is_valid_dns_name(host: &str) -> bool {
    host.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && label
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphanumeric)
            && label
                .as_bytes()
                .last()
                .is_some_and(u8::is_ascii_alphanumeric)
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    })
}

/// Stable identifier of the deterministic built-in database.
pub const DEMO_PROFILE_ID: Uuid = Uuid::from_u128(0x7265_6472_6f62_4465_6d6f_0000_0000_0001);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    Connected,
    Disconnected,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStatus {
    pub profile_id: Uuid,
    pub state: ConnectionState,
    pub message: String,
    pub capabilities: ConnectionCapabilities,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetadataKind {
    Server,
    Database,
    Schema,
    Table,
    View,
    Collection,
    Column,
    Index,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataNode {
    pub id: String,
    pub name: String,
    pub kind: MetadataKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nullable: Option<bool>,
    #[serde(default)]
    pub has_children: bool,
    #[serde(default)]
    pub children: Vec<Self>,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataRequest {
    pub connection_id: Uuid,
    #[serde(default)]
    pub parent_id: Option<String>,
}

/// Typed data representation that does not collapse large numbers, binary, or BSON into text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum DataValue {
    Null,
    Boolean(bool),
    Integer(String),
    Float(f64),
    Decimal(String),
    Text(String),
    Binary(String),
    Date(String),
    Time(String),
    DateTime(String),
    Uuid(Uuid),
    Json(Value),
    Bson(Value),
}

impl DataValue {
    #[must_use]
    pub fn from_bson(value: Bson) -> Self {
        match value {
            Bson::Null => Self::Null,
            Bson::Boolean(value) => Self::Boolean(value),
            Bson::Int32(value) => Self::Integer(value.to_string()),
            Bson::Int64(value) => Self::Integer(value.to_string()),
            Bson::Double(value) => Self::Float(value),
            Bson::Decimal128(value) => Self::Decimal(value.to_string()),
            Bson::String(value) => Self::Text(value),
            Bson::Binary(value) => Self::Binary(base64_encode(&value.bytes)),
            Bson::DateTime(value) => Self::DateTime(value.to_string()),
            Bson::ObjectId(value) => Self::Bson(serde_json::json!({"$oid": value.to_hex()})),
            Bson::Timestamp(value) => Self::Bson(serde_json::json!({
                "$timestamp": {"t": value.time, "i": value.increment}
            })),
            other => Self::Bson(other.into_canonical_extjson()),
        }
    }

    pub fn to_bson(&self) -> Result<Bson> {
        match self {
            Self::Null => Ok(Bson::Null),
            Self::Boolean(value) => Ok(Bson::Boolean(*value)),
            Self::Integer(value) => value
                .parse::<i64>()
                .map(Bson::Int64)
                .map_err(|_| DataError::Conversion("invalid integer".to_owned())),
            Self::Float(value) => Ok(Bson::Double(*value)),
            Self::Decimal(value) => value
                .parse()
                .map(Bson::Decimal128)
                .map_err(|_| DataError::Conversion("invalid decimal".to_owned())),
            Self::Text(value) | Self::Date(value) | Self::Time(value) => {
                Ok(Bson::String(value.clone()))
            }
            Self::Binary(value) => Ok(Bson::Binary(bson::Binary {
                subtype: bson::spec::BinarySubtype::Generic,
                bytes: base64_decode(value)?,
            })),
            Self::DateTime(value) => DateTime::parse_from_rfc3339(value)
                .map(|date| Bson::DateTime(bson::DateTime::from_millis(date.timestamp_millis())))
                .map_err(|_| DataError::Conversion("invalid RFC 3339 date-time".to_owned())),
            Self::Uuid(value) => Ok(Bson::String(value.to_string())),
            Self::Json(value) | Self::Bson(value) => bson::to_bson(value)
                .map_err(|_| DataError::Conversion("invalid JSON/BSON value".to_owned())),
        }
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn base64_decode(value: &str) -> Result<Vec<u8>> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|_| DataError::Conversion("invalid base64 binary".to_owned()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum QueryLanguage {
    #[default]
    Auto,
    Sql,
    MongoJson,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryRequest {
    pub connection_id: Uuid,
    pub query: String,
    #[serde(default)]
    pub language: QueryLanguage,
    #[serde(default)]
    pub parameters: Vec<DataValue>,
    #[serde(default = "default_query_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
}

const fn default_query_limit() -> usize {
    500
}
const fn default_timeout_ms() -> u64 {
    30_000
}

impl QueryRequest {
    pub(crate) fn validated_limit(&self) -> Result<usize> {
        if self.query.trim().is_empty() {
            return Err(DataError::InvalidQuery("query cannot be empty".to_owned()));
        }
        if self.offset > 100_000 {
            return Err(DataError::InvalidQuery("offset exceeds 100000".to_owned()));
        }
        Ok(self.limit.clamp(1, 10_000))
    }

    pub(crate) fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms.clamp(100, 120_000))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryColumn {
    pub name: String,
    pub data_type: String,
    pub nullable: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResultPage {
    pub columns: Vec<QueryColumn>,
    pub rows: Vec<Vec<DataValue>>,
    pub stats: QueryStats,
    pub next_offset: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryStats {
    pub elapsed_ms: u64,
    pub rows_returned: usize,
    pub rows_affected: u64,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryClassification {
    ReadOnly,
    Mutation,
    Destructive,
    TransactionControl,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationRisk {
    Low,
    Elevated,
    Destructive,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationPlan {
    #[serde(default = "Uuid::new_v4")]
    pub id: Uuid,
    pub connection_id: Uuid,
    pub statement: String,
    #[serde(default)]
    pub parameters: Vec<DataValue>,
    pub risk: MutationRisk,
    pub approval_token: Option<String>,
    pub approval_expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationResult {
    pub rows_affected: u64,
    pub elapsed_ms: u64,
    pub committed: bool,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AiAssistantRequest {
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_query: Option<String>,
    pub database_kind: DatabaseKind,
    pub query_language: QueryLanguage,
}

impl fmt::Debug for AiAssistantRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AiAssistantRequest")
            .field("prompt_bytes", &self.prompt.len())
            .field(
                "active_query_bytes",
                &self.active_query.as_ref().map(String::len),
            )
            .field("database_kind", &self.database_kind)
            .field("query_language", &self.query_language)
            .finish()
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiChatRequest {
    #[serde(default = "default_ai_model")]
    pub model: String,
    pub messages: Vec<AiMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
}

impl fmt::Debug for AiChatRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AiChatRequest")
            .field("model", &self.model)
            .field("message_count", &self.messages.len())
            .field("temperature", &self.temperature)
            .field("max_tokens", &self.max_tokens)
            .finish()
    }
}

fn default_ai_model() -> String {
    "auto".to_owned()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiRole {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiMessage {
    pub role: AiRole,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiChatResponse {
    pub id: String,
    pub model: String,
    pub message: AiMessage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<AiUsage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[cfg(test)]
mod tests {
    use mongodb::bson::{Bson, Decimal128, oid::ObjectId};

    use super::*;

    #[test]
    fn config_debug_and_display_redact_identity_and_options() {
        let config = ConnectionConfig {
            username: Some("alice@example.test".to_owned()),
            options: BTreeMap::from([("token".to_owned(), "super-secret".to_owned())]),
            ..ConnectionConfig::default()
        };
        for output in [format!("{config:?}"), config.to_string()] {
            assert!(!output.contains("alice"));
            assert!(!output.contains("super-secret"));
        }
    }

    #[test]
    fn profile_rejects_secrets_in_options() {
        for alias in ["password", "pwd", "apiKey", "credential"] {
            let profile = ConnectionProfile {
                id: Uuid::new_v4(),
                name: "db".to_owned(),
                kind: DatabaseKind::PostgreSql,
                config: ConnectionConfig {
                    host: Some("localhost".to_owned()),
                    options: BTreeMap::from([(alias.to_owned(), "nope".to_owned())]),
                    ..ConnectionConfig::default()
                },
                read_only: false,
                built_in: false,
                capabilities: DatabaseKind::PostgreSql.capabilities(),
            };
            assert!(
                profile.validate().is_err(),
                "alias should be rejected: {alias}"
            );
        }
    }

    #[test]
    fn bson_conversion_preserves_special_types() {
        let object_id = ObjectId::new();
        assert_eq!(
            DataValue::from_bson(Bson::Int64(i64::MAX)),
            DataValue::Integer(i64::MAX.to_string())
        );
        assert_eq!(
            DataValue::from_bson(Bson::Decimal128(
                "123456789.0123456789".parse::<Decimal128>().unwrap()
            )),
            DataValue::Decimal("123456789.0123456789".to_owned())
        );
        assert_eq!(
            DataValue::from_bson(Bson::ObjectId(object_id)),
            DataValue::Bson(serde_json::json!({"$oid": object_id.to_hex()}))
        );
        assert_eq!(
            DataValue::from_bson(Bson::ObjectId(object_id))
                .to_bson()
                .unwrap(),
            Bson::ObjectId(object_id)
        );
        let binary = DataValue::from_bson(Bson::Binary(bson::Binary {
            subtype: bson::spec::BinarySubtype::Generic,
            bytes: vec![0, 1, 255],
        }));
        assert_eq!(
            binary.to_bson().unwrap(),
            Bson::Binary(bson::Binary {
                subtype: bson::spec::BinarySubtype::Generic,
                bytes: vec![0, 1, 255]
            })
        );
    }

    #[test]
    fn connection_config_and_ai_model_defaults_remain_wire_compatible() {
        let config: ConnectionConfig = serde_json::from_str("{}").unwrap();
        assert!(!config.srv);

        let request: AiChatRequest = serde_json::from_value(serde_json::json!({
            "messages": [{"role": "user", "content": "hello"}]
        }))
        .unwrap();
        assert_eq!(request.model, "auto");
    }

    #[test]
    fn connector_capabilities_hide_unavailable_desktop_mutations() {
        for kind in [
            DatabaseKind::PostgreSql,
            DatabaseKind::MySql,
            DatabaseKind::SQLite,
            DatabaseKind::MongoDb,
        ] {
            assert!(!kind.capabilities().mutation, "{kind:?}");
        }

        for kind in [
            DatabaseKind::PostgreSql,
            DatabaseKind::MySql,
            DatabaseKind::SQLite,
        ] {
            assert!(kind.capabilities().transactions, "{kind:?}");
            assert!(!kind.capabilities().document_query, "{kind:?}");
        }
        let mongo = DatabaseKind::MongoDb.capabilities();
        assert!(!mongo.transactions);
        assert!(mongo.document_query);
    }

    #[test]
    fn sql_server_capability_is_honest() {
        let capabilities = DatabaseKind::SqlServer.capabilities();
        assert!(!capabilities.connect);
        assert!(capabilities.unsupported_reason.unwrap().contains("preview"));
    }
}
