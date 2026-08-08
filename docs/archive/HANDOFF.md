# Titan Quest II Native EXP Trainer — Cursor Agent Handoff (archive)

> **Archived.** Day-to-day usage: [`../../README.md`](../../README.md).  
> Live patch recipe: [`../EXP-PATCH.md`](../EXP-PATCH.md).  
> Condensed research: [`../RESEARCH.md`](../RESEARCH.md).

---

# Titan Quest II Native EXP Trainer — Cursor Agent Handoff

## 1. Project Summary

Build a small **native Linux trainer for Titan Quest II** in **Rust** whose first and primary feature is an adjustable **experience multiplier**.

The project exists because the Windows FLiNG trainer can work under Proton, but it introduces avoidable instability and complexity:

- Windows trainer executable
- Proton/Wine integration
- native .NET Framework 4.8 requirement
- Wine GUI/input quirks
- COM/OLE noise and occasional crashes
- trainer single-instance / previous-crash detection behaving unreliably under Wine

The desired replacement is a tiny Linux-native tool that directly attaches to the running Titan Quest II process and applies/restores the EXP multiplier without requiring a Windows trainer.

Initial target:

```text
tq2-trainer xp 5
```

Expected result:

```text
Titan Quest II found
PID: 123456
Supported build detected
EXP multiplier: 1x -> 5x
Patch applied successfully
```

Restore:

```text
tq2-trainer xp 1
```

or:

```text
tq2-trainer restore
```

This is intended for **single-player/offline personal use only**. Do not add anti-cheat bypasses, stealth/injection techniques, multiplayer-oriented functionality, or mechanisms intended to evade detection.

---

## 2. Current Environment

Development and target environment:

```text
OS:        CachyOS Linux
Desktop:   KDE Plasma 6 / Wayland
CPU:       AMD Ryzen 7 5800XT
GPU:       Intel Arc B580 12 GB
Steam:     Native Linux Steam client
Game:      Titan Quest II
Steam ID:  1154030
Runner:    GE-Proton 11.3
```

Game installation:

```text
/mnt/data-z/SteamLibrary/steamapps/common/Titan Quest II/
```

Proton compatdata:

```text
/mnt/data-z/SteamLibrary/steamapps/compatdata/1154030/
```

Observed game process:

```text
TQ2-Win64-Shipping.exe
```

A Linux process listing may present the command line similarly to:

```text
.../Titan Quest II/TQ2/Binaries/Win64/TQ2-Win64-Shipping.exe TQ2
```

The trainer must **discover the running process dynamically**. Never depend on a fixed PID.

---

## 3. Core Goal

Produce a robust native Rust CLI that:

1. Finds the running Titan Quest II process.
2. Locates the relevant executable memory mapping(s).
3. Identifies the EXP-award logic using a stable signature/pattern rather than an absolute address.
4. Validates expected original bytes/instructions before modifying anything.
5. Applies a configurable EXP multiplier.
6. Restores the original behavior cleanly.
7. Refuses to patch unsupported game builds.
8. Never writes arbitrary memory when validation fails.

Supported user-facing multipliers for v1:

```text
1x
2x
3x
5x
10x
```

The architecture should allow arbitrary positive multipliers later if the discovered patch supports it safely.

---

## 4. Non-Goals

Do **not** turn this into a general cheat engine.

For the initial project, do not implement:

- god mode
- infinite health
- infinite mana
- infinite gold
- speed hacks
- teleportation
- item spawning
- save editing
- DLL injection
- LD_PRELOAD injection
- kernel modules
- anti-cheat bypasses
- process hiding
- stealth techniques
- multiplayer cheating
- automatic patching of unknown game versions

The first release should do **one thing very well: EXP multiplier**.

---

## 5. Preferred Architecture

Use a simple modular Rust architecture.

Suggested repository structure:

```text
tq2-trainer/
├── Cargo.toml
├── README.md
├── LICENSE
├── src/
│   ├── main.rs
│   ├── cli.rs
│   ├── process.rs
│   ├── maps.rs
│   ├── memory.rs
│   ├── scanner.rs
│   ├── patch.rs
│   ├── exp.rs
│   ├── build.rs
│   └── error.rs
├── signatures/
│   └── tq2.toml
├── tools/
│   └── README.md
└── tests/
    ├── pattern_scan.rs
    └── patch_validation.rs
```

Keep dependencies minimal.

Possible dependencies:

