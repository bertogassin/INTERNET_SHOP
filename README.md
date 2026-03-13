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

## Next steps

1. Initialize git and create GitHub repo `INTERNET_SHOP`.
2. Move Internet Shop product code only here.
3. Keep `OMNIXIUS` with links to this repo and future shop domain.
