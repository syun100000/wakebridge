use crate::app::AppState;
use crate::auth::{hash_password, verify_password, Role, Session};
use crate::db::{AuditEventRecord, DeviceRecord, SiteRecord, UserRecord, WakeEventRecord};
use crate::providers::{normalize_ip, normalize_mac, validate_fingerprint};
use anyhow::{Context, Result};
use askama::Template;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Form, Json, Router,
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use tower_http::{timeout::TimeoutLayer, trace::TraceLayer};
use tracing::{error, info};

const SESSION_COOKIE: &str = "wakebridge_session";

#[derive(Clone, Debug)]
pub struct SiteView {
    pub id: i64,
    pub name: String,
    pub router_host: String,
    pub ssh_port: u16,
    pub lan_interface: String,
    pub ssh_username: String,
    pub fingerprint: String,
    pub trusted: bool,
    pub credential_present: bool,
    pub allow_legacy_ssh: bool,
    pub enabled: bool,
}

#[derive(Clone, Debug)]
pub struct DeviceView {
    pub id: i64,
    pub name: String,
    pub site_id: i64,
    pub site_name: String,
    pub mac_address: String,
    pub ip_address: String,
    pub description: String,
    pub enabled: bool,
}

#[derive(Clone, Debug)]
pub struct SiteOption {
    pub id: i64,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct UserView {
    pub username: String,
    pub role: String,
    pub enabled: bool,
}

#[derive(Clone, Debug)]
pub struct WakeView {
    pub username: String,
    pub device_name: String,
    pub site_name: String,
    pub status: String,
    pub status_label: String,
    pub message: String,
    pub occurred_at: String,
}

#[derive(Clone, Debug)]
pub struct AuditView {
    pub username: String,
    pub action: String,
    pub target_type: String,
    pub target_id: String,
    pub details: String,
    pub occurred_at: String,
}

#[derive(Template)]
#[template(path = "login.html")]
struct LoginTemplate {
    error: String,
}

#[derive(Template)]
#[template(path = "dashboard.html")]
struct DashboardTemplate {
    title: String,
    username: String,
    role: String,
    csrf: String,
    service_mode: String,
    started_at: String,
    site_count: usize,
    device_count: usize,
    recent_events: Vec<WakeView>,
}

#[derive(Template)]
#[template(path = "sites.html")]
struct SitesTemplate {
    title: String,
    username: String,
    role: String,
    csrf: String,
    message: String,
    fingerprint: String,
    sites: Vec<SiteView>,
}

#[derive(Template)]
#[template(path = "devices.html")]
struct DevicesTemplate {
    title: String,
    username: String,
    role: String,
    csrf: String,
    message: String,
    devices: Vec<DeviceView>,
    site_options: Vec<SiteOption>,
}

#[derive(Template)]
#[template(path = "users.html")]
struct UsersTemplate {
    title: String,
    username: String,
    role: String,
    csrf: String,
    message: String,
    users: Vec<UserView>,
}

#[derive(Template)]
#[template(path = "history.html")]
struct HistoryTemplate {
    title: String,
    username: String,
    role: String,
    wake_events: Vec<WakeView>,
    audit_events: Vec<AuditView>,
}

#[derive(Template)]
#[template(path = "settings.html")]
struct SettingsTemplate {
    title: String,
    username: String,
    role: String,
    csrf: String,
    message: String,
    cookie_secure: bool,
}

#[derive(Template)]
#[template(path = "account_password.html")]
struct AccountPasswordTemplate {
    title: String,
    username: String,
    role: String,
    csrf: String,
    message: String,
}

#[derive(Deserialize, Default)]
struct NoticeQuery {
    notice: Option<String>,
}

#[derive(Deserialize)]
struct LoginForm {
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct CsrfForm {
    csrf: String,
}

#[derive(Deserialize)]
struct SiteForm {
    csrf: String,
    name: String,
    provider: String,
    router_host: String,
    ssh_port: String,
    lan_interface: String,
    ssh_username: String,
    ssh_credential: String,
    allow_legacy_ssh: Option<String>,
    enabled: Option<String>,
}

#[derive(Deserialize)]
struct FingerprintForm {
    csrf: String,
    fingerprint: String,
}

#[derive(Deserialize)]
struct DeviceForm {
    csrf: String,
    name: String,
    site_id: String,
    mac_address: String,
    ip_address: String,
    description: String,
    enabled: Option<String>,
}

#[derive(Deserialize)]
struct UserForm {
    csrf: String,
    username: String,
    password: String,
    role: String,
}

#[derive(Deserialize)]
struct ResetPasswordForm {
    csrf: String,
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct ChangePasswordForm {
    csrf: String,
    current_password: String,
    new_password: String,
    confirm_password: String,
}

#[derive(Deserialize)]
struct SettingsForm {
    csrf: String,
    site_title: String,
    cookie_secure: Option<String>,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    service: &'static str,
    mode: &'static str,
    started_at: String,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(dashboard))
        .route("/login", get(login_page).post(login_submit))
        .route("/logout", post(logout))
        .route(
            "/account/password",
            get(account_password_page).post(account_password_submit),
        )
        .route("/sites", get(sites_page).post(site_create))
        .route("/sites/:id/update", post(site_update))
        .route("/sites/:id/delete", post(site_delete))
        .route("/sites/:id/test", post(site_test))
        .route("/sites/:id/trust", post(site_trust))
        .route("/devices", get(devices_page).post(device_create))
        .route("/devices/:id/update", post(device_update))
        .route("/devices/:id/delete", post(device_delete))
        .route("/devices/:id/wake", post(device_wake))
        .route("/users", get(users_page).post(user_create))
        .route("/users/reset", post(user_reset_password))
        .route("/history", get(history_page))
        .route("/settings", get(settings_page).post(settings_save))
        .route("/static/style.css", get(style_css))
        .route("/static/app.js", get(app_js))
        .route("/api/health", get(api_health))
        .route("/api/service-status", get(api_service_status))
        .route("/api/devices/:id/wake", post(api_device_wake))
        .with_state(state)
        .layer(TraceLayer::new_for_http())
        .layer(TimeoutLayer::new(std::time::Duration::from_secs(30)))
}

pub async fn serve_foreground(
    state: AppState,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(state.config.listen)
        .await
        .with_context(|| format!("bind {}", state.config.listen))?;
    info!(address = %state.config.listen, "WakeBridge HTTP server started");
    axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown)
    .await
    .context("HTTP server")?;
    Ok(())
}

async fn login_page() -> Response {
    render(LoginTemplate {
        error: String::new(),
    })
}

async fn login_submit(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<LoginForm>,
) -> Response {
    if !state.auth.login_allowed("local-proxy").await {
        return render(LoginTemplate {
            error: "ログイン試行回数が上限に達しました。15分後に再試行してください。".to_owned(),
        });
    }
    let user = state.db.find_user(form.username.trim()).await;
    let valid = match user {
        Ok(Some(user)) if user.enabled => verify_password(&form.password, &user.password_hash),
        _ => false,
    };
    if !valid {
        state.auth.record_login_failure("local-proxy").await;
        let _ = state
            .db
            .insert_audit_event(None, "login_failed", "auth", None, "invalid credentials")
            .await;
        return render(LoginTemplate {
            error: "ユーザー名またはパスワードが正しくありません。".to_owned(),
        });
    }
    let user = state
        .db
        .find_user(form.username.trim())
        .await
        .ok()
        .flatten();
    let Some(user) = user else {
        return render(LoginTemplate {
            error: "ログイン処理に失敗しました。".to_owned(),
        });
    };
    let (token, _) = match state.auth.create_session(&user).await {
        Ok(session) => session,
        Err(error) => return internal_error(error, "create session"),
    };
    state.auth.clear_login_failures("local-proxy").await;
    let _ = state
        .db
        .insert_audit_event(Some(user.id), "login_success", "auth", None, "")
        .await;
    let cookie = session_cookie(&token, state.cookie_secure().await);
    (jar.add(cookie), Redirect::to("/")).into_response()
}

async fn logout(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<CsrfForm>,
) -> Response {
    let Ok((token, session)) = session_or_redirect(&state, &jar).await else {
        return Redirect::to("/login").into_response();
    };
    if !csrf_valid(&session, &form.csrf) {
        return forbidden("CSRF token is invalid");
    }
    state.auth.remove_session(&token).await;
    let removal = Cookie::build(SESSION_COOKIE).path("/").build();
    (jar.remove(removal), Redirect::to("/login")).into_response()
}

async fn account_password_page(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<NoticeQuery>,
) -> Response {
    let Ok((_, session)) = session_or_redirect(&state, &jar).await else {
        return Redirect::to("/login").into_response();
    };
    render_account_password(&state, &session, query.notice.unwrap_or_default()).await
}

async fn account_password_submit(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<ChangePasswordForm>,
) -> Response {
    let Ok((_, session)) = session_or_redirect(&state, &jar).await else {
        return Redirect::to("/login").into_response();
    };
    if !csrf_valid(&session, &form.csrf) {
        return forbidden("CSRFトークンが不正です。");
    }
    let user = match state.db.find_user(&session.username).await {
        Ok(Some(user)) => user,
        Ok(None) => return forbidden("ユーザーが見つかりません。再度ログインしてください。"),
        Err(error) => return internal_error(error, "find current user"),
    };
    if !verify_password(&form.current_password, &user.password_hash) {
        return render_account_password(
            &state,
            &session,
            "現在のパスワードが正しくありません。".to_owned(),
        )
        .await;
    }
    if form.new_password != form.confirm_password {
        return render_account_password(
            &state,
            &session,
            "新しいパスワードと確認入力が一致しません。".to_owned(),
        )
        .await;
    }
    let password_hash = match hash_password(&form.new_password) {
        Ok(value) => value,
        Err(_) => {
            return render_account_password(
                &state,
                &session,
                "パスワードは12文字以上で指定してください。".to_owned(),
            )
            .await
        }
    };
    match state
        .db
        .update_password(&session.username, &password_hash)
        .await
    {
        Ok(true) => {
            let _ = state
                .db
                .insert_audit_event(
                    Some(session.user_id),
                    "user_password_changed",
                    "user",
                    Some(session.user_id),
                    "",
                )
                .await;
            redirect_notice("/account/password", "パスワードを変更しました。")
        }
        Ok(false) => {
            render_account_password(&state, &session, "ユーザーが見つかりません。".to_owned()).await
        }
        Err(error) => render_account_password(&state, &session, db_error_message(error)).await,
    }
}

async fn dashboard(State(state): State<AppState>, jar: CookieJar) -> Response {
    let Ok((_, session)) = session_or_redirect(&state, &jar).await else {
        return Redirect::to("/login").into_response();
    };
    let sites = match state.db.list_sites().await {
        Ok(sites) => sites,
        Err(error) => return internal_error(error, "list sites"),
    };
    let devices = match state.db.list_devices().await {
        Ok(devices) => devices,
        Err(error) => return internal_error(error, "list devices"),
    };
    let recent_events = match state.db.list_wake_events(10).await {
        Ok(events) => events.into_iter().map(wake_view).collect(),
        Err(error) => return internal_error(error, "list wake events"),
    };
    let title = state
        .db
        .get_setting("site_title")
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| "WakeBridge".to_owned());
    render(DashboardTemplate {
        title,
        username: session.username,
        role: session.role.as_str().to_owned(),
        csrf: session.csrf_token,
        service_mode: if state.config.service_mode {
            "Windowsサービス".to_owned()
        } else {
            "フォアグラウンド".to_owned()
        },
        started_at: state.started_at,
        site_count: sites.len(),
        device_count: devices.len(),
        recent_events,
    })
}

