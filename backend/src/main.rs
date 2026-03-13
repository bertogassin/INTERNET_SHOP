use axum::{
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
    Json, Router,
};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use rand::{distributions::Alphanumeric, Rng};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};

const ACCESS_TTL_SECONDS: usize = 15 * 60;
const REFRESH_TTL_MS: i64 = 7 * 24 * 60 * 60 * 1000;
const LOGIN_WINDOW_MS: i64 = 15 * 60 * 1000;
const LOGIN_MAX_ATTEMPTS: usize = 8;

#[derive(Clone)]
struct AppState {
    db: Arc<Mutex<Connection>>,
    jwt_secret: String,
    login_attempts: Arc<Mutex<HashMap<String, Vec<i64>>>>,
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

impl From<rusqlite::Error> for ApiError {
    fn from(err: rusqlite::Error) -> Self {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("database error: {err}"),
        )
    }
}

#[derive(Serialize)]
struct HealthResponse<'a> {
    status: &'a str,
    service: &'a str,
    storage: &'a str,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Role {
    Owner,
    Staff,
    Viewer,
}

impl Role {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "owner" => Some(Role::Owner),
            "staff" => Some(Role::Staff),
            "viewer" => Some(Role::Viewer),
            _ => None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: i64,
    email: String,
    role: Role,
    sid: i64,
    typ: String,
    iat: usize,
    exp: usize,
}

#[derive(Debug, Serialize, Clone)]
struct AuthUser {
    id: i64,
    email: String,
    role: Role,
}

#[derive(Debug, Clone)]
struct AuthContext {
    user: AuthUser,
    session_id: i64,
}

#[derive(Debug, Deserialize)]
struct LoginRequest {
    email: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct RefreshRequest {
    refresh_token: String,
}

#[derive(Debug, Deserialize)]
struct LogoutRequest {
    refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PasswordChangeRequest {
    current_password: String,
    new_password: String,
}

#[derive(Debug, Serialize)]
struct TokensResponse {
    access_token: String,
    refresh_token: String,
}

#[derive(Debug, Serialize)]
struct LoginResponse {
    access_token: String,
    refresh_token: String,
    user: AuthUser,
}

#[derive(Serialize)]
struct Shop {
    name: String,
    tagline: String,
    accent: String,
}

#[derive(Deserialize)]
struct ShopPatch {
    name: Option<String>,
    tagline: Option<String>,
    accent: Option<String>,
}

#[derive(Serialize)]
struct Product {
    id: i64,
    title: String,
    category: String,
    price: f64,
    popularity: i64,
    desc: String,
}

#[derive(Deserialize)]
struct ProductCreate {
    title: String,
    category: Option<String>,
    price: f64,
    desc: Option<String>,
}

#[derive(Deserialize)]
struct CheckoutItemInput {
    id: i64,
    qty: i64,
}

#[derive(Deserialize)]
struct CheckoutRequest {
    items: Vec<CheckoutItemInput>,
    email: Option<String>,
}

#[derive(Serialize)]
struct CheckoutItem {
    product_id: i64,
    title: String,
    price: f64,
    qty: i64,
}

#[derive(Serialize)]
struct OrderResponse {
    id: i64,
    email: String,
    total: f64,
    created_at: i64,
    items: Vec<CheckoutItem>,
}

#[derive(Serialize)]
struct SaleListing {
    ask: f64,
    gmv: f64,
    traffic: i64,
    note: String,
    status: String,
    updated_at: i64,
}

#[derive(Deserialize)]
struct SaleListingUpsert {
    ask: f64,
    gmv: Option<f64>,
    traffic: Option<i64>,
    note: Option<String>,
}

#[derive(Serialize)]
struct SaleOffer {
    id: i64,
    buyer: String,
    offer: f64,
    status: String,
    created_at: i64,
}

#[derive(Deserialize)]
struct SaleOfferCreate {
    buyer: String,
    offer: f64,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn now_s() -> usize {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as usize)
        .unwrap_or(0)
}

fn hash_password(raw: &str) -> String {
    let mut h = Sha256::new();
    h.update(raw.as_bytes());
    format!("{:x}", h.finalize())
}

fn random_token(len: usize) -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

fn extract_bearer_token(headers: &HeaderMap) -> Option<String> {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .trim();
    if auth.len() < 8 || !auth.to_ascii_lowercase().starts_with("bearer ") {
        return None;
    }
    Some(auth[7..].trim().to_string())
}

fn request_fingerprint(headers: &HeaderMap, email: &str) -> String {
    let ip = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .or_else(|| headers.get("x-real-ip").and_then(|v| v.to_str().ok()))
        .unwrap_or("local");
    format!("{}|{}", email.to_lowercase(), ip)
}

async fn check_login_rate_limit(state: &AppState, key: &str) -> Result<(), ApiError> {
    let mut map = state.login_attempts.lock().await;
    let now = now_ms();
    let slot = map.entry(key.to_string()).or_default();
    slot.retain(|ts| now - *ts <= LOGIN_WINDOW_MS);
    if slot.len() >= LOGIN_MAX_ATTEMPTS {
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "too many login attempts, retry later",
        ));
    }
    Ok(())
}

