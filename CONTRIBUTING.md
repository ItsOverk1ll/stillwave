# Contributing to Stillwave

Thanks for helping. Read [CLAUDE.md](CLAUDE.md) too: it is the short version of how this project works (it is written for an AI assistant, but it applies to everyone).

## Hard rules

These are not up for debate in a pull request:

- **Nothing from userfy.** It is closed source and only an inspiration. Do not copy, decompile or imitate its code, assets, names or design.
- **No stream ripping.** No feature may save audio from streaming services (Spotify, Tidal, YouTube, ...) or download from file-sharing networks. Songs a user does not own come from official streaming SDKs (played, never saved), free legal sources (Jamendo, Internet Archive) or stores.
- **No DRM circumvention**, including in plugins, examples, tests and scripts.
- **Never commit personal data.** Spotify exports, real libraries and test playlists go in `fixtures/private/` (git-ignored).
- **Never commit private keys.** `keys/`, `*.key` and `.env*` are git-ignored; keep it that way.
- **Never mutate a user's audio files.** Tag writing is opt-in and metadata-only.

## Principles

Sound quality, then speed, then reliability. Speed never comes from touching the audio path. Heavy work (scanning, decoding, database queries) never runs on the UI thread. See the README for the full list.

Ask before adding a heavy dependency, changing a decision in [docs/decisions/](docs/decisions/), or deviating from the architecture. Every new decision gets a short ADR there.

## Checks

Run these before opening a pull request (CI runs the same):

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo clippy -p stillwave-plugins --no-default-features --all-targets -- -D warnings   # iOS configuration

cd apps/stillwave
pnpm format:check && pnpm lint && pnpm typecheck && pnpm test

cd ../../plugin-sdk
cargo test && cargo clippy --target wasm32-unknown-unknown --all-targets -- -D warnings
```

The 50,000-track search benchmark is `cargo bench -p stillwave-db`; record notable changes in `docs/benchmarks.md`.

## Code conventions

- Rust: no `unwrap()` or `expect()` outside tests on code paths that handle user data or files. Clippy pedantic is on.
- TypeScript: strict mode, ESLint with type-aware rules, Prettier.
- Every source file starts with an SPDX header (`GPL-3.0-only`, or `MIT` in `plugin-sdk/`).
- Database changes are new migrations in `crates/stillwave-db/migrations/`. Never edit a released migration. Every user-data write also writes a `change_log` row in the same transaction.
- Small, focused commits with clear messages. Keep `main` building on every platform.
- When unsure how a platform behaves (iOS signing, Android audio, WebView DRM), write a small spike, record the finding in `docs/`, then build. Say "I couldn't verify X" rather than guessing.

## Your email address

Use a GitHub noreply address for commits (`git config user.email "<id>+<name>@users.noreply.github.com"`) if you do not want your real address in the public history.
