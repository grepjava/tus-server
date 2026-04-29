use tracing::info;

use crate::app_state::AppState;

pub async fn process(state: AppState, upload_id: &str) -> anyhow::Result<()> {
    state.upload_service.begin_processing(upload_id).await?;

    let upload = state.upload_service.get_upload(upload_id).await?;
    let file_path = state.config.storage_dir.join(&upload.storage_path);

    info!(upload_id, path = %file_path.display(), "processing upload");

    // TODO: add processing logic here (validation, transformation, forwarding, etc.)
    // On failure call: state.upload_service.fail_processing(upload_id, &err.to_string()).await?;

    state.upload_service.complete_processing(upload_id).await?;
    Ok(())
}
