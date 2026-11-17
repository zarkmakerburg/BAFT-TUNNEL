//! Every adapter runs the same contract suite.
use ports_adapters_example::adapters::memory::MemoryAuditStore;
use ports_adapters_example::testing::audit_store_contract;

#[tokio::test]
async fn memory_adapter_honours_the_port_contract() {
    audit_store_contract(MemoryAuditStore::default).await;
}

// A real backend adds, behind its feature flag / env var:
//   #[tokio::test] #[ignore = "needs ClickHouse (testcontainers)"]
//   async fn vendor_adapter_honours_the_port_contract() { audit_store_contract(|| real_store()).await; }
