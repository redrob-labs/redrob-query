use redrob_core::{
    AiAssistantRequest, AiChatResponse, ConnectionProfile, ConnectionStatus, DataService,
    MetadataNode, MetadataRequest, QueryRequest, QueryResultPage,
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
) -> Result<ConnectionProfile, String> {
    service
        .save_profile_with_secret(profile, secret.as_deref())
        .await
        .map_err(message)
}

#[tauri::command]
pub fn profile_load_warnings(service: State<'_, DataService>) -> Vec<String> {
    service.profile_load_warnings()
}

#[tauri::command]
pub async fn remove_connection(service: State<'_, DataService>, id: Uuid) -> Result<(), String> {
    service.remove_profile(id).await.map_err(message)
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
