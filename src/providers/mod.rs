mod yamaha;

use crate::db::{DeviceRecord, SiteRecord};
use anyhow::Result;
use async_trait::async_trait;
pub use yamaha::{normalize_ip, normalize_mac, validate_fingerprint, YamahaRtxProvider};

#[derive(Clone, Debug)]
pub struct ConnectionCheck {
    pub fingerprint: String,
    pub detail: String,
}

#[derive(Clone, Debug)]
pub struct WakeResult {
    pub detail: String,
}

#[async_trait]
pub trait WakeProvider: Send + Sync {
    async fn test_connection(
        &self,
        site: SiteRecord,
        credential: String,
    ) -> Result<ConnectionCheck>;

    async fn wake(
        &self,
        site: SiteRecord,
        device: DeviceRecord,
        credential: String,
    ) -> Result<WakeResult>;
}