async fn sites_page(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<NoticeQuery>,
) -> Response {
    let Ok((_, session)) = session_or_redirect(&state, &jar).await else {
        return Redirect::to("/login").into_response();
    };
    if !session.role.can_manage() {
        return forbidden("admin role is required");
    }
    render_sites(
        &state,
        &session,
        query.notice.unwrap_or_default(),
        String::new(),
    )
    .await
}

async fn site_create(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<SiteForm>,
) -> Response {
    let Ok((_, session)) = session_or_redirect(&state, &jar).await else {
        return Redirect::to("/login").into_response();
    };
    if !session.role.can_manage() {
        return forbidden("admin role is required");
    }
    if !csrf_valid(&session, &form.csrf) {
        return forbidden("CSRF token is invalid");
    }
    let values = match validate_site_form(&form, true) {
        Ok(values) => values,
        Err(message) => return render_sites(&state, &session, message, String::new()).await,
    };
    let site_id = match state
        .db
        .insert_site(
            &values.name,
            "yamaha_rtx",
            &values.router_host,
            values.ssh_port,
            &values.lan_interface,
            &values.ssh_username,
            values.allow_legacy_ssh,
        )
        .await
    {
        Ok(id) => id,
        Err(error) => {
            return render_sites(&state, &session, db_error_message(error), String::new()).await
        }
    };
    let encrypted = match state.secrets.encrypt(&form.ssh_credential) {
        Ok(value) => value,
        Err(error) => {
            let _ = state.db.delete_site(site_id).await;
            return internal_error(error, "encrypt site credential");
        }
    };
    if let Err(error) = state
        .db
        .upsert_secret(
            site_id,
            "ssh_password",
            &encrypted.nonce,
            &encrypted.ciphertext,
        )
        .await
    {
        let _ = state.db.delete_site(site_id).await;
        return internal_error(error, "store site credential");
    }
    let _ = state
        .db
        .insert_audit_event(
            Some(session.user_id),
            "site_created",
            "site",
            Some(site_id),
            "",
        )
        .await;
    redirect_notice(
        "/sites",
        "拠点を登録しました。SSHホスト鍵は接続テスト後に信頼してください。",
    )
}

