pub mod download;
pub mod status;

pub use download::{DownloadJobSpec, build_download_job, download_job_name};
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
