# backend

Internet Shop backend API (Rust-first MVP runtime).

Core domains:
- shop settings (theme/branding)
- catalog (products)
- checkout (orders)
- sale module (listing/offers/approval)

## Run locally

```bash
cargo run
```

Backend default URL:
- `http://127.0.0.1:4180`

Owner auth:
- Protected endpoints require header `x-owner-token`
- Default token: `dev-owner-token`
- Override with env: `OWNER_TOKEN=your-secret-token cargo run`

## Implemented endpoints

- `GET /health`
- `GET /api/shop`
- `PATCH /api/shop/settings` (owner)
- `GET /api/products`
- `POST /api/products` (owner)
- `POST /api/checkout`
- `GET /api/sale/listing`
- `POST /api/sale/listing` (owner)
- `GET /api/sale/offers` (owner)
- `POST /api/sale/offers`
- `POST /api/sale/offers/:id/approve` (owner)

Data persistence:
- SQLite file at `backend/data/store.sqlite` (auto-created on first start).
