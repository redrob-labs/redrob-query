use redrob_core::{
    AiAssistantRequest, AiChatResponse, ConnectionProfile, ConnectionStatus, DataService,
    MetadataNode, MetadataRequest, MutationResult, QueryRequest, QueryResultPage,
    RemoveProfileOutcome, SaveProfileOutcome,
};
use tauri::State;
use uuid::Uuid;

fn message(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[tauri::command]
pub async fn list_connections(
    service: State<'_, DataService>,
) -> Result<Vec<ConnectionProfile>, String> {
    Ok(service.list_profiles().await)
}

#[tauri::command]
pub async fn save_connection_with_secret(
    service: State<'_, DataService>,
    profile: ConnectionProfile,
    secret: Option<String>,
) -> Result<SaveProfileOutcome, String> {
    service
        .save_profile_with_secret_outcome(profile, secret.as_deref())
        .await
        .map_err(message)
}

// `State<'_, T>` is what Tauri's command macro requires: it is a lightweight handle
// the runtime constructs per invocation, and `&State<'_, T>` is not an accepted
// command argument type, so clippy's suggestion here would not compile. This is the
// only synchronous command in the file, which is why it is the only one that trips
// the lint -- an async command's future captures the handle and satisfies it.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
pub fn profile_load_warnings(service: State<'_, DataService>) -> Vec<String> {
    service.profile_load_warnings()
}

#[tauri::command]
pub async fn remove_connection(
    service: State<'_, DataService>,
    id: Uuid,
) -> Result<RemoveProfileOutcome, String> {
    service.remove_profile_outcome(id).await.map_err(message)
}

#[tauri::command]
pub async fn test_connection(
    service: State<'_, DataService>,
    profile: ConnectionProfile,
    secret: Option<String>,
) -> Result<ConnectionStatus, String> {
    service
        .test_connection(&profile, secret.as_deref())
        .await
        .map_err(message)
}

#[tauri::command]
pub async fn connect(
    service: State<'_, DataService>,
    id: Uuid,
) -> Result<ConnectionStatus, String> {
    service.connect(id).await.map_err(message)
}

#[tauri::command]
pub async fn disconnect(service: State<'_, DataService>, id: Uuid) -> Result<(), String> {
    service.disconnect(id).await.map_err(message)
}

#[tauri::command]
pub async fn load_metadata(
    service: State<'_, DataService>,
    request: MetadataRequest,
) -> Result<Vec<MetadataNode>, String> {
    service.load_metadata(request).await.map_err(message)
}

#[tauri::command]
pub async fn execute_query(
    service: State<'_, DataService>,
    request: QueryRequest,
) -> Result<QueryResultPage, String> {
    service.execute_query(request).await.map_err(message)
}

/// The statements of a batch the editor may run one by one, each through `execute_query` and its own
/// single-statement gate. `None` unless every statement is read-only (`query::read_only_statements`),
/// so a batch never widens what may run -- it only lets the grid show each statement's result.
#[tauri::command]
// Tauri deserialises command arguments into owned values; a borrowed `&str` is not an argument type it
// can produce, so the String is taken by value.
#[allow(clippy::needless_pass_by_value)]
pub fn split_read_only_batch(query: String) -> Option<Vec<String>> {
    redrob_core::query::read_only_statements(&query)
}

/// The primary-key columns of a table, so the grid knows whether a result can be edited and how a
/// row is identified. Empty when the table has none: such a result stays read-only.
#[tauri::command]
pub async fn primary_key_columns(
    service: State<'_, DataService>,
    connection_id: Uuid,
    schema: Option<String>,
    table: String,
) -> Result<Vec<String>, String> {
    service
        .primary_key_columns(connection_id, schema, table)
        .await
        .map_err(message)
}

/// Create a table or add a column from the form. Types come from a fixed list (see `redrob_core::ddl`).
#[tauri::command]
pub async fn apply_table_change(
    service: State<'_, DataService>,
    connection_id: Uuid,
    change: redrob_core::ddl::TableChange,
) -> Result<(), String> {
    service
        .apply_table_change(connection_id, change)
        .await
        .map_err(message)
}

/// Apply a change set the person reviewed in the changes panel. The core checks the profile is
/// writable, binds every value, and commits only if every row update hits exactly one row.
#[tauri::command]
pub async fn apply_cell_edits(
    service: State<'_, DataService>,
    connection_id: Uuid,
    edits: redrob_core::cell_edits::CellEditSet,
) -> Result<MutationResult, String> {
    service
        .apply_cell_edits(connection_id, edits)
        .await
        .map_err(message)
}

#[tauri::command]
pub async fn save_ai_secret(service: State<'_, DataService>, secret: String) -> Result<(), String> {
    service.save_ai_secret(&secret).map_err(message)
}

#[tauri::command]
pub async fn ai_chat(
    service: State<'_, DataService>,
    request: AiAssistantRequest,
) -> Result<AiChatResponse, String> {
    service.ai_chat(request).await.map_err(message)
}