async fn site_update(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
    Form(form): Form<SiteForm>,
) -> Response {
    let Ok((_, session)) = session_or_redirect(&state, &jar).await else {
        return Redirect::to("/login").into_response();
    };
    if !session.role.can_manage() {
        return forbidden("admin role is required");
    }
    if !csrf_valid(&session, &form.csrf) {
        return forbidden("CSRF token is invalid");
    }
    let old = match state.db.get_site(id).await {
        Ok(Some(site)) => site,
        Ok(None) => return not_found("site not found"),
        Err(error) => return internal_error(error, "get site"),
    };
    let values = match validate_site_form(&form, false) {
        Ok(values) => values,
        Err(message) => return render_sites(&state, &session, message, String::new()).await,
    };
    if let Err(error) = state
        .db
        .update_site(
            id,
            &values.name,
            &values.router_host,
            values.ssh_port,
            &values.lan_interface,
            &values.ssh_username,
            values.allow_legacy_ssh,
            values.enabled,
        )
        .await
    {
        return render_sites(&state, &session, db_error_message(error), String::new()).await;
    }
    if old.router_host != values.router_host
        || old.ssh_port != values.ssh_port
        || old.ssh_username != values.ssh_username
        || old.allow_legacy_ssh != values.allow_legacy_ssh
    {
        let _ = state.db.clear_site_fingerprint(id).await;
    }
    if !form.ssh_credential.is_empty() {
        let encrypted = match state.secrets.encrypt(&form.ssh_credential) {
            Ok(value) => value,
            Err(error) => return internal_error(error, "encrypt site credential"),
        };
        if let Err(error) = state
            .db
            .upsert_secret(id, "ssh_password", &encrypted.nonce, &encrypted.ciphertext)
            .await
        {
            return internal_error(error, "store site credential");
        }
    }
    let _ = state
        .db
        .insert_audit_event(Some(session.user_id), "site_updated", "site", Some(id), "")
        .await;
    redirect_notice(
        "/sites",
        "拠点を更新しました。接続先情報を変更した場合は再度信頼が必要です。",
    )
}

async fn site_delete(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
    Form(form): Form<CsrfForm>,
) -> Response {
    let Ok((_, session)) = session_or_redirect(&state, &jar).await else {
        return Redirect::to("/login").into_response();
    };
    if !session.role.can_manage() {
        return forbidden("admin role is required");
    }
    if !csrf_valid(&session, &form.csrf) {
        return forbidden("CSRF token is invalid");
    }
    match state.db.delete_site(id).await {
        Ok(true) => {
            let _ = state
                .db
                .insert_audit_event(Some(session.user_id), "site_deleted", "site", Some(id), "")
                .await;
            redirect_notice(
                "/sites",
                "拠点を削除しました。関連デバイスも削除されています。",
            )
        }
        Ok(false) => not_found("site not found"),
        Err(error) => internal_error(error, "delete site"),
    }
}

