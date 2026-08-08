# Gold research notes

Discovery notes for one-shot gold. Day-to-day EXP usage lives in the [README](../README.md).

## Important: Gold ≠ generic “currency”

In-game **Currencies** tab lists several stacks (Gold, Embers of Night, Moly Root, …).  
**Gold is one currency item**, not a separate wallet int and not ICU/`CurrencyCode` locale strings.

## What failed (value scan)

Collaborative snap/narrow on the on-screen gold amount finds **mirrors / caches** that track gold but do **not** drive the UI when written:

| Session note | Result |
|--------------|--------|
| Narrowed to 2–3 addresses, probe +1 | UI unchanged |
| Structured scan: `i32 == gold` at `obj+0x10` with TQ2 `.rdata` vtable | 2 hits; probe +1 on both → UI unchanged |
| Float scan (`f32`/`f64` == gold) | 0 hits |

Same lesson as early EXP research: **do not trust absolute data writes** for the real wallet.

## What the EXE shows (AddXP-style RE)

Build fingerprint matches EXP patch (`TQ2-Win64-Shipping.exe` SHA-256 `79392aa1…`).

### No `AddGold`

There is **no** `AddGold` / `Debug_AddGold` string. Gold goes through **item/currency** APIs:

| Symbol | Role |
|--------|------|
| `m_pGoldItem` / `m_CurrencyTag` | Config: which item *is* gold |
| `GetGoldItemDescriptionBlueprint` | Resolves gold item description (`RVA` body `0x6BD69B0`) |
| `GetCurrencyAmount` | Sums matching stacks in a container (`RVA` body `0x6D3ED40`) |
| Amount helper | `RVA` `0x5415A50` — loops slots, adds `[item+0x10]` when description matches |
| `AddItems` / `AddItems_Local` | Proper grant path (ProcessEvent / native thunks) |
| `GiveRewards` + `ETQ2RewardType::Gold` | Reward graph (`TQ2GiveRewardsNode.cpp`) |
| `CombatGoldGain` / `OnRep_CombatGoldGain` | Combat loot gain (float-ish OnRep) |

### `GetCurrencyAmount` shape

```text
rax = [this+0x68]
rdx = [this+0x78]          ; currency item description for THIS currency row
rcx = [rax+0x518] + 0x28   ; inventory container
jmp  0x5415A50             ; sum stacks where desc matches
```

Stack count used by the helper: **`dword [item + 0x10]`**.

### UFunction table pattern (same as AddXP)

Second registration style: `name`, exec wrapper, helper, **thunk**.  
AddXP thunk `jmp`s real body `0x6B3A890`.  
`GetCurrencyAmount` thunk calls body `0x6D3ED40`.

Afford/vendor checks near AddXP (`~0x6B3B962`) call `GetGoldItemDescription` then the same amount helper and compare to a cost at `[obj+0x460]`.

## Why one-shot still needs a code path

Writing `[item+0x10]` (even on plausible UObjects) does **not** refresh the Currencies UI. The game almost certainly expects grants via **`AddItems` / reward nodes** (replication, viewmodels, container dirty flags).

That matches EXP: the durable fix was patching **`AddXP`**, not poking live EXP ints.

## Collaborative value-scan commands (still useful for mirrors / RE)

```bash
./target/release/tq2-trainer research --target gold snap <current_gold>
# change gold in-game
./target/release/tq2-trainer research --target gold narrow <new_gold>
./target/release/tq2-trainer research --target gold list
./target/release/tq2-trainer research --target gold probe <absolute>   # guarded; UI may not move
```

Candidates: `research-dumps/gold-candidates.txt`.

## Validated live path (2026-08-08)

**Forced sell payout works.** Patching the amount register immediately before the inventory-add call in `SellItem` updates the Currencies UI.

| Field | Value |
|-------|-------|
| Build / Shipping.exe SHA-256 | `79392aa1ed71e8ea01a77a3b40cc15d2f87a58a645b8a86f95cd361276ed73b0` |
| Site RVA | `0x6D21FE3` |
| Original bytes | `44 8B 4C 24 58` (`mov r9d, [rsp+0x58]`) |
| Validated patch | `6A 7B 41 59 90` (`push 123; pop r9; nop`) |
| Before → after | `10050` → **`10173`** (`+123`) |
| Auto-restore | Yes (original bytes restored after trigger) |
| Stability | Game stayed up; later sales normal after restore |

### Failed approaches during the same session

| Attempt | Result |
|---------|--------|
| Value-scan / `[item+0x10]` poke | UI unchanged |
| Far trampoline + write flag into `.text` cave | Crash (`r-x` page not writable by game code) |
| `process_vm_readv` batch monitor over thousands of mirrors | Monitor aborted mid-arm (partial read); patch restored |

### Related RE (not yet live-validated for one-shot)

| Symbol / site | Notes |
|---------------|-------|
| `SellItem` bodies | `0x6D21670` / `0x6D21730` → core `0x6D21810` → grant call uses payout in `r9d` |
| `TQ2GoldReward` handler | RVA `0x6C31B40`; amount in `r8d` / `edi` at `0x6C31B53` (`41 8B F8`) — promising for non-sale one-shot |
| Container add | RVA `0x54052A0` (called with gold descriptor + amount) |
| `AddItems` | ProcessEvent-heavy; poorer first target than gold-reward / sell payout |

## Trainer commands (wired)

```bash
# Next sold item pays N gold; waits for balance update then restores:
./target/release/tq2-trainer sell-gold <N> --current <on-screen-gold>

# Arm without waiting (cancel with --disarm):
./target/release/tq2-trainer sell-gold <N> --no-wait
./target/release/tq2-trainer sell-gold --disarm

# Experimental free grant (crashed in live testing — requires explicit opt-in):
./target/release/tq2-trainer gold <N> --current <on-screen-gold> --unsafe-grant
```

`research --target gold probe` remains available for RE mirrors only — it does **not** grant wallet gold.

## Findings log (2026-08-08)

| Field | Value |
|-------|-------|
| On-screen gold during probes | `8458` → … → `10050` → **`10173`** → **`60173`** → **`108098`** (sell-gold) |
| Value-scan / stack poke UI update? | **No** |
| Code-path amount override UI update? | **Yes** (`sell-gold`) |
| Gold width in UI path | **i32** stack field at `item+0x10` (summed) |
| `sell-gold` CLI | Validated end-to-end; auto-restore after one sale |
| Wait detection | Many snap mirrors are sticky; accept ≥1 live `after` while others stay at `before` |
| `gold` CLI | GetGold trampoline **crashed** the game in live testing; gated behind `--unsafe-grant` |

## Remaining work

1. Fix or replace the free-grant `gold` trampoline (construct / owner calling convention).
2. Avoid writing runtime flags into `r-x` caves (crash under Proton).
