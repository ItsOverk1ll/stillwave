# ADR 0015: Plugin manifest format: TOML

Status: accepted (Phase 0)

## Context

Phase 0 had to pick TOML or JSON for the manifest.

## Decision

TOML. It is written by hand, allows comments, and is familiar from Cargo. The signature covers the exact manifest bytes, so the format has no security impact. Unknown fields and permissions are rejected.

## Consequences

Authors need a TOML parser only at build time; the app uses the `toml` crate.