async fn site_test(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
    Form(form): Form<CsrfForm>,
) -> Response {
    let Ok((token, session)) = session_or_redirect(&state, &jar).await else {
        return Redirect::to("/login").into_response();
    };
    if !session.role.can_manage() {
        return forbidden("admin role is required");
    }
    if !csrf_valid(&session, &form.csrf) {
        return forbidden("CSRF token is invalid");
    }
    let site = match state.db.get_site(id).await {
        Ok(Some(site)) => site,
        Ok(None) => return not_found("site not found"),
        Err(error) => return internal_error(error, "get site"),
    };
    let credential = match state.site_credential(id).await {
        Ok(credential) => credential,
        Err(_error) => {
            return render_sites(
                &state,
                &session,
                "SSH認証情報が未設定です。".to_owned(),
                String::new(),
            )
            .await
        }
    };
    match state.provider.test_connection(site, credential).await {
        Ok(result) => {
            state
                .auth
                .set_pending_fingerprint(&token, id, &result.fingerprint)
                .await;
            let message = format!(
                "RTXへのSSH接続と固定のshow version確認に成功しました。{}",
                if result.detail.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", result.detail)
                }
            );
            render_sites(&state, &session, message, result.fingerprint).await
        }
        Err(error) => {
            error!(site_id = id, error = %error, "Yamaha test connection failed");
            render_sites(
                &state,
                &session,
                format!("接続テスト失敗: {}", safe_error_message(&error)),
                String::new(),
            )
            .await
        }
    }
}

async fn site_trust(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
    Form(form): Form<FingerprintForm>,
) -> Response {
    let Ok((token, session)) = session_or_redirect(&state, &jar).await else {
        return Redirect::to("/login").into_response();
    };
    if !session.role.can_manage() {
        return forbidden("admin role is required");
    }
    if !csrf_valid(&session, &form.csrf) {
        return forbidden("CSRF token is invalid");
    }
    let normalized = match validate_fingerprint(&form.fingerprint) {
        Ok(value) => value,
        Err(error) => {
            return render_sites(&state, &session, error.to_string(), String::new()).await
        }
    };
    let Some(pending) = state.auth.take_pending_fingerprint(&token, id).await else {
        return render_sites(
            &state,
            &session,
            "先に接続テストを実行してください。".to_owned(),
            String::new(),
        )
        .await;
    };
    if pending != normalized {
        return render_sites(
            &state,
            &session,
            "表示された最新Fingerprintと一致しません。再度接続テストしてください。".to_owned(),
            String::new(),
        )
        .await;
    }
    match state.db.trust_site_fingerprint(id, &normalized).await {
        Ok(true) => {
            let _ = state
                .db
                .insert_audit_event(
                    Some(session.user_id),
                    "site_host_key_trusted",
                    "site",
                    Some(id),
                    "",
                )
                .await;
            redirect_notice("/sites", "SSHホスト鍵Fingerprintを信頼しました。")
        }
        Ok(false) => not_found("site not found"),
        Err(error) => internal_error(error, "trust host key"),
    }
}

async fn devices_page(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<NoticeQuery>,
) -> Response {
    let Ok((_, session)) = session_or_redirect(&state, &jar).await else {
        return Redirect::to("/login").into_response();
    };
    render_devices(&state, &session, query.notice.unwrap_or_default()).await
}

async fn device_create(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<DeviceForm>,
) -> Response {
    let Ok((_, session)) = session_or_redirect(&state, &jar).await else {
        return Redirect::to("/login").into_response();
    };
    if !session.role.can_manage() {
        return forbidden("admin role is required");
    }
    if !csrf_valid(&session, &form.csrf) {
        return forbidden("CSRF token is invalid");
    }
    let values = match validate_device_form(&form) {
        Ok(values) => values,
        Err(message) => return render_devices(&state, &session, message).await,
    };
    if state
        .db
        .get_site(values.site_id)
        .await
        .ok()
        .flatten()
        .is_none()
    {
        return render_devices(&state, &session, "拠点が存在しません。".to_owned()).await;
    }
    let device_id = match state
        .db
        .insert_device(
            &values.name,
            values.site_id,
            &values.mac_address,
            values.ip_address.as_deref(),
            &values.description,
        )
        .await
    {
        Ok(id) => id,
        Err(error) => return render_devices(&state, &session, db_error_message(error)).await,
    };
    let _ = state
        .db
        .insert_audit_event(
            Some(session.user_id),
            "device_created",
            "device",
            Some(device_id),
            "",
        )
        .await;
    redirect_notice("/devices", "デバイスを登録しました。")
}

async fn device_update(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
    Form(form): Form<DeviceForm>,
) -> Response {
    let Ok((_, session)) = session_or_redirect(&state, &jar).await else {
        return Redirect::to("/login").into_response();
    };
    if !session.role.can_manage() {
        return forbidden("admin role is required");
    }
    if !csrf_valid(&session, &form.csrf) {
        return forbidden("CSRF token is invalid");
    }
    let values = match validate_device_form(&form) {
        Ok(values) => values,
        Err(message) => return render_devices(&state, &session, message).await,
    };
    if state
        .db
        .get_site(values.site_id)
        .await
        .ok()
        .flatten()
        .is_none()
    {
        return render_devices(&state, &session, "拠点が存在しません。".to_owned()).await;
    }
    match state
        .db
        .update_device(
            id,
            &values.name,
            values.site_id,
            &values.mac_address,
            values.ip_address.as_deref(),
            &values.description,
            values.enabled,
        )
        .await
    {
        Ok(true) => {
            let _ = state
                .db
                .insert_audit_event(
                    Some(session.user_id),
                    "device_updated",
                    "device",
                    Some(id),
                    "",
                )
                .await;
            redirect_notice("/devices", "デバイスを更新しました。")
        }
        Ok(false) => not_found("device not found"),
        Err(error) => render_devices(&state, &session, db_error_message(error)).await,
    }
}

async fn device_delete(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
    Form(form): Form<CsrfForm>,
) -> Response {
    let Ok((_, session)) = session_or_redirect(&state, &jar).await else {
        return Redirect::to("/login").into_response();
    };
    if !session.role.can_manage() {
        return forbidden("admin role is required");
    }
    if !csrf_valid(&session, &form.csrf) {
        return forbidden("CSRF token is invalid");
    }
    match state.db.delete_device(id).await {
        Ok(true) => {
            let _ = state
                .db
                .insert_audit_event(
                    Some(session.user_id),
                    "device_deleted",
                    "device",
                    Some(id),
                    "",
                )
                .await;
            redirect_notice("/devices", "デバイスを削除しました。")
        }
        Ok(false) => not_found("device not found"),
        Err(error) => internal_error(error, "delete device"),
    }
}