```toml
[dependencies]
anyhow = "1"
clap = { version = "4", features = ["derive"] }
libc = "0.2"
thiserror = "2"
serde = { version = "1", features = ["derive"] }
toml = "0.9"
```

Do not add a GUI in the first implementation.

---

## 6. CLI Design

Target command structure:

```bash
tq2-trainer status
tq2-trainer xp 2
tq2-trainer xp 3
tq2-trainer xp 5
tq2-trainer xp 10
tq2-trainer xp 1
tq2-trainer restore
tq2-trainer scan
```

Example:

```text
$ tq2-trainer status

Titan Quest II
--------------
Process: TQ2-Win64-Shipping.exe
PID: 277197
Build: supported
EXP patch: found
Current multiplier: 1x
```

Applying:

```text
$ tq2-trainer xp 5

Titan Quest II found
PID: 277197

Scanning executable memory...
EXP signature found at 0x7f1234567890
Original bytes verified

EXP multiplier: 1x -> 5x

Done.
```

Unsupported game build:

```text
$ tq2-trainer xp 5

Titan Quest II found
PID: 277197

ERROR: Known EXP signature was not found.

This Titan Quest II build is currently unsupported.
No memory was modified.
```

Validation failure:

```text
ERROR: Patch location was found, but current bytes do not match
the expected original or known patched states.

Refusing to write memory.
```

---

## 7. Process Discovery

Implement process discovery using `/proc`.

Search:

```text
/proc/<pid>/cmdline
/proc/<pid>/comm
/proc/<pid>/exe
```

Preferred matching strategy:

1. enumerate numeric directories under `/proc`
2. inspect process command line
3. locate a process containing:

```text
TQ2-Win64-Shipping.exe
```

4. optionally verify that its executable/path belongs to the Titan Quest II installation
5. if exactly one valid process exists, attach to it
6. if zero are found, report that the game is not running
7. if multiple candidates are found, refuse to guess and show the candidates

Do not identify the process solely by a partial generic string such as `TQ2`.

---

## 8. Memory Map Discovery

Parse:

```text
/proc/<pid>/maps
```

Represent mappings with fields similar to:

```rust
struct MemoryRegion {
    start: usize,
    end: usize,
    readable: bool,
    writable: bool,
    executable: bool,
    private: bool,
    offset: u64,
    pathname: Option<PathBuf>,
}
```

The scanner should initially prioritize executable mappings associated with:

```text
TQ2-Win64-Shipping.exe
```

Do not scan every mapped allocation in the process unless required.

Useful categories:

```text
r-xp  executable code
rw-p  writable data
r--p  read-only data
```

The ultimate scan range depends on what the EXP investigation discovers.

---

## 9. Memory Access

Preferred Linux APIs:

```text
process_vm_readv()
process_vm_writev()
```

Wrap these behind a safe Rust abstraction.

Example conceptual interface:

```rust
trait ProcessMemory {
    fn read(&self, address: usize, buf: &mut [u8]) -> Result<()>;
    fn write(&self, address: usize, data: &[u8]) -> Result<()>;
}
```

The rest of the project should not directly call libc memory functions.

Before every write:

1. verify target process still exists
2. verify the relevant mapping still exists
3. read the current target bytes
4. compare against expected values
5. only then write

After every write:

1. read the location back
2. verify the bytes/value match what was requested
3. report failure if verification does not match

---

## 10. Linux ptrace / Permission Handling

Do not automatically weaken system security settings.

`process_vm_readv/writev` may fail with `EPERM` depending on the system's ptrace/Yama configuration.

Detect this explicitly and print a useful message.

Possible development options may include:

```text
CAP_SYS_PTRACE
```

or a temporary developer-controlled ptrace configuration, but the application itself must **not silently modify**:

```text
kernel.yama.ptrace_scope
```

The tool should first attempt normal same-user access.

If permission is denied:

```text
ERROR: Linux denied access to the game process.

The trainer requires permission to inspect/modify the process.
No memory was changed.
```

Keep privilege handling isolated so the trainer can later support a clean documented setup rather than simply requiring the whole application to run as root.

---

## 11. Signature Scanner

Never rely on an absolute address.

ASLR makes this invalid:

```text
0x7FF6A1234567
```

Instead implement wildcard-capable byte signatures.

Example syntax:

```text
48 8B ?? ?? ?? ?? ?? 48 85 C0 74 ?? F3 0F 10
```

Representation could be:

```rust
enum PatternByte {
    Exact(u8),
    Wildcard,
}
```

Implement:

