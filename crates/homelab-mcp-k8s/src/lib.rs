pub mod download;
pub mod status;

pub use download::{build_download_job, download_job_name, DownloadJobSpec};
pub use status::{
    DownloadJobRef, DownloadStatus, KserveCondition, ModelLogs, ModelStatus, SentinelInfo,
};

#[cfg(test)]
mod tests {
    #[test]
    fn crate_is_ready() {
        assert!(true);
    }
}
