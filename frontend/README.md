# frontend

Internet Shop frontend app.

Current stack:
- Vite static frontend (`index.html` + inline JS/CSS)
- Mobile-first storefront UI
- Connected to local backend API (`http://127.0.0.1:4180`)
- Auth actions use Bearer token from backend login flow

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

## Build contract

- Frontend build is Vite-only (`npm run build` -> `vite build`).
- TypeScript project references are not required for the current static HTML frontend.
