# SECURITY_BASELINE

## Scope

Wave-1 baseline hardening for INTERNET_SHOP.

## Requirements

- Rust backend must pass `cargo check` and `cargo audit`.
- Frontend dependency audit runs on each PR/push.
- Secret scanning enabled in CI.
- Role-based access control required for protected actions.
- Session revocation required for logout/password changes.