```rust
fn scan_pattern(haystack: &[u8], pattern: &[PatternByte]) -> Vec<usize>
```

Requirements:

- wildcard support
- bounds-safe scanning
- zero matches => unsupported
- one match => acceptable
- multiple matches => refuse unless additional validation resolves ambiguity

The scanner should support an additional verification stage around the match.

---

## 12. Signature Database

Keep game-version-specific details outside core logic when practical.

Suggested:

```text
signatures/tq2.toml
```

Conceptual format:

```toml
[[exp_patch]]
id = "tq2-ea-2026-06"
description = "Titan Quest II Early Access build"

pattern = "AA BB ?? CC DD"
patch_offset = 12

expected = "11 22 33 44"
```

After reverse engineering the real patch, extend the structure with the information actually needed.

Possible fields:

```text
module
pattern
pattern_offset
expected_original_bytes
known_patched_bytes
patch_kind
build_identifier
```

Do not invent signatures or patch bytes.

Only commit them after validating against a real game build.

---

# 13. Most Important Phase: Discover How EXP Works

Do **not** begin by guessing machine-code patches.

The first engineering milestone is **research/discovery**.

We already know an external FLiNG trainer was able to apply a working `5x EXP` multiplier in-game, so use that as a behavioral reference where useful.

The discovery process should be reproducible and documented.

---

## 14. Discovery Strategy A — Find the EXP Value

Start with the easiest observable quantity: the player's current EXP value.

Use a Linux-compatible memory scanning tool during research, for example:

```text
scanmem
```

A GUI such as GameConqueror may optionally be used for manual investigation, but the final trainer must not depend on it.

Workflow:

```text
1. Start Titan Quest II.
2. Load a single-player character.
3. Record exact displayed EXP.
4. Scan process memory for that value.
5. Gain EXP.
6. Record the new value.
7. Filter previous results.
8. Repeat until the likely player EXP address is isolated.
```

Determine:

- integer width: 32-bit / 64-bit
- signed or unsigned
- whether displayed EXP is total EXP or level-relative EXP
- whether multiple mirrored/cached values exist
- whether address changes between launches
- what pointer or code references the value

Do not stop at finding a dynamic address. A fixed address in one session is not enough for the final trainer.

---

## 15. Discovery Strategy B — Find What FLiNG Changes

If the FLiNG trainer can be made stable enough for a short research session, use it as a differential reference.

Goal:

```text
Trainer OFF -> snapshot
Trainer 5x  -> snapshot
Compare
```

Focus primarily on executable mappings belonging to the game.

Possible FLiNG behaviors include:

1. changing a global multiplier value
2. changing a floating-point constant
3. replacing an arithmetic instruction
4. patching a function with a jump/code cave
5. hooking the EXP-award function
6. intercepting an XP addition routine

Do not assume which method is used.

A naïve whole-process byte diff will produce huge noise. Prefer targeted investigation around:

- instructions that write the EXP value
- callers of the XP-award routine
- memory regions changed immediately when enabling/disabling the option

If FLiNG modifies executable code, identify:

```text
original bytes
patched bytes
address relative to module base
surrounding instructions
stable signature surrounding the patch
```

Document everything discovered.

---

## 16. Discovery Strategy C — Trace Writes to EXP

Once the live XP address is known, determine what code writes to it.

The goal is to locate logic conceptually similar to:

```text
old_exp + gained_exp -> new_exp
```

or:

```text
reward_exp * multiplier -> awarded_exp
```

Use an appropriate debugger/research tool to identify write instructions.

Possible tools:

```text
gdb
rr
scanmem
objdump
radare2
Cutter
Ghidra
```

Prefer tools that work reliably against the Proton-hosted game process.

The final application must not depend on heavyweight reverse-engineering tools.

---

## 17. Preferred Final EXP Implementation

Best-case implementation:

```text
Game computes XP reward
        |
        v
Known EXP award routine
        |
        v
Apply our multiplier
        |
        v
Game adds XP
```

If a native game value already controls the multiplier, change that value.

If a tiny instruction patch can safely modify the calculation, use that.

Avoid continuously polling and rewriting the player's XP if a clean award-function patch is available.

Preference order:

```text
1. Existing multiplier/value
2. Small validated instruction/data patch
3. Controlled function patch
4. XP delta watcher only as fallback
```

---

## 18. Fallback: XP Delta Watcher

If no clean multiplier patch is practical, a fallback implementation may watch the player XP value.

Concept:

