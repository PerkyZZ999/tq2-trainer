# Contributing

Thanks for taking an interest. This is a small single-player Linux trainer for Titan Quest II.

## Before you open a PR

1. Keep the scope **single-player / offline**. No multiplayer, anti-cheat evasion, or stealth features.
2. Run:

   ```bash
   cargo fmt
   cargo clippy --all-targets --all-features -- -D warnings
   cargo test
   ```

3. Do **not** commit files under `research-dumps/` (memory address dumps).
4. After a game update, update `signatures/tq2.toml` (SHA-256 + RVAs/prologues) and rebuild — that file is the build-profile source of truth.

## Bug reports

Please include:

- Distro / kernel (briefly)
- Proton / Steam runtime if relevant
- `tq2-trainer status -v` output (redact anything personal)
- Whether EXP, sell-gold, or research is involved
- Game build: `status` support line or Shipping.exe SHA-256 if you have it

## Security / safety

Process memory tooling can crash the game. Prefer save backups. Experimental commands that are known-unstable should stay gated (see `gold --unsafe-grant`).
