pub mod download;
pub mod live;
pub mod status;

pub use download::{DownloadJobSpec, build_download_job, download_job_name};
pub use live::{
    create_download_job, create_inferenceservice, get_download_status, get_events,
    get_inferenceservice_status, get_predictor_logs, k8s_client,
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