async fn mark_login_failure(state: &AppState, key: &str) {
    let mut map = state.login_attempts.lock().await;
    let now = now_ms();
    map.entry(key.to_string()).or_default().push(now);
}

async fn clear_login_failures(state: &AppState, key: &str) {
    let mut map = state.login_attempts.lock().await;
    map.remove(key);
}

fn issue_access_token(
    state: &AppState,
    user_id: i64,
    email: &str,
    role: Role,
    session_id: i64,
) -> Result<String, ApiError> {
    let iat = now_s();
    let claims = Claims {
        sub: user_id,
        email: email.to_string(),
        role,
        sid: session_id,
        typ: "access".to_string(),
        iat,
        exp: iat + ACCESS_TTL_SECONDS,
    };
    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(state.jwt_secret.as_bytes()),
    )
    .map_err(|_| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "failed to issue token"))
}

async fn auth_context_from_headers(
    headers: &HeaderMap,
    state: &AppState,
) -> Result<AuthContext, ApiError> {
    let token = extract_bearer_token(headers)
        .ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "bearer token required"))?;

    let decoded = decode::<Claims>(
        &token,
        &DecodingKey::from_secret(state.jwt_secret.as_bytes()),
        &Validation::new(Algorithm::HS256),
    )
    .map_err(|_| ApiError::new(StatusCode::UNAUTHORIZED, "invalid token"))?;
    let claims = decoded.claims;
    if claims.typ != "access" {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid token type",
        ));
    }

    let conn = state.db.lock().await;
    let user_row: Option<(i64, String, String, String)> = conn
        .query_row(
            "SELECT id, email, role, status FROM users WHERE id = ?",
            [claims.sub],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let (id, email, role_raw, status) =
        user_row.ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "user no longer exists"))?;
    if status != "active" {
        return Err(ApiError::new(StatusCode::FORBIDDEN, "user is not active"));
    }
    let role = Role::parse(&role_raw)
        .ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "invalid user role"))?;

    let sess: Option<(String, i64)> = conn
        .query_row(
            "SELECT status, expires_at FROM auth_sessions WHERE id = ? AND user_id = ?",
            params![claims.sid, id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let (sess_status, sess_exp) =
        sess.ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "session not found"))?;
    if sess_status != "active" || sess_exp <= now_ms() {
        return Err(ApiError::new(StatusCode::UNAUTHORIZED, "session expired"));
    }
    if role != claims.role || email.to_lowercase() != claims.email.to_lowercase() {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "token no longer valid",
        ));
    }

    Ok(AuthContext {
        user: AuthUser { id, email, role },
        session_id: claims.sid,
    })
}

fn enforce_role(user: &AuthUser, allowed: &[Role]) -> Result<(), ApiError> {
    if allowed.contains(&user.role) {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "insufficient role permissions",
        ))
    }
}