```text
old = 15000
game changes XP to 15100
delta = 100

requested multiplier = 5x

additional = delta * (5 - 1)
           = 400

write 15500
```

However, this is inherently less reliable.

Potential edge cases:

- quest XP
- combat XP
- loading saves
- level transitions
- XP loss/reset mechanics
- large scripted rewards
- multiple XP writes per frame
- mirrored values
- race conditions

Do not use this method unless it is demonstrated to be safe enough.

---

## 19. Patch Abstraction

Create a generic reversible patch abstraction.

Concept:

```rust
struct MemoryPatch {
    address: usize,
    original: Vec<u8>,
    replacement: Vec<u8>,
}
```

Operations:

```rust
impl MemoryPatch {
    fn verify_original(&self, process: &Process) -> Result<bool>;
    fn apply(&self, process: &Process) -> Result<()>;
    fn restore(&self, process: &Process) -> Result<()>;
}
```

Support known states:

```text
ORIGINAL
PATCHED
UNKNOWN
```

If state is `UNKNOWN`, do not write anything.

---

## 20. Multiplier Representation

Do not assume the final patch requires separate machine code for every multiplier.

The reverse-engineering phase should determine whether the multiplier can be represented as:

```text
integer
float
double
immediate instruction operand
data variable
```

The final design should expose:

```rust
struct ExpMultiplier(u32);
```

with validation:

```text
minimum: 1
initial maximum: 10
```

If the underlying mechanism safely supports arbitrary values later, the CLI can evolve to:

```bash
tq2-trainer xp 7
```

For v1, prioritize tested presets.

---

## 21. Build Identification

Titan Quest II is in active development, so updates can invalidate signatures.

The trainer must detect whether it understands the running build.

Possible identifiers:

- executable file size
- SHA-256 of the PE executable
- PE timestamp
- module `.text` hash
- Steam build metadata if easily available
- signature-set identity

Prefer something deterministic.

Example:

```text
Titan Quest II build
Executable SHA-256: ...
Signature profile: tq2-ea-2026-08-xx
Status: supported
```

Do not require an exact whole-file hash if harmless Steam/packaging changes make that too brittle. A `.text` section hash may eventually be more useful.

---

## 22. PE Awareness

Although the trainer runs on Linux, the target is a Windows PE executable running through Proton.

It may be useful to implement minimal PE parsing or use a small Rust PE parsing library to identify:

```text
.text
.rdata
.data
image base
section RVA
section sizes
```

This can make scanning substantially more precise than scanning all executable mappings.

Do not build a full PE loader.

---

## 23. Handling ASLR

Never assume module base addresses are stable.

At runtime:

```text
module_base = discover from /proc/<pid>/maps
match_offset = scan module memory
target = module_base + calculated offset
```

The exact calculation depends on the mapping and PE layout.

Validate it empirically.

---

## 24. Failure Safety

This project should be intentionally conservative.

Before every modification:

```text
Process exists?                    YES
Correct process?                   YES
Expected module found?             YES
Supported signature/build?         YES
Exactly one patch site?            YES
Expected original/current bytes?   YES
```

Only then:

```text
WRITE
```

If any answer is NO:

```text
ABORT
```

Never "try anyway".

---

## 25. Crash Safety / Restoration

The trainer should not need to remain running if the final implementation is a static patch.

If practical:

```text
tq2-trainer xp 5
```

should:

1. attach
2. patch
3. verify
4. exit

Then Titan Quest II continues with 5x enabled.

Restoration:

```text
tq2-trainer restore
```

should restore normal behavior.

If the patch requires allocated code or an active watcher, document why and keep lifetime management explicit.

---

## 26. Optional Ctrl+C Restoration

If the selected implementation requires the trainer to remain active, handle:

```text
SIGINT
SIGTERM
```

and attempt to restore original state before exiting.

Never promise restoration after:

```text
SIGKILL
power loss
game crash
```

Design the patch so the game closing naturally makes any modified process memory disappear.

---

## 27. Logging

Keep default output readable.

Normal:

```text
INFO  Found Titan Quest II (PID 277197)
INFO  Build profile: tq2-ea-2026-08
INFO  EXP patch found
INFO  Applied 5x multiplier
```

Verbose mode:

```bash
tq2-trainer -v xp 5
```

may include:

```text
module base
mapping ranges
signature address
patch offset
current bytes
expected bytes
written bytes
```

Never dump huge memory regions by default.

---

## 28. Testing

Memory-writing logic must be separated enough to test without running Titan Quest II.

