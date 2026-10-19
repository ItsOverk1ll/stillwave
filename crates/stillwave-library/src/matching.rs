// SPDX-License-Identifier: GPL-3.0-only
//! Finds the library track for a song named by an import (title, artist, album).
//!
//! Matching is deliberately strict: a wrong match plays the wrong song, while a
//! miss only leaves a placeholder the user can see. So versions that are different
//! recordings (live, instrumental, acoustic, remix, demo…) must agree, and when both
//! durations are known they must be close. Tiers, best first:
//! 1. same ISRC;
//! 2. same normalised title and a shared artist;
//! 3. same title once decorations like "(feat. X)" or "- Remastered 2011" are
//!    removed, and a shared artist.

use std::collections::{BTreeSet, HashMap};

use stillwave_core::MediaItemId;
use stillwave_db::MatchCandidate;
use unicode_normalization::UnicodeNormalization;

/// Words that mark a different recording; both sides must have the same set.
const VARIANT_WORDS: &[&str] = &[
    "live",
    "instrumental",
    "acoustic",
    "remix",
    "demo",
    "karaoke",
    "unplugged",
    "orchestral",
    "piano",
    "cover",
    "slowed",
    "sped",
    "nightcore",
];

/// Durations further apart than this are different recordings.
const MAX_DURATION_DIFF_MS: u64 = 15_000;

/// What an import knows about one song.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query<'a> {
    pub title: &'a str,
    pub artist: &'a str,
    pub album: Option<&'a str>,
    pub duration_ms: Option<u64>,
    pub isrc: Option<&'a str>,
}

struct Entry {
    item: MediaItemId,
    full: String,
    variants: BTreeSet<&'static str>,
    artists: Artists,
    album: Option<String>,
    duration_ms: Option<u64>,
}

/// An index over the library, built once per import.
pub struct Matcher {
    entries: Vec<Entry>,
    by_bare: HashMap<String, Vec<usize>>,
    by_isrc: HashMap<String, MediaItemId>,
}

impl Matcher {
    #[must_use]
    pub fn new(candidates: Vec<MatchCandidate>) -> Self {
        let mut m = Self {
            entries: Vec::with_capacity(candidates.len()),
            by_bare: HashMap::new(),
            by_isrc: HashMap::new(),
        };
        for c in candidates {
            if let Some(isrc) = c.isrc.as_deref().map(normalise_isrc) {
                if !isrc.is_empty() {
                    m.by_isrc.entry(isrc).or_insert(c.item_id);
                }
            }
            // Untagged files often carry "NN - Artist - Title" in the name.
            let (title, artist) = match c.artist.as_deref() {
                Some(a) if !a.trim().is_empty() => (c.title.clone(), a.to_owned()),
                _ => match split_filename_title(&c.title) {
                    Some((a, t)) => (t, a),
                    None => continue,
                },
            };
            let idx = m.entries.len();
            m.by_bare.entry(bare(&title)).or_default().push(idx);
            m.entries.push(Entry {
                item: c.item_id,
                full: normalise(&title),
                variants: variants(&title),
                artists: Artists::parse(&artist),
                album: c.album.as_deref().map(normalise),
                duration_ms: c.duration_ms,
            });
        }
        m
    }

    /// The best library item for `q`, or `None` when nothing is a safe match.
    #[must_use]
    #[allow(clippy::many_single_char_names)] // q(uery), e(ntry), i(ndex): short and local
    pub fn find(&self, q: &Query) -> Option<MediaItemId> {
        if let Some(isrc) = q.isrc.map(normalise_isrc) {
            if let Some(id) = self.by_isrc.get(&isrc) {
                return Some(*id);
            }
        }
        let list = self.by_bare.get(&bare(q.title))?;
        let artists = Artists::parse(q.artist);
        let full = normalise(q.title);
        let wanted_variants = variants(q.title);
        let album = q.album.map(normalise);

        let mut best: Option<(u64, MediaItemId)> = None;
        for &i in list {
            let e = &self.entries[i];
            if e.variants != wanted_variants || !e.artists.overlaps(&artists) {
                continue;
            }
            let diff = match (e.duration_ms, q.duration_ms) {
                (Some(a), Some(b)) => {
                    let d = a.abs_diff(b);
                    if d > MAX_DURATION_DIFF_MS {
                        continue;
                    }
                    d
                }
                _ => MAX_DURATION_DIFF_MS,
            };
            // Lower is better: exact title, then same album, then closer length.
            let score = u64::from(e.full != full) * 1_000_000
                + u64::from(album.is_some() && e.album != album) * 100_000
                + diff;
            if best.is_none_or(|(s, _)| score < s) {
                best = Some((score, e.item));
            }
        }
        best.map(|(_, id)| id)
    }
}

