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

Auth model:
- Access + refresh flow (`JWT + rotating refresh token`)
- Protected endpoints require `Authorization: Bearer <access_token>`
- Roles: `owner`, `staff`, `viewer`
- Login rate-limit: max 8 attempts per 15 minutes per email/ip fingerprint
- Override signing secret with env:
  - `JWT_SECRET=your-secret cargo run`

Default seeded users (dev only):
- `owner@internet.shop / Owner123!`
- `staff@internet.shop / Staff123!`
- `viewer@internet.shop / Viewer123!`

## Implemented endpoints

- `GET /health`
- `POST /api/auth/login`
- `POST /api/auth/refresh`
- `POST /api/auth/logout`
- `POST /api/auth/password/change`
- `GET /api/auth/me` (any authenticated role)
- `GET /api/shop`
- `PATCH /api/shop/settings` (owner/staff)
- `GET /api/products`
- `POST /api/products` (owner/staff)
- `POST /api/checkout`
- `GET /api/sale/listing`
- `POST /api/sale/listing` (owner)
- `GET /api/sale/offers` (owner/staff)
- `POST /api/sale/offers`
- `POST /api/sale/offers/:id/approve` (owner)

Data persistence:
- SQLite file at `backend/data/store.sqlite` (auto-created on first start).

## Production runtime vars

- `APP_ENV=production` enables strict checks (fails startup if `JWT_SECRET` is default).
- `PORT=8080` and `BIND_HOST=0.0.0.0` for container runtime.
- `DB_PATH=/app/data/store.sqlite` to control SQLite location.
- `SEED_DEMO_USERS=false` to disable demo user bootstrap.
- `BOOTSTRAP_OWNER_EMAIL=owner@your-domain` and `BOOTSTRAP_OWNER_PASSWORD=<strong-password>` for first owner creation when demo users are disabled.
- `CORS_ORIGINS=https://your-frontend-domain` (comma-separated list supported).
- `JWT_SECRET=<strong-random-secret>` required in production.

## Docker

Build:

```bash
docker build -t internet-shop-api ./backend
```

Run:

```bash
docker run --rm -p 8080:8080 \
  -e APP_ENV=production \
  -e PORT=8080 \
  -e BIND_HOST=0.0.0.0 \
  -e DB_PATH=/app/data/store.sqlite \
  -e SEED_DEMO_USERS=false \
  -e BOOTSTRAP_OWNER_EMAIL=owner@your-domain \
  -e BOOTSTRAP_OWNER_PASSWORD=replace-with-strong-password \
  -e CORS_ORIGINS=https://your-frontend-domain \
  -e JWT_SECRET=replace-with-strong-secret \
  internet-shop-api
```
