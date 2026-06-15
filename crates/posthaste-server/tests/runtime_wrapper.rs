//! API runtime-wrapper migration tests.
//!
//! spec: docs/eph/PLAN-L3-api-runtime-wrapper-migration#wrapper-fitness-tests

mod support;

use posthaste_runtime_contract::RuntimeLifecycle;
use support::Harness;

#[tokio::test]
async fn api_harness_state_exposes_runtime_handle_status() {
    let harness = Harness::new();

    let status = harness.runtime_status().await;

    assert_eq!(status.lifecycle, RuntimeLifecycle::Ready);
    assert!(status.store.config_loaded);
    assert!(status.store.state_store_open);
    assert!(
        !status.store.cache_root_ready,
        "manual API harnesses attach a legacy graph and do not build cache roots"
    );
}