/// Lowercase, accents removed, `&` as "and", punctuation as spaces, single spaces.
#[must_use]
pub fn normalise(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = true;
    for c in s
        .nfkd()
        .filter(|c| !is_combining(*c))
        .flat_map(char::to_lowercase)
    {
        if c == '&' {
            if !space {
                out.push(' ');
            }
            out.push_str("and ");
            space = true;
        } else if c.is_alphanumeric() {
            out.push(c);
            space = false;
        } else if c == '\'' || c == '’' {
            // "don't" and "dont" are the same word
        } else if !space {
            out.push(' ');
            space = true;
        }
    }
    out.trim_end().to_owned()
}

fn is_combining(c: char) -> bool {
    matches!(c as u32, 0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F)
}

fn normalise_isrc(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// The title without bracketed parts and without a " - …" suffix:
/// "Song (feat. X) - Remastered 2011" → "song".
#[must_use]
pub fn bare(title: &str) -> String {
    let mut kept = String::with_capacity(title.len());
    let mut depth = 0u32;
    for c in title.chars() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            _ if depth == 0 => kept.push(c),
            _ => {}
        }
    }
    let head = kept.split(" - ").next().unwrap_or(&kept);
    // "Song feat. X" without brackets.
    let lower = head.to_lowercase();
    let cut = [" feat. ", " feat ", " ft. ", " ft ", " featuring "]
        .iter()
        .filter_map(|m| lower.find(m))
        .min()
        .unwrap_or(head.len());
    let b = normalise(head.get(..cut).unwrap_or(head));
    if b.is_empty() {
        normalise(title)
    } else {
        b
    }
}

fn variants(title: &str) -> BTreeSet<&'static str> {
    let n = normalise(title);
    let words: BTreeSet<&str> = n.split(' ').collect();
    VARIANT_WORDS
        .iter()
        .copied()
        .filter(|w| words.contains(w) && !bare(title).split(' ').any(|b| b == *w))
        .collect()
}

/// "12 - Nickelback - San Quentin" → ("Nickelback", "San Quentin").
fn split_filename_title(name: &str) -> Option<(String, String)> {
    let rest = name.trim_start_matches(|c: char| c.is_ascii_digit());
    let rest = if rest.len() < name.len() {
        rest.trim_start_matches([' ', '-', '.', '_'])
    } else {
        name
    };
    let (artist, title) = rest.split_once(" - ")?;
    let (artist, title) = (artist.trim(), title.trim());
    (!artist.is_empty() && !title.is_empty()).then(|| (artist.to_owned(), title.to_owned()))
}

/// The people credited, from strings like "A feat. B", "A, B", "A; B", "A & B".
struct Artists {
    whole: String,
    names: BTreeSet<String>,
}

impl Artists {
    fn parse(s: &str) -> Self {
        let lower = s.to_lowercase();
        let mut parts = vec![lower.clone()];
        for sep in [
            " featuring ",
            " feat. ",
            " feat ",
            " ft. ",
            " ft ",
            " with ",
            " x ",
            " vs. ",
            " vs ",
            ",",
            ";",
            "&",
            "/",
            " and ",
        ] {
            parts = parts
                .iter()
                .flat_map(|p| p.split(sep).map(str::to_owned).collect::<Vec<_>>())
                .collect();
        }
        Self {
            whole: strip_the(&normalise(&lower)),
            names: parts
                .iter()
                .map(|p| strip_the(&normalise(p)))
                .filter(|p| !p.is_empty())
                .collect(),
        }
    }

    fn overlaps(&self, other: &Self) -> bool {
        (!self.whole.is_empty() && self.whole == other.whole)
            || self.names.iter().any(|n| other.names.contains(n))
    }
}