### Unit tests

Test:

```text
pattern parser
wildcards
pattern scanner
zero matches
one match
multiple matches
bounds behavior
patch state detection
multiplier validation
TOML signature parsing
```

Example scanner fixture:

```rust
let memory = [
    0x48, 0x8B, 0x12, 0x34,
    0x90, 0x90,
];

pattern:
48 8B ?? ??
```

Expected:

```text
match at offset 0
```

### Mock process memory

Create an in-memory backend implementing the same abstraction as the real process-memory backend.

This allows tests for:

```text
apply
verify
restore
refuse unexpected bytes
```

without touching another process.

---

## 29. Development Modes

Consider a compile-time or runtime dry-run mode:

```bash
tq2-trainer --dry-run xp 5
```

Output:

```text
Would patch:
PID: 277197
Address: 0x...
Current: ...
Expected: ...
Replacement: ...

DRY RUN: no memory was modified.
```

This will be extremely useful while developing signatures.

---

## 30. Research Tooling

A separate `tools/` area may contain one-off developer utilities.

Examples:

```text
memory-dump
module-dump
signature-test
memory-diff
```

These are development/research tools, not necessarily part of the shipped CLI.

Example useful utility:

```bash
cargo run --bin tq2-dump -- --module text --output before.bin
```

Enable FLiNG 5x, then:

```bash
cargo run --bin tq2-dump -- --module text --output after.bin
```

Then:

```bash
cargo run --bin tq2-diff -- before.bin after.bin
```

But ensure the comparison method accounts for ordinary runtime changes; code sections should generally be much cleaner targets than arbitrary writable memory.

---

## 31. Avoid Depending on FLiNG

FLiNG may be used temporarily as a research reference because its EXP multiplier was observed working.

The finished trainer must not:

- launch FLiNG
- inspect FLiNG every time
- require FLiNG installed
- copy proprietary trainer code
- redistribute FLiNG assets
- embed extracted FLiNG executable code

We only want to independently determine how Titan Quest II's own EXP behavior can be safely adjusted.

---

## 32. README Requirements

Document:

### Build

```bash
cargo build --release
```

### Run

```bash
./target/release/tq2-trainer status
./target/release/tq2-trainer xp 5
./target/release/tq2-trainer restore
```

### Requirements

```text
Linux
Steam/Proton Titan Quest II
supported game build
permission to inspect/write the user's own game process
```

### Disclaimer

State clearly:

```text
For personal single-player/offline use.
Game updates may make signatures unsupported.
The tool refuses to patch unknown builds.
Back up important saves.
```

---

# 33. Implementation Phases

## Phase 0 — Repository Bootstrap

Deliver:

```text
Cargo project
CLI skeleton
error types
logging
README
```

Commands may initially be stubs.

Acceptance:

```bash
cargo build
cargo test
```

both succeed.

---

## Phase 1 — Process Detection

Implement:

```text
/proc enumeration
Titan Quest II detection
PID reporting
process disappearance handling
```

Acceptance:

```bash
tq2-trainer status
```

finds the real running game and does not confuse Wine/Steam helper processes with the target.

---

## Phase 2 — Memory Map Parsing

Implement:

```text
/proc/<pid>/maps parser
module mapping detection
readable/executable region selection
```

Acceptance:

```bash
tq2-trainer scan
```

can show the relevant game mappings without modifying memory.

---

## Phase 3 — Read-Only Memory Access

Implement:

```text
process_vm_readv
safe wrapper
errors
permission detection
```

No write support required yet.

Acceptance:

trainer can read a small known region from the game process reliably.

---

## Phase 4 — Research EXP

This is the major manual/reverse-engineering phase.

Determine:

```text
XP data representation
XP write path
EXP reward function
FLiNG 5x behavior if useful
stable patch/signature candidate
```

Create a research note:

```text
docs/EXP-RESEARCH.md
```

It must contain:

```text
game build tested
tools used
observations
addresses relative to module
relevant disassembly
original bytes
patched/value behavior
chosen signature
why the signature is considered stable
```

Do not move to automatic writing until this is understood.

---

## Phase 5 — Scanner + Build Profile

Implement:

```text
signature parser
scanner
profile loading
match validation
supported-build reporting
```

Acceptance:

the trainer finds exactly one validated EXP location on the tested game build.

---

## Phase 6 — Write Backend

Add:

```text
process_vm_writev
read-before-write
write
read-after-write
verification
```

Keep write operations disabled from user commands until patch validation tests pass.