fn init_schema(
    conn: &Connection,
    seed_demo_users: bool,
    bootstrap_owner: Option<(&str, &str)>,
) -> Result<(), ApiError> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS shop_settings (
          id INTEGER PRIMARY KEY CHECK (id = 1),
          name TEXT NOT NULL,
          tagline TEXT NOT NULL DEFAULT '',
          accent TEXT NOT NULL DEFAULT '#3dd9b3'
        );
        CREATE TABLE IF NOT EXISTS products (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          title TEXT NOT NULL,
          category TEXT NOT NULL,
          price REAL NOT NULL,
          popularity INTEGER NOT NULL DEFAULT 50,
          desc TEXT NOT NULL DEFAULT '',
          created_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS orders (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          email TEXT NOT NULL DEFAULT '',
          total REAL NOT NULL,
          created_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS order_items (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          order_id INTEGER NOT NULL REFERENCES orders(id) ON DELETE CASCADE,
          product_id INTEGER NOT NULL,
          title TEXT NOT NULL,
          price REAL NOT NULL,
          qty INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS sale_listing (
          id INTEGER PRIMARY KEY CHECK (id = 1),
          ask REAL NOT NULL,
          gmv REAL NOT NULL DEFAULT 0,
          traffic INTEGER NOT NULL DEFAULT 0,
          note TEXT NOT NULL DEFAULT '',
          status TEXT NOT NULL DEFAULT 'active',
          updated_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS sale_offers (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          buyer TEXT NOT NULL,
          offer REAL NOT NULL,
          status TEXT NOT NULL DEFAULT 'pending',
          created_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS users (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          email TEXT NOT NULL UNIQUE,
          password_hash TEXT NOT NULL,
          role TEXT NOT NULL,
          status TEXT NOT NULL DEFAULT 'active',
          created_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS auth_sessions (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
          refresh_hash TEXT NOT NULL UNIQUE,
          status TEXT NOT NULL DEFAULT 'active',
          created_at INTEGER NOT NULL,
          expires_at INTEGER NOT NULL,
          last_seen_at INTEGER NOT NULL
        );
        "#,
    )?;

    let count: i64 = conn.query_row("SELECT COUNT(*) FROM products", [], |r| r.get(0))?;
    if count == 0 {
        let ts = now_ms();
        conn.execute(
            "INSERT INTO shop_settings (id, name, tagline, accent) VALUES (1, ?, ?, ?)",
            params![
                "Street Pulse Shop",
                "Trend-first storefront with flexible style and sell-shop module.",
                "#3dd9b3"
            ],
        )?;
        let seed_products = vec![
            (
                "Neo Jacket",
                "fashion",
                129.0_f64,
                92_i64,
                "Urban fit, limited drop.",
            ),
            (
                "Pulse Headset",
                "tech",
                89.0_f64,
                88_i64,
                "Low-latency wireless sound.",
            ),
            (
                "Glow Lamp",
                "home",
                59.0_f64,
                74_i64,
                "Ambient adaptive light mode.",
            ),
            (
                "Street Sneakers",
                "fashion",
                109.0_f64,
                95_i64,
                "Comfort sole for daily wear.",
            ),
            (
                "Creator Mic",
                "tech",
                149.0_f64,
                81_i64,
                "Clean voice capture for streams.",
            ),
            (
                "Smart Shelf",
                "home",
                79.0_f64,
                70_i64,
                "Modular shelf with hidden cable path.",
            ),
        ];
        for p in seed_products {
            conn.execute(
                "INSERT INTO products (title, category, price, popularity, desc, created_at) VALUES (?, ?, ?, ?, ?, ?)",
                params![p.0, p.1, p.2, p.3, p.4, ts],
            )?;
        }
        conn.execute(
            "INSERT INTO sale_offers (buyer, offer, status, created_at) VALUES (?, ?, ?, ?)",
            params![
                "buyer.alpha@shopmail.com",
                42000.0_f64,
                "pending",
                ts - 200_000
            ],
        )?;
        conn.execute(
            "INSERT INTO sale_offers (buyer, offer, status, created_at) VALUES (?, ?, ?, ?)",
            params![
                "studio.delta@shopmail.com",
                45500.0_f64,
                "pending",
                ts - 100_000
            ],
        )?;
    }

    let users_count: i64 = conn.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?;
    if users_count == 0 && seed_demo_users {
        let ts = now_ms();
        conn.execute(
            "INSERT INTO users (email, password_hash, role, status, created_at) VALUES (?, ?, ?, ?, ?)",
            params!["owner@internet.shop", hash_password("Owner123!"), "owner", "active", ts],
        )?;
        conn.execute(
            "INSERT INTO users (email, password_hash, role, status, created_at) VALUES (?, ?, ?, ?, ?)",
            params!["staff@internet.shop", hash_password("Staff123!"), "staff", "active", ts],
        )?;
        conn.execute(
            "INSERT INTO users (email, password_hash, role, status, created_at) VALUES (?, ?, ?, ?, ?)",
            params!["viewer@internet.shop", hash_password("Viewer123!"), "viewer", "active", ts],
        )?;
    }
    if users_count == 0 && !seed_demo_users {
        if let Some((email, password)) = bootstrap_owner {
            let ts = now_ms();
            conn.execute(
                "INSERT INTO users (email, password_hash, role, status, created_at) VALUES (?, ?, ?, ?, ?)",
                params![email.trim().to_lowercase(), hash_password(password), "owner", "active", ts],
            )?;
        } else {
            return Err(ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "users table is empty and demo seeding is disabled",
            ));
        }
    }
    Ok(())
}

async fn health() -> Json<HealthResponse<'static>> {
    Json(HealthResponse {
        status: "ok",
        service: "internet-shop-backend",
        storage: "sqlite",
    })
}

async fn post_auth_login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, ApiError> {
    let email = payload.email.trim().to_lowercase();
    let password = payload.password.trim().to_string();
    if email.is_empty() || password.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "email and password required",
        ));
    }

    let fp = request_fingerprint(&headers, &email);
    check_login_rate_limit(&state, &fp).await?;

    let conn = state.db.lock().await;
    let user_row: Option<(i64, String, String, String, String)> = conn
        .query_row(
            "SELECT id, email, password_hash, role, status FROM users WHERE lower(email)=lower(?)",
            [email.clone()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?;

    let (id, db_email, db_hash, role_raw, status) = match user_row {
        Some(v) => v,
        None => {
            mark_login_failure(&state, &fp).await;
            return Err(ApiError::new(
                StatusCode::UNAUTHORIZED,
                "invalid credentials",
            ));
        }
    };
    if status != "active" || db_hash != hash_password(&password) {
        mark_login_failure(&state, &fp).await;
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid credentials",
        ));
    }
    clear_login_failures(&state, &fp).await;
    let role = Role::parse(&role_raw)
        .ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "invalid role"))?;

    let refresh = random_token(64);
    let refresh_hash = hash_password(&refresh);
    let now = now_ms();
    conn.execute(
        "INSERT INTO auth_sessions (user_id, refresh_hash, status, created_at, expires_at, last_seen_at) VALUES (?, ?, 'active', ?, ?, ?)",
        params![id, refresh_hash, now, now + REFRESH_TTL_MS, now],
    )?;
    let sid = conn.last_insert_rowid();
    let access = issue_access_token(&state, id, &db_email, role, sid)?;

    Ok(Json(LoginResponse {
        access_token: access,
        refresh_token: refresh,
        user: AuthUser {
            id,
            email: db_email,
            role,
        },
    }))
}