fn strip_the(s: &str) -> String {
    s.strip_prefix("the ").unwrap_or(s).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(
        title: &str,
        artist: Option<&str>,
        album: Option<&str>,
        dur: Option<u64>,
    ) -> MatchCandidate {
        MatchCandidate {
            item_id: MediaItemId::new(),
            title: title.into(),
            artist: artist.map(String::from),
            album: album.map(String::from),
            duration_ms: dur,
            isrc: None,
        }
    }

    fn q<'a>(title: &'a str, artist: &'a str) -> Query<'a> {
        Query {
            title,
            artist,
            ..Query::default()
        }
    }

    #[test]
    fn normalises_case_accents_and_punctuation() {
        assert_eq!(normalise("Beyoncé – Halo!"), "beyonce halo");
        assert_eq!(normalise("Don't Stop Me Now"), "dont stop me now");
        assert_eq!(normalise("Simon & Garfunkel"), "simon and garfunkel");
        assert_eq!(bare("Song (feat. X) - Remastered 2011"), "song");
        assert_eq!(bare("Song feat. X"), "song");
        assert_eq!(bare("(What's the Story) Morning Glory?"), "morning glory");
    }

    #[test]
    fn matches_through_decorations_and_featured_artists() {
        let c = cand(
            "Dreamer",
            Some("Ozzy Osbourne feat. Mike McCready"),
            Some("Down to Earth"),
            None,
        );
        let want = c.item_id;
        let m = Matcher::new(vec![c]);
        assert_eq!(
            m.find(&q("Dreamer - 2011 Remaster", "Ozzy Osbourne")),
            Some(want)
        );
        assert_eq!(m.find(&q("Dreamer", "Mike McCready")), Some(want));
        assert_eq!(m.find(&q("Dreamer", "Someone Else")), None);
    }

    #[test]
    fn different_recordings_do_not_match() {
        let studio = cand("San Quentin", Some("Nickelback"), None, None);
        let inst = cand("San Quentin (Instrumental)", Some("Nickelback"), None, None);
        let (s, i) = (studio.item_id, inst.item_id);
        let m = Matcher::new(vec![inst, studio]);
        assert_eq!(m.find(&q("San Quentin", "Nickelback")), Some(s));
        assert_eq!(
            m.find(&q("San Quentin - Instrumental", "Nickelback")),
            Some(i)
        );
        assert_eq!(m.find(&q("San Quentin - Live", "Nickelback")), None);
    }

    #[test]
    fn reads_artist_and_title_from_untagged_file_names() {
        let c = cand(
            "12 - Nickelback - San Quentin (Instrumental)",
            None,
            None,
            None,
        );
        let want = c.item_id;
        let m = Matcher::new(vec![c, cand("Bass Stem_01", None, None, None)]);
        assert_eq!(
            m.find(&q("San Quentin (Instrumental)", "Nickelback")),
            Some(want)
        );
        assert_eq!(m.find(&q("Bass Stem_01", "Anyone")), None);
    }

    #[test]
    fn prefers_exact_title_album_and_close_duration() {
        let a = cand("Song", Some("Band"), Some("Live Album"), Some(300_000));
        let b = cand("Song", Some("Band"), Some("Studio Album"), Some(200_000));
        let (ia, ib) = (a.item_id, b.item_id);
        let m = Matcher::new(vec![a, b]);
        let mut query = q("Song", "Band");
        query.album = Some("Studio Album");
        assert_eq!(m.find(&query), Some(ib));
        query.album = None;
        query.duration_ms = Some(299_000);
        assert_eq!(m.find(&query), Some(ia));
        query.duration_ms = Some(500_000);
        assert_eq!(m.find(&query), None, "lengths too far apart");
    }

    #[test]
    fn isrc_wins() {
        let mut c = cand("Totally Different Title", Some("X"), None, None);
        c.isrc = Some("GB-AYE-03-00012".into());
        let want = c.item_id;
        let m = Matcher::new(vec![c]);
        let mut query = q("Anything", "Anyone");
        query.isrc = Some("gbaye0300012");
        assert_eq!(m.find(&query), Some(want));
    }

    #[test]
    fn band_names_with_the_and_ampersand() {
        let c = cand("Mrs. Robinson", Some("Simon & Garfunkel"), None, None);
        let d = cand("Yesterday", Some("The Beatles"), None, None);
        let (ic, id) = (c.item_id, d.item_id);
        let m = Matcher::new(vec![c, d]);
        assert_eq!(m.find(&q("Mrs Robinson", "Simon and Garfunkel")), Some(ic));
        assert_eq!(
            m.find(&q("Yesterday - Remastered 2009", "Beatles")),
            Some(id)
        );
    }
}