async fn device_wake(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
    Form(form): Form<CsrfForm>,
) -> Response {
    let Ok((_, session)) = session_or_redirect(&state, &jar).await else {
        return Redirect::to("/login").into_response();
    };
    if !csrf_valid(&session, &form.csrf) {
        return forbidden("CSRF token is invalid");
    }
    let device = match state.db.get_device(id).await {
        Ok(Some(device)) => device,
        Ok(None) => return not_found("device not found"),
        Err(error) => return internal_error(error, "get device"),
    };
    if !device.enabled {
        return render_devices(&state, &session, "デバイスが無効です。".to_owned()).await;
    }
    let site = match state.db.get_site(device.site_id).await {
        Ok(Some(site)) => site,
        Ok(None) => return not_found("site not found"),
        Err(error) => return internal_error(error, "get site"),
    };
    if !site.enabled {
        return render_devices(&state, &session, "拠点が無効です。".to_owned()).await;
    }
    let credential = match state.site_credential(site.id).await {
        Ok(credential) => credential,
        Err(_error) => {
            let _ = state
                .db
                .insert_wake_event(
                    Some(session.user_id),
                    device.id,
                    site.id,
                    "failed",
                    "SSH認証情報が未設定です。",
                )
                .await;
            return render_devices(&state, &session, "SSH認証情報が未設定です。".to_owned()).await;
        }
    };
    match state
        .provider
        .wake(site.clone(), device.clone(), credential)
        .await
    {
        Ok(result) => {
            let _ = state
                .db
                .insert_wake_event(
                    Some(session.user_id),
                    device.id,
                    site.id,
                    "success",
                    &result.detail,
                )
                .await;
            let _ = state
                .db
                .insert_audit_event(Some(session.user_id), "device_wake", "device", Some(id), "")
                .await;
            render_devices(&state, &session, format!("WOL送信成功: {}", result.detail)).await
        }
        Err(error) => {
            error!(device_id = id, site_id = site.id, error = %error, "WOL failed");
            let message = safe_error_message(&error);
            let _ = state
                .db
                .insert_wake_event(
                    Some(session.user_id),
                    device.id,
                    site.id,
                    "failed",
                    &message,
                )
                .await;
            let _ = state
                .db
                .insert_audit_event(
                    Some(session.user_id),
                    "device_wake_failed",
                    "device",
                    Some(id),
                    "",
                )
                .await;
            render_devices(&state, &session, format!("WOL送信失敗: {message}")).await
        }
    }
}

async fn users_page(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<NoticeQuery>,
) -> Response {
    let Ok((_, session)) = session_or_redirect(&state, &jar).await else {
        return Redirect::to("/login").into_response();
    };
    if !session.role.can_manage() {
        return forbidden("admin role is required");
    }
    render_users(&state, &session, query.notice.unwrap_or_default()).await
}

async fn user_create(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<UserForm>,
) -> Response {
    let Ok((_, session)) = session_or_redirect(&state, &jar).await else {
        return Redirect::to("/login").into_response();
    };
    if !session.role.can_manage() {
        return forbidden("admin role is required");
    }
    if !csrf_valid(&session, &form.csrf) {
        return forbidden("CSRF token is invalid");
    }
    let username = match validate_username(&form.username) {
        Ok(value) => value,
        Err(message) => return render_users(&state, &session, message).await,
    };
    let role = match Role::parse(&form.role) {
        Some(role) => role,
        None => return render_users(&state, &session, "権限が不正です。".to_owned()).await,
    };
    let password_hash = match hash_password(&form.password) {
        Ok(value) => value,
        Err(_) => {
            return render_users(
                &state,
                &session,
                "パスワードは12文字以上で指定してください。".to_owned(),
            )
            .await
        }
    };
    let user_id = match state
        .db
        .insert_user(&username, &password_hash, role.as_str())
        .await
    {
        Ok(id) => id,
        Err(error) => return render_users(&state, &session, db_error_message(error)).await,
    };
    let _ = state
        .db
        .insert_audit_event(
            Some(session.user_id),
            "user_created",
            "user",
            Some(user_id),
            "",
        )
        .await;
    redirect_notice("/users", "ユーザーを作成しました。")
}

async fn user_reset_password(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<ResetPasswordForm>,
) -> Response {
    let Ok((_, session)) = session_or_redirect(&state, &jar).await else {
        return Redirect::to("/login").into_response();
    };
    if !session.role.can_manage() {
        return forbidden("admin role is required");
    }
    if !csrf_valid(&session, &form.csrf) {
        return forbidden("CSRF token is invalid");
    }
    let username = match validate_username(&form.username) {
        Ok(value) => value,
        Err(message) => return render_users(&state, &session, message).await,
    };
    let password_hash = match hash_password(&form.password) {
        Ok(value) => value,
        Err(_) => {
            return render_users(
                &state,
                &session,
                "パスワードは12文字以上で指定してください。".to_owned(),
            )
            .await
        }
    };
    match state.db.update_password(&username, &password_hash).await {
        Ok(true) => {
            let _ = state
                .db
                .insert_audit_event(
                    Some(session.user_id),
                    "user_password_reset",
                    "user",
                    None,
                    "",
                )
                .await;
            redirect_notice("/users", "パスワードを更新しました。")
        }
        Ok(false) => render_users(&state, &session, "ユーザーが存在しません。".to_owned()).await,
        Err(error) => render_users(&state, &session, db_error_message(error)).await,
    }
}