async fn post_auth_refresh(
    State(state): State<AppState>,
    Json(payload): Json<RefreshRequest>,
) -> Result<Json<TokensResponse>, ApiError> {
    let token = payload.refresh_token.trim().to_string();
    if token.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "refresh token required",
        ));
    }
    let token_hash = hash_password(&token);
    let conn = state.db.lock().await;
    let row: Option<(i64, i64, i64, String, String, String)> = conn
        .query_row(
            "SELECT s.id, u.id, s.expires_at, s.status, u.email, u.role FROM auth_sessions s JOIN users u ON u.id=s.user_id WHERE s.refresh_hash=?",
            [token_hash],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .optional()?;
    let (sid, uid, exp_at, status, email, role_raw) =
        row.ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "invalid refresh token"))?;
    if status != "active" || exp_at <= now_ms() {
        conn.execute(
            "UPDATE auth_sessions SET status='revoked' WHERE id=?",
            [sid],
        )?;
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "refresh token expired",
        ));
    }
    let role = Role::parse(&role_raw)
        .ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "invalid role"))?;
    let next_refresh = random_token(64);
    let now = now_ms();
    conn.execute(
        "UPDATE auth_sessions SET refresh_hash=?, expires_at=?, last_seen_at=? WHERE id=?",
        params![hash_password(&next_refresh), now + REFRESH_TTL_MS, now, sid],
    )?;
    let access = issue_access_token(&state, uid, &email, role, sid)?;
    Ok(Json(TokensResponse {
        access_token: access,
        refresh_token: next_refresh,
    }))
}

async fn post_auth_logout(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<LogoutRequest>,
) -> Result<Json<Value>, ApiError> {
    let ctx = auth_context_from_headers(&headers, &state).await?;
    let conn = state.db.lock().await;
    conn.execute(
        "UPDATE auth_sessions SET status='revoked', last_seen_at=? WHERE id=?",
        params![now_ms(), ctx.session_id],
    )?;
    if let Some(refresh) = payload.refresh_token {
        let hash = hash_password(refresh.trim());
        conn.execute(
            "UPDATE auth_sessions SET status='revoked', last_seen_at=? WHERE refresh_hash=?",
            params![now_ms(), hash],
        )?;
    }
    Ok(Json(json!({"ok": true})))
}

