use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ResourceRequests;

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FitConfidence {
    Low,
    Medium,
    High,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct ActiveModelCapacity {
    pub name: String,
    pub namespace: String,
    pub recipe_id: Option<String>,
    pub requested: ResourceRequests,
    pub ready: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct CapacityReport {
    pub target: String,
    pub node_ready: bool,
    pub active_models: Vec<ActiveModelCapacity>,
    pub observed_gpu_utilization_percent: Option<f64>,
    pub observed_gpu_memory_used_bytes: Option<f64>,
    pub observed_gpu_memory_total_bytes: Option<f64>,
    pub risks: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct FitEstimate {
    pub target: String,
    pub fits: bool,
    pub confidence: FitConfidence,
    pub mode: String,
    pub risks: Vec<String>,
    pub recommended_resources: ResourceRequests,
}

pub fn estimate_fit_from_report(
    report: &CapacityReport,
    requested: ResourceRequests,
) -> FitEstimate {
    let mut risks = report.risks.clone();
    if !report.node_ready {
        risks.push("target node is not Ready".into());
    }

    // Simple GPU request sum. With integer gpu_count this is conservative:
    // two models each requesting 1 GPU on a 1-GPU node will not fit.
    // Shared 1-GPU colocation would require fractional gpu_count, which
    // is not yet supported in ResourceRequests.
    let active_gpu: u32 = report
        .active_models
        .iter()
        .map(|model| model.requested.gpu_count)
        .sum();
    let fits = report.node_ready && active_gpu + requested.gpu_count <= 1;

    let confidence = if report.observed_gpu_memory_total_bytes.is_some() {
        FitConfidence::Medium
    } else {
        FitConfidence::Low
    };

    FitEstimate {
        target: report.target.clone(),
        fits,
        confidence,
        mode: if active_gpu == 0 {
            "single-model".into()
        } else {
            "co-locate-small-model".into()
        },
        risks,
        recommended_resources: requested,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_fails_when_node_not_ready() {
        let report = CapacityReport {
            target: "spark".into(),
            node_ready: false,
            active_models: Vec::new(),
            observed_gpu_utilization_percent: None,
            observed_gpu_memory_used_bytes: None,
            observed_gpu_memory_total_bytes: None,
            risks: Vec::new(),
        };
        let fit = estimate_fit_from_report(
            &report,
            ResourceRequests {
                cpu: "2".into(),
                memory: "16Gi".into(),
                gpu_count: 1,
            },
        );
        assert!(!fit.fits);
        assert!(fit.risks.contains(&"target node is not Ready".into()));
    }

    #[test]
    fn fit_confidence_is_medium_when_gpu_memory_total_observed() {
        let report = CapacityReport {
            target: "spark".into(),
            node_ready: true,
            active_models: Vec::new(),
            observed_gpu_utilization_percent: None,
            observed_gpu_memory_used_bytes: None,
            observed_gpu_memory_total_bytes: Some(24_000_000_000.0),
            risks: Vec::new(),
        };
        let fit = estimate_fit_from_report(
            &report,
            ResourceRequests {
                cpu: "2".into(),
                memory: "16Gi".into(),
                gpu_count: 1,
            },
        );
        assert!(fit.fits);
        assert_eq!(fit.confidence, FitConfidence::Medium);
    }

    #[test]
    fn fit_confidence_is_low_when_no_gpu_memory_total() {
        let report = CapacityReport {
            target: "spark".into(),
            node_ready: true,
            active_models: Vec::new(),
            observed_gpu_utilization_percent: None,
            observed_gpu_memory_used_bytes: None,
            observed_gpu_memory_total_bytes: None,
            risks: Vec::new(),
        };
        let fit = estimate_fit_from_report(
            &report,
            ResourceRequests {
                cpu: "2".into(),
                memory: "16Gi".into(),
                gpu_count: 1,
            },
        );
        assert_eq!(fit.confidence, FitConfidence::Low);
    }

    #[test]
    fn fit_mode_is_single_model_when_no_active_workload() {
        let report = CapacityReport {
            target: "spark".into(),
            node_ready: true,
            active_models: Vec::new(),
            observed_gpu_utilization_percent: None,
            observed_gpu_memory_used_bytes: None,
            observed_gpu_memory_total_bytes: None,
            risks: Vec::new(),
        };
        let fit = estimate_fit_from_report(
            &report,
            ResourceRequests {
                cpu: "2".into(),
                memory: "16Gi".into(),
                gpu_count: 1,
            },
        );
        assert_eq!(fit.mode, "single-model");
    }

    #[test]
    fn fit_mode_is_colocate_when_active_gpu_workload_present() {
        let report = CapacityReport {
            target: "spark".into(),
            node_ready: true,
            active_models: vec![ActiveModelCapacity {
                name: "existing-model".into(),
                namespace: "default".into(),
                recipe_id: Some("qwen3-8b".into()),
                requested: ResourceRequests {
                    cpu: "2".into(),
                    memory: "16Gi".into(),
                    gpu_count: 1,
                },
                ready: true,
            }],
            observed_gpu_utilization_percent: None,
            observed_gpu_memory_used_bytes: None,
            observed_gpu_memory_total_bytes: None,
            risks: Vec::new(),
        };
        let fit = estimate_fit_from_report(
            &report,
            ResourceRequests {
                cpu: "2".into(),
                memory: "16Gi".into(),
                gpu_count: 1,
            },
        );
        assert_eq!(fit.mode, "co-locate-small-model");
    }

    #[test]
    fn fit_preserves_report_risks() {
        let report = CapacityReport {
            target: "spark".into(),
            node_ready: true,
            active_models: Vec::new(),
            observed_gpu_utilization_percent: None,
            observed_gpu_memory_used_bytes: None,
            observed_gpu_memory_total_bytes: None,
            risks: vec!["high temperature warning".into()],
        };
        let fit = estimate_fit_from_report(
            &report,
            ResourceRequests {
                cpu: "2".into(),
                memory: "16Gi".into(),
                gpu_count: 1,
            },
        );
        assert!(fit.risks.contains(&"high temperature warning".into()));
    }

    #[test]
    fn fit_includes_requested_resources_as_recommended() {
        let requested = ResourceRequests {
            cpu: "4".into(),
            memory: "32Gi".into(),
            gpu_count: 1,
        };
        let report = CapacityReport {
            target: "spark".into(),
            node_ready: true,
            active_models: Vec::new(),
            observed_gpu_utilization_percent: None,
            observed_gpu_memory_used_bytes: None,
            observed_gpu_memory_total_bytes: None,
            risks: Vec::new(),
        };
        let fit = estimate_fit_from_report(&report, requested.clone());
        assert_eq!(fit.recommended_resources, requested);
    }

    #[test]
    fn integer_gpu_count_is_conservative_for_colocation() {
        // With integer gpu_count where each model requests 1 GPU,
        // a 1-GPU node cannot fit two models under the simple sum heuristic.
        // This documents the current limitation; fractional gpu_count
        // would be needed to support shared 1-GPU colocation.
        let report = CapacityReport {
            target: "spark".into(),
            node_ready: true,
            active_models: vec![ActiveModelCapacity {
                name: "existing".into(),
                namespace: "default".into(),
                recipe_id: None,
                requested: ResourceRequests {
                    cpu: "2".into(),
                    memory: "16Gi".into(),
                    gpu_count: 1,
                },
                ready: true,
            }],
            observed_gpu_utilization_percent: None,
            observed_gpu_memory_used_bytes: None,
            observed_gpu_memory_total_bytes: None,
            risks: Vec::new(),
        };
        let fit = estimate_fit_from_report(
            &report,
            ResourceRequests {
                cpu: "2".into(),
                memory: "16Gi".into(),
                gpu_count: 1,
            },
        );
        // Simple sum: 1 (active) + 1 (requested) > 1 (capacity)
        assert!(!fit.fits);
    }
}
