# Changelog

## 1.0.0 — 2026-08-08

First public release.

- Live EXP multiplier presets (`1`, `2`, `3`, `5`, `10`) via `AddXP` trampoline
- `sell-gold` next-sale payout override with auto-restore
- Shipping.exe SHA-256 gate before memory writes (`--force` to override)
- Chunked AddXP signature scan + sites built from discovered prologue address
- Build fingerprint / signature refuse for unknown Shipping.exe builds
- Collaborative `research` snap/narrow/list/probe helpers
- Hidden experimental `gold --unsafe-grant` free-grant path (known unstable)
- CI: fmt / clippy / test / release build