async fn post_auth_password_change(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<PasswordChangeRequest>,
) -> Result<Json<Value>, ApiError> {
    let ctx = auth_context_from_headers(&headers, &state).await?;
    let current = payload.current_password.trim().to_string();
    let next = payload.new_password.trim().to_string();
    if current.is_empty() || next.len() < 8 {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "current password required and new password must be at least 8 chars",
        ));
    }
    let conn = state.db.lock().await;
    let old_hash: String = conn.query_row(
        "SELECT password_hash FROM users WHERE id=?",
        [ctx.user.id],
        |r| r.get(0),
    )?;
    if old_hash != hash_password(&current) {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "current password is incorrect",
        ));
    }
    conn.execute(
        "UPDATE users SET password_hash=? WHERE id=?",
        params![hash_password(&next), ctx.user.id],
    )?;
    conn.execute(
        "UPDATE auth_sessions SET status='revoked', last_seen_at=? WHERE user_id=?",
        params![now_ms(), ctx.user.id],
    )?;
    Ok(Json(json!({"ok": true, "reauth_required": true})))
}

async fn get_auth_me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let ctx = auth_context_from_headers(&headers, &state).await?;
    Ok(Json(json!({ "user": ctx.user })))
}

async fn get_shop(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let conn = state.db.lock().await;
    let shop_opt: Option<Shop> = conn
        .query_row(
            "SELECT name, tagline, accent FROM shop_settings WHERE id = 1",
            [],
            |r| {
                Ok(Shop {
                    name: r.get(0)?,
                    tagline: r.get(1)?,
                    accent: r.get(2)?,
                })
            },
        )
        .optional()?;
    let shop = if let Some(s) = shop_opt {
        s
    } else {
        conn.execute(
            "INSERT INTO shop_settings (id, name, tagline, accent) VALUES (1, ?, ?, ?)",
            params!["Street Pulse Shop", "", "#3dd9b3"],
        )?;
        Shop {
            name: "Street Pulse Shop".to_string(),
            tagline: "".to_string(),
            accent: "#3dd9b3".to_string(),
        }
    };
    Ok(Json(json!({ "shop": shop })))
}

async fn patch_shop_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ShopPatch>,
) -> Result<Json<Value>, ApiError> {
    let ctx = auth_context_from_headers(&headers, &state).await?;
    enforce_role(&ctx.user, &[Role::Owner, Role::Staff])?;
    let conn = state.db.lock().await;
    let current: Option<Shop> = conn
        .query_row(
            "SELECT name, tagline, accent FROM shop_settings WHERE id = 1",
            [],
            |r| {
                Ok(Shop {
                    name: r.get(0)?,
                    tagline: r.get(1)?,
                    accent: r.get(2)?,
                })
            },
        )
        .optional()?;
    let next = Shop {
        name: payload
            .name
            .and_then(|v| {
                let t = v.trim().to_string();
                if t.is_empty() {
                    None
                } else {
                    Some(t)
                }
            })
            .or_else(|| current.as_ref().map(|c| c.name.clone()))
            .unwrap_or_else(|| "Internet Shop".to_string()),
        tagline: payload
            .tagline
            .map(|v| v.trim().to_string())
            .or_else(|| current.as_ref().map(|c| c.tagline.clone()))
            .unwrap_or_default(),
        accent: payload
            .accent
            .and_then(|v| {
                let t = v.trim().to_string();
                if t.is_empty() {
                    None
                } else {
                    Some(t)
                }
            })
            .or_else(|| current.as_ref().map(|c| c.accent.clone()))
            .unwrap_or_else(|| "#3dd9b3".to_string()),
    };
    conn.execute(
        "INSERT INTO shop_settings (id, name, tagline, accent) VALUES (1, ?, ?, ?) ON CONFLICT(id) DO UPDATE SET name=excluded.name, tagline=excluded.tagline, accent=excluded.accent",
        params![next.name, next.tagline, next.accent],
    )?;
    Ok(Json(json!({ "ok": true, "shop": next })))
}

async fn get_products(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let conn = state.db.lock().await;
    let mut stmt = conn.prepare(
        "SELECT id, title, category, price, popularity, desc FROM products ORDER BY id DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Product {
            id: r.get(0)?,
            title: r.get(1)?,
            category: r.get(2)?,
            price: r.get(3)?,
            popularity: r.get(4)?,
            desc: r.get(5)?,
        })
    })?;
    let mut products = Vec::new();
    for row in rows {
        products.push(row?);
    }
    Ok(Json(json!({ "products": products })))
}

