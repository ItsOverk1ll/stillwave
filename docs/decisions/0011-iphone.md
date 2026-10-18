# ADR 0011: iPhone distribution: unsigned ipa for SideStore

Status: accepted (Phase 0)

## Context

The project cannot assume a paid Apple developer account.

## Decision

Ship an unsigned `.ipa`; users sign it with their own Apple ID through SideStore. See docs/ios-unsigned-builds.md (the build approach is a spike, not yet proven).

## Consequences

Free-account signing expires every 7 days (SideStore refreshes it). No App Store reach.
