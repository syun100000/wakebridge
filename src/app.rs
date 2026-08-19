use crate::auth::AuthState;
use crate::config::AppConfig;
use crate::db::Db;
use crate::providers::{WakeProvider, YamahaRtxProvider};
use crate::secrets::SecretStore;
use anyhow::{Context, Result};
use chrono::Utc;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub db: Db,
    pub secrets: SecretStore,
    pub auth: AuthState,
    pub provider: Arc<dyn WakeProvider>,
    pub started_at: String,
}

impl AppState {
    pub async fn initialize(config: AppConfig) -> Result<Self> {
        std::fs::create_dir_all(&config.data_dir)
            .with_context(|| format!("create {}", config.data_dir.display()))?;
        let secrets = SecretStore::load_or_create(&config.data_dir)?;
        let db = Db::open(&config.database_path()).await?;
        ensure_default_settings(&db).await?;
        Ok(Self {
            config,
            db,
            secrets,
            auth: AuthState::new(),
            provider: Arc::new(YamahaRtxProvider),
            started_at: Utc::now().to_rfc3339(),
        })
    }

    pub async fn cookie_secure(&self) -> bool {
        if std::env::var("WAKEBRIDGE_DEV_INSECURE_COOKIE").as_deref() == Ok("1") {
            return false;
        }
        self.db
            .get_setting("cookie_secure")
            .await
            .ok()
            .flatten()
            .and_then(|value| value.parse::<bool>().ok())
            .unwrap_or(true)
    }

    pub async fn site_credential(&self, site_id: i64) -> Result<String> {
        let secret = self
            .db
            .get_secret(site_id)
            .await?
            .context("SSH credential is not configured for this site")?;
        if secret.kind != "ssh_password" {
            anyhow::bail!("unsupported site credential type");
        }
        self.secrets
            .decrypt(&secret.nonce, &secret.ciphertext)
            .context("decrypt site SSH credential")
    }
}

async fn ensure_default_settings(db: &Db) -> Result<()> {
    if db.get_setting("site_title").await?.is_none() {
        db.set_setting("site_title", "WakeBridge").await?;
    }
    if db.get_setting("cookie_secure").await?.is_none() {
        db.set_setting("cookie_secure", "true").await?;
    }
    Ok(())
}