async fn history_page(State(state): State<AppState>, jar: CookieJar) -> Response {
    let Ok((_, session)) = session_or_redirect(&state, &jar).await else {
        return Redirect::to("/login").into_response();
    };
    let wake_events = match state.db.list_wake_events(200).await {
        Ok(events) => events.into_iter().map(wake_view).collect(),
        Err(error) => return internal_error(error, "list wake events"),
    };
    let audit_events = if session.role.can_manage() {
        match state.db.list_audit_events(200).await {
            Ok(events) => events.into_iter().map(audit_view).collect(),
            Err(error) => return internal_error(error, "list audit events"),
        }
    } else {
        Vec::new()
    };
    let title = page_title(&state).await;
    render(HistoryTemplate {
        title,
        username: session.username,
        role: session.role.as_str().to_owned(),
        wake_events,
        audit_events,
    })
}

async fn settings_page(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<NoticeQuery>,
) -> Response {
    let Ok((_, session)) = session_or_redirect(&state, &jar).await else {
        return Redirect::to("/login").into_response();
    };
    if !session.role.can_manage() {
        return forbidden("admin role is required");
    }
    let cookie_secure = state.cookie_secure().await;
    render(SettingsTemplate {
        title: page_title(&state).await,
        username: session.username,
        role: session.role.as_str().to_owned(),
        csrf: session.csrf_token,
        message: query.notice.unwrap_or_default(),
        cookie_secure,
    })
}

async fn settings_save(
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<SettingsForm>,
) -> Response {
    let Ok((_, session)) = session_or_redirect(&state, &jar).await else {
        return Redirect::to("/login").into_response();
    };
    if !session.role.can_manage() {
        return forbidden("admin role is required");
    }
    if !csrf_valid(&session, &form.csrf) {
        return forbidden("CSRF token is invalid");
    }
    let title = match validate_title(&form.site_title) {
        Ok(value) => value,
        Err(message) => {
            return render(SettingsTemplate {
                title: page_title(&state).await,
                username: session.username,
                role: session.role.as_str().to_owned(),
                csrf: session.csrf_token,
                message,
                cookie_secure: state.cookie_secure().await,
            })
        }
    };
    if let Err(error) = state.db.set_setting("site_title", &title).await {
        return internal_error(error, "save site title");
    }
    if let Err(error) = state
        .db
        .set_setting(
            "cookie_secure",
            if form.cookie_secure.is_some() {
                "true"
            } else {
                "false"
            },
        )
        .await
    {
        return internal_error(error, "save cookie setting");
    }
    let _ = state
        .db
        .insert_audit_event(
            Some(session.user_id),
            "settings_updated",
            "settings",
            None,
            "",
        )
        .await;
    redirect_notice("/settings", "設定を保存しました。")
}

async fn style_css() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("static/style.css"),
    )
}

async fn app_js() -> impl IntoResponse {
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/javascript; charset=utf-8",
        )],
        include_str!("static/app.js"),
    )
}

async fn api_health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: "WakeBridge",
        mode: if state.config.service_mode {
            "windows-service"
        } else {
            "foreground"
        },
        started_at: state.started_at,
    })
}

async fn api_service_status(State(state): State<AppState>, jar: CookieJar) -> Response {
    let Ok((_, session)) = session_or_redirect(&state, &jar).await else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let _ = session;
    Json(HealthResponse {
        status: "ok",
        service: "WakeBridge",
        mode: if state.config.service_mode {
            "windows-service"
        } else {
            "foreground"
        },
        started_at: state.started_at,
    })
    .into_response()
}

async fn api_device_wake(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Response {
    let Ok((_, session)) = session_or_redirect(&state, &jar).await else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let Some(csrf) = headers
        .get("x-csrf-token")
        .and_then(|value| value.to_str().ok())
    else {
        return forbidden("CSRF token is required");
    };
    if !csrf_valid(&session, csrf) {
        return forbidden("CSRF token is invalid");
    }
    let device = match state.db.get_device(id).await {
        Ok(Some(device)) => device,
        Ok(None) => return not_found("device not found"),
        Err(error) => return internal_error(error, "get device"),
    };
    let site = match state.db.get_site(device.site_id).await {
        Ok(Some(site)) => site,
        Ok(None) => return not_found("site not found"),
        Err(error) => return internal_error(error, "get site"),
    };
    if !device.enabled || !site.enabled {
        return (
            StatusCode::PRECONDITION_FAILED,
            "デバイスまたは拠点が無効です。",
        )
            .into_response();
    }
    let credential = match state.site_credential(site.id).await {
        Ok(value) => value,
        Err(_) => {
            return (StatusCode::PRECONDITION_FAILED, "SSH認証情報が未設定です。").into_response()
        }
    };
    match state
        .provider
        .wake(site.clone(), device.clone(), credential)
        .await
    {
        Ok(result) => {
            let _ = state
                .db
                .insert_wake_event(
                    Some(session.user_id),
                    device.id,
                    site.id,
                    "success",
                    &result.detail,
                )
                .await;
            let _ = state
                .db
                .insert_audit_event(Some(session.user_id), "device_wake", "device", Some(id), "")
                .await;
            Json(serde_json::json!({"status":"success","message":result.detail})).into_response()
        }
        Err(error) => {
            let message = safe_error_message(&error);
            let _ = state
                .db
                .insert_wake_event(
                    Some(session.user_id),
                    device.id,
                    site.id,
                    "failed",
                    &message,
                )
                .await;
            let _ = state
                .db
                .insert_audit_event(
                    Some(session.user_id),
                    "device_wake_failed",
                    "device",
                    Some(id),
                    "",
                )
                .await;
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"status":"failed","message":message})),
            )
                .into_response()
        }
    }
}