---

## Phase 7 — EXP 5x Proof of Concept

Implement only:

```bash
tq2-trainer xp 5
tq2-trainer restore
```

Acceptance:

1. Start game.
2. Note XP.
3. Enable 5x.
4. Earn a known XP reward.
5. Confirm reward is multiplied correctly.
6. Restore.
7. Confirm normal XP behavior returns.

Do not add additional multipliers until 5x is reliable.

---

## Phase 8 — Preset Multipliers

Add:

```text
1x
2x
3x
5x
10x
```

Verify each multiplier empirically.

---

## Phase 9 — Hardening

Add:

```text
unsupported build handling
duplicate signature handling
unexpected byte handling
dry-run
verbose diagnostics
tests
clean documentation
```

---

## Phase 10 — Optional Packaging

Possible later outputs:

```text
Arch PKGBUILD
AUR package
single release binary
desktop launcher
```

Do not prioritize packaging before the core patch is proven.

---

# 34. Definition of Done — v1

The first real release is complete when all of these are true:

- [ ] Rust CLI builds cleanly on CachyOS.
- [ ] Game PID is detected automatically.
- [ ] No fixed process PID is used.
- [ ] No absolute game-memory address is hardcoded.
- [ ] EXP patch is located through a validated signature/profile.
- [ ] Unsupported builds fail safely.
- [ ] `xp 5` reliably produces 5x EXP.
- [ ] `xp 1` or `restore` restores normal EXP.
- [ ] 2x, 3x, 5x, and 10x are tested.
- [ ] Current bytes are validated before every write.
- [ ] Writes are verified after every modification.
- [ ] No FLiNG, .NET, Wine trainer process, or GUI is needed.
- [ ] No anti-cheat bypass functionality exists.
- [ ] Tool is documented as single-player/offline only.
- [ ] `cargo test` passes.
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` passes.
- [ ] `cargo fmt --check` passes.
- [ ] `docs/EXP-RESEARCH.md` explains the discovered EXP mechanism.

---

# 35. Engineering Principles

Priorities, in order:

```text
1. Safety
2. Correctness
3. Build compatibility validation
4. Reliability
5. Simplicity
6. Performance
7. Convenience
```

This trainer performs tiny amounts of memory I/O, so performance optimization is not important.

Prefer obvious, auditable code.

Avoid clever unsafe Rust.

Where `unsafe` is unavoidable for libc calls:

- keep it in a very small module
- document invariants
- validate sizes and addresses
- expose a safe API to the rest of the program

---

# 36. Agent Instructions

When implementing this project:

1. **Do not invent the EXP address, signature, data type, or patch bytes.**
2. Build the read-only tooling first.
3. Treat the reverse-engineering phase as an explicit milestone.
4. Document empirical observations.
5. Ask for a user-side test when live game behavior must be observed.
6. Never write to memory until the candidate location and original state are validated.
7. Do not broaden scope beyond EXP multiplier without explicit approval.
8. Avoid dependencies unless they meaningfully reduce complexity.
9. Keep the trainer native to Linux.
10. Keep the game itself running normally through Steam + Proton.
11. Do not modify the Titan Quest II files on disk unless a later design explicitly requires it.
12. Prefer reversible in-memory changes.
13. Never add anti-cheat bypass, evasion, stealth, or multiplayer functionality.

---

# 37. Immediate First Task

Start with **Phase 0 through Phase 3 only**.

The first development iteration should produce a Rust program capable of:

```text
$ tq2-trainer status
Titan Quest II found
PID: <pid>
Executable/module mappings: <count>
Memory access: OK
```

and:

```text
$ tq2-trainer scan
```

should print a concise summary of the relevant `TQ2-Win64-Shipping.exe` memory mappings.

At this point:

```text
NO MEMORY WRITES.
NO EXP PATCH.
NO GUESSED SIGNATURES.
```

Once the read-only foundation works, begin the controlled EXP discovery phase and create:

```text
docs/EXP-RESEARCH.md
```

before implementing the actual trainer patch.

---

## Final Vision

The desired end-user experience should eventually be this simple:

```bash
$ tq2-trainer xp 5

Titan Quest II found (PID 277197)
Supported build detected
EXP multiplier: 1x -> 5x
Done.
```

No FLiNG trainer.

No .NET runtime.

No Protontricks invocation.

No second Wine GUI.

No launcher crashes.

Just a small, native Rust utility that safely modifies the user's local single-player Titan Quest II process.