async fn post_products(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ProductCreate>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let ctx = auth_context_from_headers(&headers, &state).await?;
    enforce_role(&ctx.user, &[Role::Owner, Role::Staff])?;
    let title = payload.title.trim().to_string();
    if title.is_empty() || payload.price < 0.0 {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "title and non-negative price required",
        ));
    }
    let conn = state.db.lock().await;
    conn.execute(
        "INSERT INTO products (title, category, price, popularity, desc, created_at) VALUES (?, ?, ?, ?, ?, ?)",
        params![
            title,
            payload.category.unwrap_or_else(|| "general".to_string()).trim().to_string(),
            payload.price,
            50_i64,
            payload.desc.unwrap_or_default().trim().to_string(),
            now_ms()
        ],
    )?;
    let id = conn.last_insert_rowid();
    let product: Product = conn.query_row(
        "SELECT id, title, category, price, popularity, desc FROM products WHERE id=?",
        [id],
        |r| {
            Ok(Product {
                id: r.get(0)?,
                title: r.get(1)?,
                category: r.get(2)?,
                price: r.get(3)?,
                popularity: r.get(4)?,
                desc: r.get(5)?,
            })
        },
    )?;
    Ok((StatusCode::CREATED, Json(json!({ "product": product }))))
}

async fn post_checkout(
    State(state): State<AppState>,
    Json(payload): Json<CheckoutRequest>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    if payload.items.is_empty() {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "items required"));
    }
    let mut conn = state.db.lock().await;
    let mut normalized = Vec::<CheckoutItem>::new();
    let mut total = 0.0_f64;
    for item in payload.items {
        if item.qty <= 0 {
            continue;
        }
        let row: Option<(i64, String, f64)> = conn
            .query_row(
                "SELECT id, title, price FROM products WHERE id=?",
                [item.id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        if let Some((id, title, price)) = row {
            total += price * item.qty as f64;
            normalized.push(CheckoutItem {
                product_id: id,
                title,
                price,
                qty: item.qty,
            });
        }
    }
    if normalized.is_empty() {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "no valid items"));
    }
    let created_at = now_ms();
    let email = payload.email.unwrap_or_default().trim().to_string();
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO orders (email, total, created_at) VALUES (?, ?, ?)",
        params![email, (total * 100.0).round() / 100.0, created_at],
    )?;
    let order_id = tx.last_insert_rowid();
    for it in &normalized {
        tx.execute(
            "INSERT INTO order_items (order_id, product_id, title, price, qty) VALUES (?, ?, ?, ?, ?)",
            params![order_id, it.product_id, it.title, it.price, it.qty],
        )?;
    }
    tx.commit()?;
    let order = OrderResponse {
        id: order_id,
        email,
        total: (total * 100.0).round() / 100.0,
        created_at,
        items: normalized,
    };
    Ok((
        StatusCode::CREATED,
        Json(json!({ "ok": true, "order": order })),
    ))
}

async fn get_sale_listing(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let conn = state.db.lock().await;
    let listing: Option<SaleListing> = conn
        .query_row(
            "SELECT ask, gmv, traffic, note, status, updated_at FROM sale_listing WHERE id=1",
            [],
            |r| {
                Ok(SaleListing {
                    ask: r.get(0)?,
                    gmv: r.get(1)?,
                    traffic: r.get(2)?,
                    note: r.get(3)?,
                    status: r.get(4)?,
                    updated_at: r.get(5)?,
                })
            },
        )
        .optional()?;
    Ok(Json(json!({ "listing": listing })))
}

async fn post_sale_listing(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<SaleListingUpsert>,
) -> Result<Json<Value>, ApiError> {
    let ctx = auth_context_from_headers(&headers, &state).await?;
    enforce_role(&ctx.user, &[Role::Owner])?;
    if payload.ask <= 0.0 {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "ask price must be > 0",
        ));
    }
    let conn = state.db.lock().await;
    let now = now_ms();
    conn.execute(
        "INSERT INTO sale_listing (id, ask, gmv, traffic, note, status, updated_at) VALUES (1, ?, ?, ?, ?, ?, ?) ON CONFLICT(id) DO UPDATE SET ask=excluded.ask, gmv=excluded.gmv, traffic=excluded.traffic, note=excluded.note, status=excluded.status, updated_at=excluded.updated_at",
        params![
            payload.ask,
            payload.gmv.unwrap_or(0.0),
            payload.traffic.unwrap_or(0),
            payload.note.unwrap_or_default().trim().to_string(),
            "active",
            now
        ],
    )?;
    let listing: SaleListing = conn.query_row(
        "SELECT ask, gmv, traffic, note, status, updated_at FROM sale_listing WHERE id=1",
        [],
        |r| {
            Ok(SaleListing {
                ask: r.get(0)?,
                gmv: r.get(1)?,
                traffic: r.get(2)?,
                note: r.get(3)?,
                status: r.get(4)?,
                updated_at: r.get(5)?,
            })
        },
    )?;
    Ok(Json(json!({ "ok": true, "listing": listing })))
}

