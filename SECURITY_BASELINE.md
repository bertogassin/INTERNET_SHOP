# SECURITY_BASELINE

Repository-level minimum security controls.

## Required controls
- Run security CI on every PR and push to `main`.
- Block hardcoded secrets in the repository.
- Run dependency vulnerability checks for detected stacks.
- Keep authentication and authorization deny-by-default.
- Validate and sanitize all external inputs.

## Session and token policy
- Short-lived access tokens and revocable refresh sessions where auth exists.
- Immediate session revocation on logout/password rotation for auth-enabled services.

## Change policy
- Security-relevant changes require risk notes in PR.
- Every PR must include test plan and rollback notes.
