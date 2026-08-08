# Titan Quest II Native Trainer

Linux-native Rust CLI for **Titan Quest II** (Steam + Proton). Patch a live EXP multiplier and force the next sold item’s gold payout — no Windows trainer, no Wine/.NET wrapper, no injector framework.

**Single-player / offline personal use only.** Unofficial fan project — not affiliated with the game’s publishers.

## Features

- **EXP multiplier** presets: `1×` (restore), `2×`, `3×`, `5×`, `10×`
- **`sell-gold`** — next item you sell pays exactly the amount you choose, then the patch restores itself
- Refuses unknown game builds (signature check)
- Optional collaborative value-scan helpers for research after updates

## Requirements

- Linux (developed on CachyOS; should work on other distros)
- Steam / Proton install of Titan Quest II (`TQ2-Win64-Shipping.exe`)
- Rust toolchain (`cargo`) to build from source
- Permission to read/write the game process (see [Process access](#process-access-ptrace--yama))

### Supported build (current)

| Field | Value |
|-------|-------|
| Process | `TQ2-Win64-Shipping.exe` |
| Steam AppID | `1154030` |
| Executable SHA-256 | `79392aa1ed71e8ea01a77a3b40cc15d2f87a58a645b8a86f95cd361276ed73b0` |

Game updates often move code. If `status` says unsupported, see [`docs/EXP-PATCH.md`](docs/EXP-PATCH.md) and [`docs/RESEARCH.md`](docs/RESEARCH.md).

## Quick start

```bash
git clone https://github.com/PerkyZZ999/tq2-trainer.git
cd tq2-trainer
cargo build --release

# If status/xp fails with a permission error (Yama ptrace_scope=1):
sudo setcap cap_sys_ptrace=ep target/release/tq2-trainer

# Start Titan Quest II, then:
./target/release/tq2-trainer status
./target/release/tq2-trainer xp 10
./target/release/tq2-trainer restore
```

## Commands

| Command | What it does |
|---------|----------------|
| `status` | Find the game, check build support, show current EXP multiplier |
| `xp <n>` | Apply multiplier (`1` restores original) |
| `restore` | Same as `xp 1` |
| `sell-gold <n> --current <g>` | Next sold item pays `n` gold, then auto-restores |
| `sell-gold --disarm` | Cancel an armed sell-gold override |
| `scan` | List Shipping.exe memory mappings (diagnostics) |
| `research --target exp\|gold …` | Collaborative value scan (`snap` / `narrow` / `list` / `probe`) |
| `-v …` | Verbose details (bases, patch sites, etc.) |
| `--force` | Skip Shipping.exe SHA-256 check (unsafe / research only) |

Examples:

```bash
./target/release/tq2-trainer xp 5
./target/release/tq2-trainer -v status
./target/release/tq2-trainer restore

# You have 58098 gold; next sale should pay 50000:
./target/release/tq2-trainer sell-gold 50000 --current 58098
# … sell exactly one item …
```

Writes refuse unknown Shipping.exe hashes (see supported build table). Re-run `setcap` after rebuilds.
## Process access (ptrace / Yama)

With `kernel.yama.ptrace_scope=1`, unrelated processes may be denied `process_vm_readv` / `/proc/<pid>/mem`.

This tool does **not** change that sysctl. If you see a permission error:

1. **Preferred** — grant the capability to the release binary:

   ```bash
   sudo setcap cap_sys_ptrace=ep target/release/tq2-trainer
   ```

   Re-run `setcap` after each `cargo build --release` (rebuilds replace the binary).

2. **Temporary (dev only):**

   ```bash
   sudo sysctl kernel.yama.ptrace_scope=0
   ```

## After a game update

If `status` reports an unsupported build, the AddXP / gold signatures no longer match. See:

- [`docs/EXP-PATCH.md`](docs/EXP-PATCH.md) — EXP patch recipe
- [`docs/GOLD-RESEARCH.md`](docs/GOLD-RESEARCH.md) — gold / sell-gold notes
- [`docs/RESEARCH.md`](docs/RESEARCH.md) — rediscovery notes
- `research --target exp|gold snap|narrow|list` — live value helpers

## Develop

```bash
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

## Layout

```text
src/            CLI + process / memory / patch logic
signatures/     Human-readable patch profile (mirrors code constants)
docs/           Patch reference + research notes
research-dumps/ Local scan artifacts (gitignored; created on demand)
```

## Disclaimer

- Personal **single-player / offline** use only — do not use in multiplayer.
- Not affiliated with, endorsed by, or related to the Titan Quest II developers or publishers.
- Game updates may invalidate signatures; unsupported builds are refused.
- Back up important saves before patching.
- Prefer save backups. Don’t leave a world-writable `setcap` binary around — `cap_sys_ptrace` can read/write other processes you own.
- Restarting the game clears live patches; re-run `xp` / `sell-gold` after launch if needed.
- You are responsible for complying with the game’s Terms of Service and applicable law.

## License

MIT — see [`LICENSE`](LICENSE).