async fn get_sale_offers(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let ctx = auth_context_from_headers(&headers, &state).await?;
    enforce_role(&ctx.user, &[Role::Owner, Role::Staff])?;
    let conn = state.db.lock().await;
    let mut stmt = conn
        .prepare("SELECT id, buyer, offer, status, created_at FROM sale_offers ORDER BY id DESC")?;
    let rows = stmt.query_map([], |r| {
        Ok(SaleOffer {
            id: r.get(0)?,
            buyer: r.get(1)?,
            offer: r.get(2)?,
            status: r.get(3)?,
            created_at: r.get(4)?,
        })
    })?;
    let mut offers = Vec::new();
    for row in rows {
        offers.push(row?);
    }
    Ok(Json(json!({ "offers": offers })))
}

async fn post_sale_offer(
    State(state): State<AppState>,
    Json(payload): Json<SaleOfferCreate>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let buyer = payload.buyer.trim().to_lowercase();
    if buyer.is_empty() || payload.offer <= 0.0 {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "buyer and offer required",
        ));
    }
    let conn = state.db.lock().await;
    conn.execute(
        "INSERT INTO sale_offers (buyer, offer, status, created_at) VALUES (?, ?, ?, ?)",
        params![buyer, payload.offer, "pending", now_ms()],
    )?;
    let id = conn.last_insert_rowid();
    let offer: SaleOffer = conn.query_row(
        "SELECT id, buyer, offer, status, created_at FROM sale_offers WHERE id=?",
        [id],
        |r| {
            Ok(SaleOffer {
                id: r.get(0)?,
                buyer: r.get(1)?,
                offer: r.get(2)?,
                status: r.get(3)?,
                created_at: r.get(4)?,
            })
        },
    )?;
    Ok((StatusCode::CREATED, Json(json!({ "offer": offer }))))
}

