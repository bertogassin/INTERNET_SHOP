# frontend

Internet Shop frontend app.

Current stack:
- Vite static frontend (`index.html` + inline JS/CSS)
- Mobile-first storefront UI
- Connected to backend API (local default `http://127.0.0.1:4180`, production default same-origin `/api`)
- Auth UX with login form, role status, logout, and password change
- Access + refresh token flow (auto-refresh on 401)

## Run locally

1. Start backend in `../backend`:
   - `cargo run`
2. Start frontend:
   - `npm install`
   - `npm run dev -- --host 127.0.0.1 --port 4173`

Open:
- `http://127.0.0.1:4173`

## Connected features

- Storefront catalog via `GET /api/products`
- Shop theme load/save via `GET /api/shop` and `PATCH /api/shop/settings` (owner/staff)
- Checkout via `POST /api/checkout`
- Sell Shop listing via `GET/POST /api/sale/listing` (`POST` owner)
- Offers queue and approval via `GET /api/sale/offers` (owner/staff) and `POST /api/sale/offers/:id/approve` (owner)

## Demo auth users

- `owner@internet.shop / Owner123!`
- `staff@internet.shop / Staff123!`
- `viewer@internet.shop / Viewer123!`

## API base override

By default:
- Local dev (`localhost/127.0.0.1`): `http://127.0.0.1:4180`
- Production host: same-origin path (for reverse-proxy or App Platform route `/api`)

You can override at runtime:
- Set `window.__INTERNET_SHOP_API_BASE__` before app script loads, or
- Set `localStorage.internet_shop_api_base` manually in browser devtools.
