# ADR 0009: Schema: generic media items

Status: accepted (Phase 0)

## Context

Music is first, but podcasts, audiobooks and video are planned.

## Decision

A `media_items` table with a `kind` column rather than a tracks-only schema. Placeholders (songs not owned) are first-class and playlist entries and likes can point at either, which is what makes Spotify imports lossless.

## Consequences

Some music-specific queries filter on `kind`. No migration is needed to add new kinds.
