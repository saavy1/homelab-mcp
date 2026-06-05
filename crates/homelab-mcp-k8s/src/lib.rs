pub mod capacity;
pub mod download;
pub mod live;
pub mod runtime_store;
pub mod status;

pub use capacity::collect_capacity_report;
pub use download::{DownloadJobSpec, build_download_job, download_job_name};
pub use live::{
    create_download_job, create_inferenceservice, delete_inferenceservice,
    dry_run_inferenceservice, get_download_status, get_events, get_inferenceservice_status,
    get_predictor_logs, k8s_client,
};
pub use runtime_store::{
    delete_runtime_recipe, get_runtime_recipe, list_runtime_deployments, list_runtime_recipes,
    upsert_runtime_deployment, upsert_runtime_recipe,
};
pub use status::{
    DownloadJobRef, DownloadStatus, KserveCondition, ModelLogs, ModelStatus, SentinelInfo,
};

#[cfg(test)]
mod tests {
    #[test]
    fn crate_is_ready() {
        // k8s crate modules compile and re-export
    }
}
