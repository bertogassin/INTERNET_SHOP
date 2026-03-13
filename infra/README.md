# infra

Deployment and runtime config for Internet Shop.

## DigitalOcean App Platform

Spec file:
- `infra/digitalocean-app.yaml`

Deploy commands:

```bash
doctl auth init --access-token "$DO_TOKEN"
doctl apps create --spec infra/digitalocean-app.yaml
```

Update existing app:

```bash
doctl apps update <APP_ID> --spec infra/digitalocean-app.yaml
```

Before production deploy:
- Set `JWT_SECRET` to a strong random secret in App settings.
- Set `BOOTSTRAP_OWNER_PASSWORD` to a strong password and `BOOTSTRAP_OWNER_EMAIL` to your real admin email.
- Replace `CORS_ORIGINS` placeholder with your real frontend domain.
- Keep `SEED_DEMO_USERS=false`.
