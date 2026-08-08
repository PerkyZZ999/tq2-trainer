# EXP patch reference

Validated live on Titan Quest II Early Access (researched **2026-08**).

| Field | Value |
|-------|-------|
| Process | `TQ2-Win64-Shipping.exe` |
| Steam AppID | `1154030` |
| Executable SHA-256 | `79392aa1ed71e8ea01a77a3b40cc15d2f87a58a645b8a86f95cd361276ed73b0` |
| Size | `183080448` bytes |
| Profile | `signatures/tq2.toml` (`tq2-ea-2026-08-addxp`) |

## Mechanism

Patch the Unreal **`AddXP`** native body so the incoming XP amount (`edx` → `r15d`) is multiplied before the rest of the function runs.

| Item | Value |
|------|-------|
| AddXP RVA | `0x6B3A890` |
| Calling convention | `rcx` = character/object, `edx` = XP amount (`int32`) |
| Entry patch @ `+0x10` | `E9 rel32; 90` → jump to cave |
| Continue @ `+0x16` | original `call` resume point |
| Cave @ `+0x262` | 14× `INT3` padding → trampoline |

### Stable prologue (signature; not overwritten)

```text
48 89 5C 24 20 55 56 41 57 48 81 EC C0 00 00 00
```

### Original entry bytes (`+0x10`, restored on `xp 1`)

```text
44 8B FA 48 8B F1          ; mov r15d, edx ; mov rsi, rcx
```

### Cave trampoline (example: 10×)

```text
6B D2 0A                   ; imul edx, edx, 10
44 8B FA                   ; mov r15d, edx
48 8B F1                   ; mov rsi, rcx
E9 xx xx xx xx             ; jmp continue (+0x16)
```

Presets: `1` (restore), `2`, `3`, `5`, `10`.

## Proton mapping note

Wine/Proton maps Shipping.exe as:

1. Tiny named `r--p` PE header → **module base**
2. Large anonymous `r-xp` `.text` (~123 MiB)
3. Further anonymous/named sections

Live VA = `module_base + RVA`. Writes use `/proc/<pid>/mem` (FOLL_FORCE) so `.text` patches work without remapping.

## Safety rules

- Refuse to write if the prologue / entry / cave bytes are unexpected.
- No ptrace hardware watchpoints — they crash TQ2 under Proton.
- Multiplayer / anti-cheat / stealth features are out of scope.

## When the signature breaks

1. Confirm SHA-256 of `TQ2-Win64-Shipping.exe` changed.
2. Re-locate `AddXP` (offline PE / string xrefs) or re-scan the unique prologue.
3. Update constants in `src/exp.rs` and `signatures/tq2.toml`.
4. Re-test with `status` → `xp 2` → in-game kill → `restore`.
