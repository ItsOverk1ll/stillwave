# ADR 0014: Server (later): Cloudflare Workers, D1, Durable Objects, R2

Status: accepted (Phase 0)

## Context

The optional account and sync service should be cheap to run and scale to zero.

## Decision

Build the account server on Cloudflare Workers with D1, Durable Objects and R2, AGPL-3.0. `server/` is a placeholder until Phase 4; synced data is end-to-end encrypted.

## Consequences

Vendor lock-in to Cloudflare for the hosted service; self-hosting would need a different runtime.
