use crate::db::UserRecord;
use anyhow::{bail, Context, Result};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngCore;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const SESSION_TTL: Duration = Duration::from_secs(8 * 60 * 60);
const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(15 * 60);
const MAX_LOGIN_FAILURES: u32 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Admin,
    Operator,
}

impl Role {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "admin" => Some(Self::Admin),
            "operator" => Some(Self::Operator),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Operator => "operator",
        }
    }

    pub fn can_manage(self) -> bool {
        matches!(self, Self::Admin)
    }
}

#[derive(Clone, Debug)]
pub struct Session {
    pub user_id: i64,
    pub username: String,
    pub role: Role,
    pub csrf_token: String,
    pub expires_at: Instant,
}

#[derive(Clone, Debug)]
struct LoginAttempt {
    window_started: Instant,
    failures: u32,
}

#[derive(Clone)]
pub struct AuthState {
    sessions: std::sync::Arc<Mutex<HashMap<String, Session>>>,
    login_attempts: std::sync::Arc<Mutex<HashMap<String, LoginAttempt>>>,
    pending_fingerprints: std::sync::Arc<Mutex<HashMap<(String, i64), String>>>,
}

impl AuthState {
    pub fn new() -> Self {
        Self {
            sessions: std::sync::Arc::new(Mutex::new(HashMap::new())),
            login_attempts: std::sync::Arc::new(Mutex::new(HashMap::new())),
            pending_fingerprints: std::sync::Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn login_allowed(&self, client_key: &str) -> bool {
        let mut attempts = self.login_attempts.lock().await;
        let now = Instant::now();
        let attempt = attempts
            .entry(client_key.to_owned())
            .or_insert(LoginAttempt {
                window_started: now,
                failures: 0,
            });
        if now.duration_since(attempt.window_started) >= RATE_LIMIT_WINDOW {
            attempt.window_started = now;
            attempt.failures = 0;
        }
        attempt.failures < MAX_LOGIN_FAILURES
    }

    pub async fn record_login_failure(&self, client_key: &str) {
        let mut attempts = self.login_attempts.lock().await;
        let now = Instant::now();
        let attempt = attempts
            .entry(client_key.to_owned())
            .or_insert(LoginAttempt {
                window_started: now,
                failures: 0,
            });
        if now.duration_since(attempt.window_started) >= RATE_LIMIT_WINDOW {
            attempt.window_started = now;
            attempt.failures = 0;
        }
        attempt.failures = attempt.failures.saturating_add(1);
    }

    pub async fn clear_login_failures(&self, client_key: &str) {
        self.login_attempts.lock().await.remove(client_key);
    }

    pub async fn create_session(&self, user: &UserRecord) -> Result<(String, Session)> {
        let role = Role::parse(&user.role).context("invalid role in users table")?;
        let token = random_token();
        let session = Session {
            user_id: user.id,
            username: user.username.clone(),
            role,
            csrf_token: random_token(),
            expires_at: Instant::now() + SESSION_TTL,
        };
        self.sessions
            .lock()
            .await
            .insert(token.clone(), session.clone());
        Ok((token, session))
    }

    pub async fn get_session(&self, token: &str) -> Option<Session> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions.get(token).cloned();
        if session
            .as_ref()
            .is_some_and(|session| session.expires_at <= Instant::now())
        {
            sessions.remove(token);
            return None;
        }
        session
    }

    pub async fn remove_session(&self, token: &str) {
        self.sessions.lock().await.remove(token);
        self.pending_fingerprints
            .lock()
            .await
            .retain(|(session_token, _), _| session_token != token);
    }

    pub async fn set_pending_fingerprint(&self, token: &str, site_id: i64, fingerprint: &str) {
        self.pending_fingerprints
            .lock()
            .await
            .insert((token.to_owned(), site_id), fingerprint.to_owned());
    }

    pub async fn take_pending_fingerprint(&self, token: &str, site_id: i64) -> Option<String> {
        self.pending_fingerprints
            .lock()
            .await
            .remove(&(token.to_owned(), site_id))
    }
}

pub fn hash_password(password: &str) -> Result<String> {
    if password.chars().count() < 12 {
        bail!("password must be at least 12 characters");
    }
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| anyhow::anyhow!("hash password"))
}

pub fn verify_password(password: &str, encoded_hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(encoded_hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

fn random_token() -> String {
    let mut bytes = [0_u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_hash_verifies_without_storing_plaintext() {
        let password = "correct horse battery staple";
        let hash = hash_password(password).expect("hash");
        assert!(!hash.contains(password));
        assert!(verify_password(password, &hash));
        assert!(!verify_password("wrong password", &hash));
    }

    #[test]
    fn role_parser_is_restricted() {
        assert_eq!(Role::parse("admin"), Some(Role::Admin));
        assert_eq!(Role::parse("operator"), Some(Role::Operator));
        assert_eq!(Role::parse("root"), None);
    }
}
