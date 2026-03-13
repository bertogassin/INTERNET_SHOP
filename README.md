# INTERNET_SHOP

Dedicated product repository for the OMNIXIUS Internet Shop direction.

## Architecture boundary

- This repository contains Internet Shop product logic (catalog, checkout, shop customization, store-sale flow).
- `OMNIXIUS` repository remains hub-only (links, ecosystem map, product direction pages).

## Initial MVP modules

- Storefront core (catalog, product page, cart, checkout)
- Trend Builder (theme presets, block toggles/order, live preview)
- Sell Store module (listing, offer, approval transfer)
- Security baseline (RBAC, audit events, anti-fraud flags, rate limits)

## Suggested structure

```text
frontend/       # React app for shop owner + customer storefront
backend/        # API service and business logic
infra/          # docker/deploy/runtime configs
docs/           # contracts, rollout, security notes
```

## Production readiness

- Rust backend with JWT auth, RBAC, refresh/logout/password-change flows.
- Frontend connected to backend API with refresh-token retry flow.
- Dockerized backend runtime for deployment.
- DigitalOcean App Platform spec in `infra/digitalocean-app.yaml`.