async fn approve_sale_offer(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let ctx = auth_context_from_headers(&headers, &state).await?;
    enforce_role(&ctx.user, &[Role::Owner])?;
    let mut conn = state.db.lock().await;
    let current: Option<SaleOffer> = conn
        .query_row(
            "SELECT id, buyer, offer, status, created_at FROM sale_offers WHERE id=?",
            [id],
            |r| {
                Ok(SaleOffer {
                    id: r.get(0)?,
                    buyer: r.get(1)?,
                    offer: r.get(2)?,
                    status: r.get(3)?,
                    created_at: r.get(4)?,
                })
            },
        )
        .optional()?;
    let existing =
        current.ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "offer not found"))?;
    if existing.status != "pending" {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "offer already processed",
        ));
    }
    let tx = conn.transaction()?;
    tx.execute("UPDATE sale_offers SET status='approved' WHERE id=?", [id])?;
    tx.execute(
        "UPDATE sale_offers SET status='rejected' WHERE status='pending' AND id<>?",
        [id],
    )?;
    tx.execute(
        "UPDATE sale_listing SET status='sold', updated_at=? WHERE id=1",
        [now_ms()],
    )?;
    tx.commit()?;
    let approved: SaleOffer = conn.query_row(
        "SELECT id, buyer, offer, status, created_at FROM sale_offers WHERE id=?",
        [id],
        |r| {
            Ok(SaleOffer {
                id: r.get(0)?,
                buyer: r.get(1)?,
                offer: r.get(2)?,
                status: r.get(3)?,
                created_at: r.get(4)?,
            })
        },
    )?;
    Ok(Json(json!({ "ok": true, "approved": approved })))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app_env = std::env::var("APP_ENV").unwrap_or_else(|_| "development".to_string());
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(4180);
    let bind_host = std::env::var("BIND_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let jwt_secret =
        std::env::var("JWT_SECRET").unwrap_or_else(|_| "dev-jwt-secret-change-me".to_string());
    let seed_demo_users =
        std::env::var("SEED_DEMO_USERS").unwrap_or_else(|_| "true".to_string()) == "true";
    let db_path = std::env::var("DB_PATH").unwrap_or_else(|_| "data/store.sqlite".to_string());
    let bootstrap_owner_email = std::env::var("BOOTSTRAP_OWNER_EMAIL").ok();
    let bootstrap_owner_password = std::env::var("BOOTSTRAP_OWNER_PASSWORD").ok();
    let bootstrap_owner = match (bootstrap_owner_email, bootstrap_owner_password) {
        (Some(email), Some(password))
            if !email.trim().is_empty() && !password.trim().is_empty() =>
        {
            Some((email, password))
        }
        _ => None,
    };

    if app_env == "production" && jwt_secret == "dev-jwt-secret-change-me" {
        return Err("JWT_SECRET must be set in production".into());
    }

    let db_file = PathBuf::from(&db_path);
    let data_dir = db_file
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data"));
    if !data_dir.exists() {
        std::fs::create_dir_all(&data_dir)?;
    }
    let conn = Connection::open(&db_file)?;
    init_schema(
        &conn,
        seed_demo_users,
        bootstrap_owner
            .as_ref()
            .map(|(email, password)| (email.as_str(), password.as_str())),
    )
    .map_err(|e| format!("init schema failed: {}", e.message))?;

    let state = AppState {
        db: Arc::new(Mutex::new(conn)),
        jwt_secret: jwt_secret.clone(),
        login_attempts: Arc::new(Mutex::new(HashMap::new())),
    };

    let cors_origins = std::env::var("CORS_ORIGINS").unwrap_or_default();
    let cors_layer = if cors_origins.trim().is_empty() {
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any)
    } else {
        let origins: Vec<HeaderValue> = cors_origins
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse::<HeaderValue>().ok())
            .collect();
        if origins.is_empty() {
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any)
        } else {
            CorsLayer::new()
                .allow_origin(origins)
                .allow_methods(Any)
                .allow_headers(Any)
        }
    };

    let api_routes = Router::new()
        .route("/api/auth/login", post(post_auth_login))
        .route("/api/auth/refresh", post(post_auth_refresh))
        .route("/api/auth/logout", post(post_auth_logout))
        .route("/api/auth/password/change", post(post_auth_password_change))
        .route("/api/auth/me", get(get_auth_me))
        .route("/api/shop", get(get_shop))
        .route("/api/shop/settings", patch(patch_shop_settings))
        .route("/api/products", get(get_products).post(post_products))
        .route("/api/checkout", post(post_checkout))
        .route(
            "/api/sale/listing",
            get(get_sale_listing).post(post_sale_listing),
        )
        .route(
            "/api/sale/offers",
            get(get_sale_offers).post(post_sale_offer),
        )
        .route("/api/sale/offers/{id}/approve", post(approve_sale_offer));
    let unprefixed_api_routes = Router::new()
        .route("/auth/login", post(post_auth_login))
        .route("/auth/refresh", post(post_auth_refresh))
        .route("/auth/logout", post(post_auth_logout))
        .route("/auth/password/change", post(post_auth_password_change))
        .route("/auth/me", get(get_auth_me))
        .route("/shop", get(get_shop))
        .route("/shop/settings", patch(patch_shop_settings))
        .route("/products", get(get_products).post(post_products))
        .route("/checkout", post(post_checkout))
        .route(
            "/sale/listing",
            get(get_sale_listing).post(post_sale_listing),
        )
        .route("/sale/offers", get(get_sale_offers).post(post_sale_offer))
        .route("/sale/offers/{id}/approve", post(approve_sale_offer));
    let app = Router::new()
        .route("/health", get(health))
        .route("/", get(health))
        .merge(api_routes)
        .merge(unprefixed_api_routes)
        .layer(cors_layer)
        .with_state(state);

    let addr: SocketAddr = format!("{bind_host}:{port}").parse()?;
    println!("Internet Shop backend (Rust) listening on http://{}", addr);
    println!("Environment: {}", app_env);
    println!("JWT secret loaded (len={} chars)", jwt_secret.len());
    println!("Demo user seeding: {}", seed_demo_users);
    println!(
        "Bootstrap owner configured: {}",
        if bootstrap_owner.is_some() {
            "true"
        } else {
            "false"
        }
    );
    println!("DB path: {}", db_path);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
