# Titan Quest II Native Trainer

Linux-native Rust CLI for Titan Quest II (Steam + Proton). Applies a reversible **experience multiplier** by patching the live `AddXP` function, and can force the next item sale's gold payout via `sell-gold` — no Wine trainer, no injection framework.

**Single-player / offline personal use only.**

## Quick start

```bash
# Build
cargo build --release

# If status/xp fails with a permission error (Yama ptrace_scope=1):
sudo setcap cap_sys_ptrace=ep target/release/tq2-trainer

# Start Titan Quest II, then:
./target/release/tq2-trainer status
./target/release/tq2-trainer xp 10
./target/release/tq2-trainer restore
```

EXP presets: **`1`** (original), **`2`**, **`3`**, **`5`**, **`10`**.

## Daily commands

| Command | What it does |
|---------|----------------|
| `status` | Find the game, check build support, show current multiplier |
| `xp <n>` | Apply multiplier (`1` restores original) |
| `restore` | Same as `xp 1` |
| `sell-gold <n> [--current <g>]` | Next sold item pays `n` gold, then auto-restores |
| `sell-gold --disarm` | Cancel an armed sell-gold override |
| `gold <n> --unsafe-grant …` | Experimental free grant (can crash — prefer `sell-gold`) |
| `scan` | List Shipping.exe memory mappings (diagnostics) |
| `research --target exp\|gold …` | Collaborative value scan (`snap` / `narrow` / `list` / `probe`) |
| `-v …` | Verbose details (bases, patch sites, etc.) |

Examples:

```bash
./target/release/tq2-trainer xp 5
./target/release/tq2-trainer -v status
./target/release/tq2-trainer restore

# Next sale pays exactly 50000 gold (pass --current so it can detect + restore):
./target/release/tq2-trainer sell-gold 50000 --current 58098
# … sell one item …
```

## Requirements

- Linux (developed on CachyOS)
- Steam / Proton Titan Quest II (`TQ2-Win64-Shipping.exe`)
- Permission to read/write the game process (see below)
- A supported game build (AddXP signature must match)

## Process access (ptrace / Yama)

With `kernel.yama.ptrace_scope=1`, unrelated processes may be denied `process_vm_readv` / `/proc/<pid>/mem`.

The trainer does **not** change that sysctl. If you see a permission error:

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

If `status` reports an unsupported build, the AddXP signature no longer matches. See [`docs/EXP-PATCH.md`](docs/EXP-PATCH.md) for the patch recipe and [`docs/RESEARCH.md`](docs/RESEARCH.md) for rediscovery notes. The built-in `research --target exp snap|narrow|list` helpers can help isolate live EXP values again.

Gold uses a temporary SellItem payout patch (same build fingerprint as EXP). Pass `--current <on-screen gold>` so the trainer can detect the grant and restore. Value-scan `research --target gold probe` only writes memory mirrors — it does **not** update the Currencies UI. The experimental `gold` free-grant path requires `--unsafe-grant` and can crash. See [`docs/GOLD-RESEARCH.md`](docs/GOLD-RESEARCH.md).

## Develop

```bash
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

## Layout

```text
src/            CLI + process/memory/patch logic
signatures/     Human-readable patch profile (mirrors code constants)
docs/           Patch reference + research notes
research-dumps/ Local scan artifacts (gitignored; created on demand)
```

## Disclaimer

- Personal single-player / offline use only — do not use in multiplayer.
- Game updates may invalidate the signature; unsupported builds are refused.
- Back up important saves before patching.
- Restarting the game clears the live patch; re-run `xp` after launch if you want it again.
