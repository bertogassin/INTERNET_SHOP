use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
    Json, Router,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};

#[derive(Clone)]
struct AppState {
    db: Arc<Mutex<Connection>>,
    owner_token: String,
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

fn owner_only(headers: &HeaderMap, state: &AppState) -> Result<(), ApiError> {
    let token = headers
        .get("x-owner-token")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    if token != state.owner_token {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "owner token required",
        ));
    }
    Ok(())
}

fn init_schema(conn: &Connection) -> Result<(), ApiError> {
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
    Ok(())
}

async fn health() -> Json<HealthResponse<'static>> {
    Json(HealthResponse {
        status: "ok",
        service: "internet-shop-backend",
        storage: "sqlite",
    })
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
    owner_only(&headers, &state)?;
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
        "INSERT INTO shop_settings (id, name, tagline, accent) VALUES (1, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, tagline = excluded.tagline, accent = excluded.accent",
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
    owner_only(&headers, &state)?;
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
        "SELECT id, title, category, price, popularity, desc FROM products WHERE id = ?",
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
        let qty = item.qty;
        if qty <= 0 {
            continue;
        }
        let row: Option<(i64, String, f64)> = conn
            .query_row(
                "SELECT id, title, price FROM products WHERE id = ?",
                [item.id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        if let Some((id, title, price)) = row {
            total += price * qty as f64;
            normalized.push(CheckoutItem {
                product_id: id,
                title,
                price,
                qty,
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
            "SELECT ask, gmv, traffic, note, status, updated_at FROM sale_listing WHERE id = 1",
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
    owner_only(&headers, &state)?;
    if payload.ask <= 0.0 {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "ask price must be > 0",
        ));
    }
    let conn = state.db.lock().await;
    let now = now_ms();
    conn.execute(
        "INSERT INTO sale_listing (id, ask, gmv, traffic, note, status, updated_at) VALUES (1, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET ask = excluded.ask, gmv = excluded.gmv, traffic = excluded.traffic, note = excluded.note, status = excluded.status, updated_at = excluded.updated_at",
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
        "SELECT ask, gmv, traffic, note, status, updated_at FROM sale_listing WHERE id = 1",
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
    owner_only(&headers, &state)?;
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
        "SELECT id, buyer, offer, status, created_at FROM sale_offers WHERE id = ?",
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
    owner_only(&headers, &state)?;
    let mut conn = state.db.lock().await;
    let row: Option<SaleOffer> = conn
        .query_row(
            "SELECT id, buyer, offer, status, created_at FROM sale_offers WHERE id = ?",
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

    let current = match row {
        Some(r) => r,
        None => return Err(ApiError::new(StatusCode::NOT_FOUND, "offer not found")),
    };
    if current.status != "pending" {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "offer already processed",
        ));
    }

    let tx = conn.transaction()?;
    tx.execute(
        "UPDATE sale_offers SET status = 'approved' WHERE id = ?",
        [id],
    )?;
    tx.execute(
        "UPDATE sale_offers SET status = 'rejected' WHERE status = 'pending' AND id <> ?",
        [id],
    )?;
    tx.execute(
        "UPDATE sale_listing SET status = 'sold', updated_at = ? WHERE id = 1",
        [now_ms()],
    )?;
    tx.commit()?;

    let approved: SaleOffer = conn.query_row(
        "SELECT id, buyer, offer, status, created_at FROM sale_offers WHERE id = ?",
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
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(4180);
    let owner_token =
        std::env::var("OWNER_TOKEN").unwrap_or_else(|_| "dev-owner-token".to_string());

    let data_dir = PathBuf::from("data");
    if !data_dir.exists() {
        std::fs::create_dir_all(&data_dir)?;
    }
    let db_path = data_dir.join("store.sqlite");
    let conn = Connection::open(db_path)?;
    init_schema(&conn).map_err(|e| format!("init schema failed: {}", e.message))?;

    let state = AppState {
        db: Arc::new(Mutex::new(conn)),
        owner_token: owner_token.clone(),
    };

    let app = Router::new()
        .route("/health", get(health))
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
        .route("/api/sale/offers/{id}/approve", post(approve_sale_offer))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    println!("Internet Shop backend (Rust) listening on http://{}", addr);
    println!("Owner token: {}", owner_token);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
