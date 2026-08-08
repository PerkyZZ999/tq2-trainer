# EXP research notes

Historical notes from the 2026-08 discovery session. Day-to-day usage lives in the [README](../README.md); the live patch recipe is in [EXP-PATCH.md](EXP-PATCH.md).

## Environment

| Field | Value |
|-------|-------|
| OS | CachyOS Linux |
| Game | Titan Quest II (AppID `1154030`) |
| Runner | GE-Proton |
| Process | `TQ2-Win64-Shipping.exe` |
| Yama `ptrace_scope` | `1` (same-user access still worked; `setcap` preferred) |
| FLiNG | Unavailable / unstable under Proton — not used |

## What worked

- Process discovery via `/proc` cmdline ending with `TQ2-Win64-Shipping.exe`
- Read-only `process_vm_readv`; `.text` writes via `/proc/<pid>/mem`
- UI EXP commas are thousands separators only
- Live current EXP is **i64** at **required − 32**; the object relocates (GC) — value scanning alone is fragile
- Award path: Unreal **`AddXP`** @ RVA **`0x6B3A890`** (unique prologue; many callers)
- Live multiplier trampoline in INT3 cave @ AddXP `+0x262` — validated in-game (including 10×)

## What failed / is unsafe

- Hardware watchpoints (gdb / ptrace debug registers) **crash** TQ2 under Proton — do not use
- Absolute process addresses for EXP data — they move between gains / sessions

## Collaborative value scan (optional)

If you need to rediscover live EXP addresses after an update:

```bash
./target/release/tq2-trainer research --target exp snap <current_exp>
# gain some EXP in-game
./target/release/tq2-trainer research --target exp narrow <new_exp>
./target/release/tq2-trainer research --target exp list
```

Candidates are written to `research-dumps/exp-candidates.txt` (gitignored). Default `--target` is `exp` if omitted.

Prefer re-finding **AddXP** for the multiplier (see EXP-PATCH.md) over chasing relocating data values.

For gold one-shot adds, use `--target gold` and see [GOLD-RESEARCH.md](GOLD-RESEARCH.md).

## Archive

The original agent handoff (long-form planning context) is in [`archive/HANDOFF.md`](archive/HANDOFF.md).
