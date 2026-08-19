use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

const MIGRATION_SQL: &str = r#"
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('admin', 'operator')),
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sites (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    provider TEXT NOT NULL CHECK (provider = 'yamaha_rtx'),
    router_host TEXT NOT NULL,
    ssh_port INTEGER NOT NULL DEFAULT 22,
    lan_interface TEXT NOT NULL,
    ssh_username TEXT NOT NULL,
    ssh_host_key_fingerprint TEXT,
    allow_legacy_ssh INTEGER NOT NULL DEFAULT 0,
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS devices (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    site_id INTEGER NOT NULL REFERENCES sites(id) ON DELETE CASCADE,
    mac_address TEXT NOT NULL,
    ip_address TEXT,
    description TEXT NOT NULL DEFAULT '',
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (site_id, mac_address)
);

CREATE TABLE IF NOT EXISTS wake_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER REFERENCES users(id) ON DELETE SET NULL,
    device_id INTEGER NOT NULL,
    site_id INTEGER NOT NULL,
    status TEXT NOT NULL,
    message TEXT NOT NULL,
    occurred_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS audit_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER REFERENCES users(id) ON DELETE SET NULL,
    action TEXT NOT NULL,
    target_type TEXT NOT NULL,
    target_id INTEGER,
    details TEXT NOT NULL DEFAULT '',
    occurred_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS secrets (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    site_id INTEGER NOT NULL UNIQUE REFERENCES sites(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    nonce BLOB NOT NULL,
    ciphertext BLOB NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_devices_site_id ON devices(site_id);
CREATE INDEX IF NOT EXISTS idx_wake_events_occurred_at ON wake_events(occurred_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_events_occurred_at ON audit_events(occurred_at DESC);
"#;

#[derive(Clone)]
pub struct Db {
    connection: Arc<Mutex<Connection>>,
}

#[derive(Clone, Debug)]
pub struct UserRecord {
    pub id: i64,
    pub username: String,
    pub password_hash: String,
    pub role: String,
    pub enabled: bool,
}

#[derive(Clone, Debug)]
pub struct SiteRecord {
    pub id: i64,
    pub name: String,
    pub provider: String,
    pub router_host: String,
    pub ssh_port: u16,
    pub lan_interface: String,
    pub ssh_username: String,
    pub ssh_host_key_fingerprint: Option<String>,
    pub allow_legacy_ssh: bool,
    pub enabled: bool,
}

#[derive(Clone, Debug)]
pub struct DeviceRecord {
    pub id: i64,
    pub name: String,
    pub site_id: i64,
    pub site_name: String,
    pub mac_address: String,
    pub ip_address: Option<String>,
    pub description: String,
    pub enabled: bool,
}

#[derive(Clone, Debug)]
pub struct WakeEventRecord {
    pub username: Option<String>,
    pub device_name: String,
    pub site_name: String,
    pub status: String,
    pub message: String,
    pub occurred_at: String,
}

#[derive(Clone, Debug)]
pub struct AuditEventRecord {
    pub username: Option<String>,
    pub action: String,
    pub target_type: String,
    pub target_id: Option<i64>,
    pub details: String,
    pub occurred_at: String,
}

#[derive(Clone, Debug)]
pub struct SecretRecord {
    pub kind: String,
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

impl Db {
    pub async fn open(path: &Path) -> Result<Self> {
        let path = path.to_path_buf();
        let connection = tokio::task::spawn_blocking(move || -> Result<Connection> {
            let connection = Connection::open(&path)
                .with_context(|| format!("open SQLite database {}", path.display()))?;
            connection
                .execute_batch(MIGRATION_SQL)
                .context("apply SQLite migrations")?;
            connection
                .pragma_update(None, "journal_mode", "WAL")
                .context("enable SQLite WAL mode")?;
            connection
                .pragma_update(None, "foreign_keys", "ON")
                .context("enable SQLite foreign keys")?;
            Ok(connection)
        })
        .await
        .context("join SQLite open task")??;

        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    async fn with_connection<T>(
        &self,
        operation: impl FnOnce(&Connection) -> Result<T>,
    ) -> Result<T> {
        let connection = self.connection.lock().await;
        operation(&connection)
    }

    pub async fn find_user(&self, username: &str) -> Result<Option<UserRecord>> {
        let username = username.to_owned();
        self.with_connection(move |connection| {
            connection
                .query_row(
                    "SELECT id, username, password_hash, role, enabled FROM users WHERE username = ?1",
                    params![username],
                    |row| {
                        Ok(UserRecord {
                            id: row.get(0)?,
                            username: row.get(1)?,
                            password_hash: row.get(2)?,
                            role: row.get(3)?,
                            enabled: row.get::<_, i64>(4)? != 0,
                        })
                    },
                )
                .optional()
                .context("find user")
        })
        .await
    }

    pub async fn list_users(&self) -> Result<Vec<UserRecord>> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT id, username, password_hash, role, enabled
                 FROM users ORDER BY username",
            )?;
            let rows = statement.query_map([], |row| {
                Ok(UserRecord {
                    id: row.get(0)?,
                    username: row.get(1)?,
                    password_hash: row.get(2)?,
                    role: row.get(3)?,
                    enabled: row.get::<_, i64>(4)? != 0,
                })
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .context("list users")
        })
        .await
    }

    pub async fn insert_user(
        &self,
        username: &str,
        password_hash: &str,
        role: &str,
    ) -> Result<i64> {
        let username = username.to_owned();
        let password_hash = password_hash.to_owned();
        let role = role.to_owned();
        let now = Utc::now().to_rfc3339();
        self.with_connection(move |connection| {
            connection.execute(
                "INSERT INTO users (username, password_hash, role, enabled, created_at, updated_at)
                 VALUES (?1, ?2, ?3, 1, ?4, ?4)",
                params![username, password_hash, role, now],
            )?;
            Ok(connection.last_insert_rowid())
        })
        .await
    }

    pub async fn update_password(&self, username: &str, password_hash: &str) -> Result<bool> {
        let username = username.to_owned();
        let password_hash = password_hash.to_owned();
        let now = Utc::now().to_rfc3339();
        self.with_connection(move |connection| {
            let changed = connection.execute(
                "UPDATE users SET password_hash = ?1, updated_at = ?2 WHERE username = ?3",
                params![password_hash, now, username],
            )?;
            Ok(changed == 1)
        })
        .await
    }

    pub async fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let key = key.to_owned();
        self.with_connection(move |connection| {
            connection
                .query_row(
                    "SELECT value FROM settings WHERE key = ?1",
                    params![key],
                    |row| row.get(0),
                )
                .optional()
                .context("get setting")
        })
        .await
    }

    pub async fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let key = key.to_owned();
        let value = value.to_owned();
        let now = Utc::now().to_rfc3339();
        self.with_connection(move |connection| {
            connection.execute(
                "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
                params![key, value, now],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn list_sites(&self) -> Result<Vec<SiteRecord>> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT id, name, provider, router_host, ssh_port, lan_interface,
                        ssh_username, ssh_host_key_fingerprint, allow_legacy_ssh, enabled
                 FROM sites ORDER BY name",
            )?;
            let rows = statement.query_map([], |row| {
                Ok(SiteRecord {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    provider: row.get(2)?,
                    router_host: row.get(3)?,
                    ssh_port: row.get::<_, i64>(4)? as u16,
                    lan_interface: row.get(5)?,
                    ssh_username: row.get(6)?,
                    ssh_host_key_fingerprint: row.get(7)?,
                    allow_legacy_ssh: row.get::<_, i64>(8)? != 0,
                    enabled: row.get::<_, i64>(9)? != 0,
                })
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .context("list sites")
        })
        .await
    }

    pub async fn get_site(&self, id: i64) -> Result<Option<SiteRecord>> {
        self.with_connection(move |connection| {
            connection
                .query_row(
                    "SELECT id, name, provider, router_host, ssh_port, lan_interface,
                            ssh_username, ssh_host_key_fingerprint, allow_legacy_ssh, enabled
                     FROM sites WHERE id = ?1",
                    params![id],
                    |row| {
                        Ok(SiteRecord {
                            id: row.get(0)?,
                            name: row.get(1)?,
                            provider: row.get(2)?,
                            router_host: row.get(3)?,
                            ssh_port: row.get::<_, i64>(4)? as u16,
                            lan_interface: row.get(5)?,
                            ssh_username: row.get(6)?,
                            ssh_host_key_fingerprint: row.get(7)?,
                            allow_legacy_ssh: row.get::<_, i64>(8)? != 0,
                            enabled: row.get::<_, i64>(9)? != 0,
                        })
                    },
                )
                .optional()
                .context("get site")
        })
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_site(
        &self,
        name: &str,
        provider: &str,
        router_host: &str,
        ssh_port: u16,
        lan_interface: &str,
        ssh_username: &str,
        allow_legacy_ssh: bool,
    ) -> Result<i64> {
        let now = Utc::now().to_rfc3339();
        let values = (
            name.to_owned(),
            provider.to_owned(),
            router_host.to_owned(),
            ssh_port as i64,
            lan_interface.to_owned(),
            ssh_username.to_owned(),
            i64::from(allow_legacy_ssh),
            now,
        );
        self.with_connection(move |connection| {
            connection.execute(
                "INSERT INTO sites
                 (name, provider, router_host, ssh_port, lan_interface, ssh_username,
                  allow_legacy_ssh, enabled, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8, ?8)",
                params![
                    values.0, values.1, values.2, values.3, values.4, values.5, values.6, values.7
                ],
            )?;
            Ok(connection.last_insert_rowid())
        })
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_site(
        &self,
        id: i64,
        name: &str,
        router_host: &str,
        ssh_port: u16,
        lan_interface: &str,
        ssh_username: &str,
        allow_legacy_ssh: bool,
        enabled: bool,
    ) -> Result<bool> {
        let now = Utc::now().to_rfc3339();
        let values = (
            id,
            name.to_owned(),
            router_host.to_owned(),
            ssh_port as i64,
            lan_interface.to_owned(),
            ssh_username.to_owned(),
            i64::from(allow_legacy_ssh),
            i64::from(enabled),
            now,
        );
        self.with_connection(move |connection| {
            let changed = connection.execute(
                "UPDATE sites SET name = ?2, router_host = ?3, ssh_port = ?4,
                 lan_interface = ?5, ssh_username = ?6, allow_legacy_ssh = ?7,
                 enabled = ?8, updated_at = ?9 WHERE id = ?1",
                params![
                    values.0, values.1, values.2, values.3, values.4, values.5, values.6, values.7,
                    values.8
                ],
            )?;
            Ok(changed == 1)
        })
        .await
    }

    pub async fn trust_site_fingerprint(&self, id: i64, fingerprint: &str) -> Result<bool> {
        let fingerprint = fingerprint.to_owned();
        let now = Utc::now().to_rfc3339();
        self.with_connection(move |connection| {
            let changed = connection.execute(
                "UPDATE sites SET ssh_host_key_fingerprint = ?1, updated_at = ?2 WHERE id = ?3",
                params![fingerprint, now, id],
            )?;
            Ok(changed == 1)
        })
        .await
    }

    pub async fn clear_site_fingerprint(&self, id: i64) -> Result<bool> {
        let now = Utc::now().to_rfc3339();
        self.with_connection(move |connection| {
            let changed = connection.execute(
                "UPDATE sites SET ssh_host_key_fingerprint = NULL, updated_at = ?1 WHERE id = ?2",
                params![now, id],
            )?;
            Ok(changed == 1)
        })
        .await
    }

    pub async fn delete_site(&self, id: i64) -> Result<bool> {
        self.with_connection(move |connection| {
            let changed = connection.execute("DELETE FROM sites WHERE id = ?1", params![id])?;
            Ok(changed == 1)
        })
        .await
    }

    pub async fn list_devices(&self) -> Result<Vec<DeviceRecord>> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT d.id, d.name, d.site_id, s.name, d.mac_address, d.ip_address,
                        d.description, d.enabled
                 FROM devices d JOIN sites s ON s.id = d.site_id
                 ORDER BY s.name, d.name",
            )?;
            let rows = statement.query_map([], |row| {
                Ok(DeviceRecord {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    site_id: row.get(2)?,
                    site_name: row.get(3)?,
                    mac_address: row.get(4)?,
                    ip_address: row.get(5)?,
                    description: row.get(6)?,
                    enabled: row.get::<_, i64>(7)? != 0,
                })
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .context("list devices")
        })
        .await
    }

    pub async fn get_device(&self, id: i64) -> Result<Option<DeviceRecord>> {
        self.with_connection(move |connection| {
            connection
                .query_row(
                    "SELECT d.id, d.name, d.site_id, s.name, d.mac_address, d.ip_address,
                            d.description, d.enabled
                     FROM devices d JOIN sites s ON s.id = d.site_id WHERE d.id = ?1",
                    params![id],
                    |row| {
                        Ok(DeviceRecord {
                            id: row.get(0)?,
                            name: row.get(1)?,
                            site_id: row.get(2)?,
                            site_name: row.get(3)?,
                            mac_address: row.get(4)?,
                            ip_address: row.get(5)?,
                            description: row.get(6)?,
                            enabled: row.get::<_, i64>(7)? != 0,
                        })
                    },
                )
                .optional()
                .context("get device")
        })
        .await
    }

    pub async fn insert_device(
        &self,
        name: &str,
        site_id: i64,
        mac_address: &str,
        ip_address: Option<&str>,
        description: &str,
    ) -> Result<i64> {
        let now = Utc::now().to_rfc3339();
        let values = (
            name.to_owned(),
            site_id,
            mac_address.to_owned(),
            ip_address.map(ToOwned::to_owned),
            description.to_owned(),
            now,
        );
        self.with_connection(move |connection| {
            connection.execute(
                "INSERT INTO devices
                 (name, site_id, mac_address, ip_address, description, enabled, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?6)",
                params![
                    values.0,
                    values.1,
                    values.2,
                    values.3,
                    values.4,
                    values.5
                ],
            )?;
            Ok(connection.last_insert_rowid())
        })
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_device(
        &self,
        id: i64,
        name: &str,
        site_id: i64,
        mac_address: &str,
        ip_address: Option<&str>,
        description: &str,
        enabled: bool,
    ) -> Result<bool> {
        let now = Utc::now().to_rfc3339();
        let values = (
            id,
            name.to_owned(),
            site_id,
            mac_address.to_owned(),
            ip_address.map(ToOwned::to_owned),
            description.to_owned(),
            i64::from(enabled),
            now,
        );
        self.with_connection(move |connection| {
            let changed = connection.execute(
                "UPDATE devices SET name = ?2, site_id = ?3, mac_address = ?4,
                 ip_address = ?5, description = ?6, enabled = ?7, updated_at = ?8
                 WHERE id = ?1",
                params![
                    values.0, values.1, values.2, values.3, values.4, values.5, values.6, values.7
                ],
            )?;
            Ok(changed == 1)
        })
        .await
    }

    pub async fn delete_device(&self, id: i64) -> Result<bool> {
        self.with_connection(move |connection| {
            let changed = connection.execute("DELETE FROM devices WHERE id = ?1", params![id])?;
            Ok(changed == 1)
        })
        .await
    }

    pub async fn upsert_secret(
        &self,
        site_id: i64,
        kind: &str,
        nonce: &[u8],
        ciphertext: &[u8],
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.with_connection(move |connection| {
            connection.execute(
                "INSERT INTO secrets (site_id, kind, nonce, ciphertext, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(site_id) DO UPDATE SET kind = excluded.kind,
                 nonce = excluded.nonce, ciphertext = excluded.ciphertext,
                 updated_at = excluded.updated_at",
                params![site_id, kind, nonce, ciphertext, now],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn get_secret(&self, site_id: i64) -> Result<Option<SecretRecord>> {
        self.with_connection(move |connection| {
            connection
                .query_row(
                    "SELECT kind, nonce, ciphertext FROM secrets WHERE site_id = ?1",
                    params![site_id],
                    |row| {
                        Ok(SecretRecord {
                            kind: row.get(0)?,
                            nonce: row.get(1)?,
                            ciphertext: row.get(2)?,
                        })
                    },
                )
                .optional()
                .context("get site secret")
        })
        .await
    }

    pub async fn insert_wake_event(
        &self,
        user_id: Option<i64>,
        device_id: i64,
        site_id: i64,
        status: &str,
        message: &str,
    ) -> Result<i64> {
        let now = Utc::now().to_rfc3339();
        let status = status.to_owned();
        let message = message.to_owned();
        self.with_connection(move |connection| {
            connection.execute(
                "INSERT INTO wake_events
                 (user_id, device_id, site_id, status, message, occurred_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![user_id, device_id, site_id, status, message, now],
            )?;
            Ok(connection.last_insert_rowid())
        })
        .await
    }

    pub async fn list_wake_events(&self, limit: u32) -> Result<Vec<WakeEventRecord>> {
        let limit = i64::from(limit.min(200));
        self.with_connection(move |connection| {
            let mut statement = connection.prepare(
                "SELECT w.id, u.username, d.name, s.name, w.status, w.message, w.occurred_at
                 FROM wake_events w
                 LEFT JOIN users u ON u.id = w.user_id
                 JOIN devices d ON d.id = w.device_id
                 JOIN sites s ON s.id = w.site_id
                 ORDER BY w.occurred_at DESC LIMIT ?1",
            )?;
            let rows = statement.query_map(params![limit], |row| {
                Ok(WakeEventRecord {
                    username: row.get(1)?,
                    device_name: row.get(2)?,
                    site_name: row.get(3)?,
                    status: row.get(4)?,
                    message: row.get(5)?,
                    occurred_at: row.get(6)?,
                })
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .context("list wake events")
        })
        .await
    }

    pub async fn insert_audit_event(
        &self,
        user_id: Option<i64>,
        action: &str,
        target_type: &str,
        target_id: Option<i64>,
        details: &str,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let action = action.to_owned();
        let target_type = target_type.to_owned();
        let details = details.to_owned();
        self.with_connection(move |connection| {
            connection.execute(
                "INSERT INTO audit_events
                 (user_id, action, target_type, target_id, details, occurred_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![user_id, action, target_type, target_id, details, now],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn list_audit_events(&self, limit: u32) -> Result<Vec<AuditEventRecord>> {
        let limit = i64::from(limit.min(200));
        self.with_connection(move |connection| {
            let mut statement = connection.prepare(
                "SELECT a.id, u.username, a.action, a.target_type, a.target_id,
                        a.details, a.occurred_at
                 FROM audit_events a LEFT JOIN users u ON u.id = a.user_id
                 ORDER BY a.occurred_at DESC LIMIT ?1",
            )?;
            let rows = statement.query_map(params![limit], |row| {
                Ok(AuditEventRecord {
                    username: row.get(1)?,
                    action: row.get(2)?,
                    target_type: row.get(3)?,
                    target_id: row.get(4)?,
                    details: row.get(5)?,
                    occurred_at: row.get(6)?,
                })
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .context("list audit events")
        })
        .await
    }
}
