FROM rust:1.89-bookworm AS builder
WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim
WORKDIR /app
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/internet_shop_backend /usr/local/bin/internet_shop_backend

ENV APP_ENV=production
ENV PORT=8080
ENV BIND_HOST=0.0.0.0
ENV DB_PATH=/app/data/store.sqlite
ENV SEED_DEMO_USERS=false

EXPOSE 8080
CMD ["internet_shop_backend"]