async fn render_sites(
    state: &AppState,
    session: &Session,
    message: String,
    fingerprint: String,
) -> Response {
    let records = match state.db.list_sites().await {
        Ok(records) => records,
        Err(error) => return internal_error(error, "list sites"),
    };
    let mut sites = Vec::with_capacity(records.len());
    for record in records {
        let credential_present = match state.db.get_secret(record.id).await {
            Ok(value) => value.is_some(),
            Err(error) => return internal_error(error, "check site credential"),
        };
        sites.push(site_view(record, credential_present));
    }
    render(SitesTemplate {
        title: page_title(state).await,
        username: session.username.clone(),
        role: session.role.as_str().to_owned(),
        csrf: session.csrf_token.clone(),
        message,
        fingerprint,
        sites,
    })
}

async fn render_devices(state: &AppState, session: &Session, message: String) -> Response {
    let records = match state.db.list_devices().await {
        Ok(records) => records,
        Err(error) => return internal_error(error, "list devices"),
    };
    let devices = records.into_iter().map(device_view).collect();
    let site_options = match state.db.list_sites().await {
        Ok(sites) => sites
            .into_iter()
            .map(|site| SiteOption {
                id: site.id,
                name: site.name,
            })
            .collect(),
        Err(error) => return internal_error(error, "list site options"),
    };
    render(DevicesTemplate {
        title: page_title(state).await,
        username: session.username.clone(),
        role: session.role.as_str().to_owned(),
        csrf: session.csrf_token.clone(),
        message,
        devices,
        site_options,
    })
}

async fn render_users(state: &AppState, session: &Session, message: String) -> Response {
    let users = match state.db.list_users().await {
        Ok(users) => users.into_iter().map(user_view).collect(),
        Err(error) => return internal_error(error, "list users"),
    };
    render(UsersTemplate {
        title: page_title(state).await,
        username: session.username.clone(),
        role: session.role.as_str().to_owned(),
        csrf: session.csrf_token.clone(),
        message,
        users,
    })
}

async fn render_account_password(state: &AppState, session: &Session, message: String) -> Response {
    render(AccountPasswordTemplate {
        title: page_title(state).await,
        username: session.username.clone(),
        role: session.role.as_str().to_owned(),
        csrf: session.csrf_token.clone(),
        message,
    })
}

fn site_view(record: SiteRecord, credential_present: bool) -> SiteView {
    let trusted = record.ssh_host_key_fingerprint.is_some();
    SiteView {
        id: record.id,
        name: record.name,
        router_host: record.router_host,
        ssh_port: record.ssh_port,
        lan_interface: record.lan_interface,
        ssh_username: record.ssh_username,
        fingerprint: record.ssh_host_key_fingerprint.unwrap_or_default(),
        trusted,
        credential_present,
        allow_legacy_ssh: record.allow_legacy_ssh,
        enabled: record.enabled,
    }
}

fn device_view(record: DeviceRecord) -> DeviceView {
    DeviceView {
        id: record.id,
        name: record.name,
        site_id: record.site_id,
        site_name: record.site_name,
        mac_address: record.mac_address,
        ip_address: record.ip_address.unwrap_or_default(),
        description: record.description,
        enabled: record.enabled,
    }
}

fn user_view(record: UserRecord) -> UserView {
    UserView {
        username: record.username,
        role: record.role,
        enabled: record.enabled,
    }
}

fn wake_view(record: WakeEventRecord) -> WakeView {
    let status_label = wake_status_label(&record.status);
    WakeView {
        username: record.username.unwrap_or_else(|| "システム".to_owned()),
        device_name: record.device_name,
        site_name: record.site_name,
        status: record.status,
        status_label,
        message: record.message,
        occurred_at: record.occurred_at,
    }
}

fn audit_view(record: AuditEventRecord) -> AuditView {
    AuditView {
        username: record.username.unwrap_or_else(|| "システム".to_owned()),
        action: audit_action_label(&record.action),
        target_type: audit_target_label(&record.target_type),
        target_id: record
            .target_id
            .map(|value| value.to_string())
            .unwrap_or_default(),
        details: record.details,
        occurred_at: record.occurred_at,
    }
}

fn wake_status_label(status: &str) -> String {
    match status {
        "success" => "成功".to_owned(),
        "failed" => "失敗".to_owned(),
        other => other.to_owned(),
    }
}

fn audit_action_label(action: &str) -> String {
    match action {
        "login_failed" => "ログイン失敗",
        "login_success" => "ログイン成功",
        "site_created" => "拠点登録",
        "site_updated" => "拠点更新",
        "site_deleted" => "拠点削除",
        "site_host_key_trusted" => "SSHホスト鍵を信頼",
        "device_created" => "デバイス登録",
        "device_updated" => "デバイス更新",
        "device_deleted" => "デバイス削除",
        "device_wake" => "WOL実行",
        "device_wake_failed" => "WOL失敗",
        "user_created" => "ユーザー登録",
        "user_password_reset" => "パスワードリセット",
        "user_password_changed" => "パスワード変更",
        "settings_updated" => "設定更新",
        other => other,
    }
    .to_owned()
}

fn audit_target_label(target_type: &str) -> String {
    match target_type {
        "auth" => "認証",
        "site" => "拠点",
        "device" => "デバイス",
        "user" => "ユーザー",
        "settings" => "設定",
        other => other,
    }
    .to_owned()
}

async fn page_title(state: &AppState) -> String {
    state
        .db
        .get_setting("site_title")
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| "WakeBridge".to_owned())
}

fn session_cookie(token: &str, secure: bool) -> Cookie<'static> {
    Cookie::build((SESSION_COOKIE, token.to_owned()))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(secure)
        .build()
}

async fn session_or_redirect(
    state: &AppState,
    jar: &CookieJar,
) -> std::result::Result<(String, Session), Response> {
    let Some(token) = jar
        .get(SESSION_COOKIE)
        .map(|cookie| cookie.value().to_owned())
    else {
        return Err(Redirect::to("/login").into_response());
    };
    let Some(session) = state.auth.get_session(&token).await else {
        return Err(Redirect::to("/login").into_response());
    };
    Ok((token, session))
}

