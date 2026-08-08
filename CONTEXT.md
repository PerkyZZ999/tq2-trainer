# Domain glossary

Vocabulary for TQ2-trainer modules. Architecture terms follow deep-module design: **module**, **interface**, **seam**, **adapter**, **depth**, **leverage**, **locality**.

| Term | Meaning |
|------|---------|
| **Live patch** | Temporary code rewrite in the game process (EXP trampoline, sell-gold payout). Applied and restored through one deep interface. |
| **Build profile** | Fingerprint + RVAs + prologues for a supported Shipping.exe (SHA-256 + site layout), loaded from `signatures/tq2.toml`. |
| **Balance watch** | Wait until gold (or similar) memory mirrors transition `before → after`, then signal completion (`src/balance_watch.rs`). |
| **Research session** | Collaborative snap / narrow / list / probe workflow (`src/research_session.rs`). |
| **Shipping.exe** | `TQ2-Win64-Shipping.exe` under Proton — the only process we attach to. |
