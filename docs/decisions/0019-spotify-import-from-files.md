# 0019 Spotify import starts from files, not the Spotify API

Status: accepted (2026-10-19)

## Context

Phase 3 promises that moving from Spotify takes minutes. There are two ways to get a user's playlists out of Spotify: files the user downloads (the account data download, or CSV exports made by tools such as Exportify), or the Spotify Web API, which needs a registered developer app, sign-in, and Spotify's approval to serve more than a small allow-listed set of users. We could not verify current API quotas for an open-source app without registering one.

## Decision

The first importer reads files only:

- the account data download: `Playlist*.json` (playlists) and `YourLibrary.json` (liked songs);
- CSV playlist exports, one playlist per file, recognised by their column names.

Nothing is sent to Spotify or anywhere else. Podcast episodes and audiobooks are left out and counted.

Each song is matched to a library track by ISRC, then by normalised title plus a shared artist, then by title without decorations ("(feat. X)", "- Remastered 2011"). Matching is strict, because a wrong match plays the wrong song while a miss is visible. Different recordings (live, instrumental, acoustic, remix…) must agree, and lengths must be within 15 s when both are known. Untagged files named "NN - Artist - Title" are matched by name.

Songs not found become placeholders (schema 0009), so playlists keep their full shape. Every scan re-matches open placeholders, so playlists fill in as music is added. Re-importing the same export updates playlists in place (`playlists.source_key`) and reuses placeholders by Spotify URI.

## Consequences

- No API keys, no account, works offline, and nothing for Spotify to revoke.
- The account data download takes Spotify days to prepare; CSV exports are immediate and carry ISRCs and durations, so they match better.
- The data download has no ISRCs or durations, so its matches rely on names.
- Pasting a playlist link still needs the Web API; that is a separate decision for later.