fn csrf_valid(session: &Session, provided: &str) -> bool {
    session.csrf_token == provided
}

fn render<T: Template>(template: T) -> Response {
    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(error) => internal_error(error, "render page"),
    }
}

fn redirect_notice(path: &str, message: &str) -> Response {
    let encoded = urlencoding::encode(message);
    Redirect::to(&format!("{path}?notice={encoded}")).into_response()
}

fn internal_error(error: impl std::fmt::Display, context: &str) -> Response {
    error!(context, error = %error, "WakeBridge request failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "内部エラーが発生しました。",
    )
        .into_response()
}

fn db_error_message(error: anyhow::Error) -> String {
    error!(error = %error, "database operation failed");
    "保存に失敗しました。入力値とログを確認してください。".to_owned()
}

fn safe_error_message(error: &anyhow::Error) -> String {
    error
        .to_string()
        .replace(['\r', '\n'], " ")
        .chars()
        .take(240)
        .collect()
}

fn forbidden(message: &str) -> Response {
    let message = match message {
        "admin role is required" => "管理者権限が必要です。",
        "CSRF token is invalid" => "CSRFトークンが不正です。",
        "CSRF token is required" => "CSRFトークンが必要です。",
        other => other,
    };
    (StatusCode::FORBIDDEN, message.to_owned()).into_response()
}

fn not_found(message: &str) -> Response {
    let message = match message {
        "site not found" => "拠点が見つかりません。",
        "device not found" => "デバイスが見つかりません。",
        other => other,
    };
    (StatusCode::NOT_FOUND, message.to_owned()).into_response()
}

struct SiteValues {
    name: String,
    router_host: String,
    ssh_port: u16,
    lan_interface: String,
    ssh_username: String,
    allow_legacy_ssh: bool,
    enabled: bool,
}

fn validate_site_form(form: &SiteForm, creating: bool) -> std::result::Result<SiteValues, String> {
    if form.provider != "yamaha_rtx" {
        return Err("Yamaha RTX以外のProviderには対応していません。".to_owned());
    }
    let name = validate_title(&form.name)?;
    let router_host = validate_router_host(&form.router_host)?;
    let ssh_port = form
        .ssh_port
        .parse::<u16>()
        .ok()
        .filter(|port| *port > 0)
        .ok_or_else(|| "SSH Portは1から65535の整数で指定してください。".to_owned())?;
    let lan_interface = validate_interface(&form.lan_interface)?;
    let ssh_username = validate_ssh_username(&form.ssh_username)?;
    if creating && form.ssh_credential.is_empty() {
        return Err("新規拠点にはSSH認証情報が必要です。".to_owned());
    }
    Ok(SiteValues {
        name,
        router_host,
        ssh_port,
        lan_interface,
        ssh_username,
        allow_legacy_ssh: form.allow_legacy_ssh.is_some(),
        enabled: form.enabled.is_some(),
    })
}

struct DeviceValues {
    name: String,
    site_id: i64,
    mac_address: String,
    ip_address: Option<String>,
    description: String,
    enabled: bool,
}

fn validate_device_form(form: &DeviceForm) -> std::result::Result<DeviceValues, String> {
    let name = validate_title(&form.name)?;
    let site_id = form
        .site_id
        .parse::<i64>()
        .ok()
        .filter(|id| *id > 0)
        .ok_or_else(|| "拠点を選択してください。".to_owned())?;
    let mac_address = normalize_mac(&form.mac_address).map_err(|error| error.to_string())?;
    let ip_address = if form.ip_address.trim().is_empty() {
        None
    } else {
        Some(normalize_ip(&form.ip_address).map_err(|error| error.to_string())?)
    };
    if form.description.chars().count() > 500 {
        return Err("説明は500文字以内です。".to_owned());
    }
    Ok(DeviceValues {
        name,
        site_id,
        mac_address,
        ip_address,
        description: form.description.trim().to_owned(),
        enabled: form.enabled.is_some(),
    })
}

fn validate_title(value: &str) -> std::result::Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 64 {
        return Err("名称は1から64文字で指定してください。".to_owned());
    }
    if value.chars().any(|character| character.is_control()) {
        return Err("名称に制御文字は使用できません。".to_owned());
    }
    Ok(value.to_owned())
}

fn validate_router_host(value: &str) -> std::result::Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 253 {
        return Err("ルーターホストを指定してください。".to_owned());
    }
    if value.chars().any(|character| {
        character.is_control()
            || character.is_whitespace()
            || matches!(character, ';' | '|' | '&' | '\u{60}' | '$' | '\u{0}')
    }) {
        return Err("ルーターホストに不正な文字があります。".to_owned());
    }
    Ok(value.to_owned())
}

fn validate_interface(value: &str) -> std::result::Result<String, String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 32
        || value.chars().any(|character| {
            !character.is_ascii_alphanumeric() && !matches!(character, '.' | '_' | '-' | '/')
        })
    {
        return Err("LANインターフェースに不正な文字があります。".to_owned());
    }
    Ok(value.to_owned())
}

fn validate_ssh_username(value: &str) -> std::result::Result<String, String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 64
        || value.chars().any(|character| {
            character.is_control() || character.is_whitespace() || character == ':'
        })
    {
        return Err("SSHユーザー名に不正な文字があります。".to_owned());
    }
    Ok(value.to_owned())
}

fn validate_username(value: &str) -> std::result::Result<String, String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 64
        || value.chars().any(|character| {
            !character.is_ascii_alphanumeric() && !matches!(character, '.' | '_' | '-')
        })
    {
        return Err(
            "ユーザー名は英数字、ドット、アンダースコア、ハイフンで指定してください。".to_owned(),
        );
    }
    Ok(value.to_owned())
}
