//! What identifies an item beyond its name: codes such as ISRCs and barcodes, MusicBrainz ids,
//! and a streaming service's own id; and the normalized names candidates for being the same
//! item are found by. Matching and lookup use the same normalization, so that a candidate
//! found by one is not then rejected by the other.

use deunicode::deunicode;

/// A kind of external id, named as Music Assistant names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum IdKind {
    /// A recording's code, shared by its releases and often by its remasters.
    Isrc,
    /// A release's UPC or EAN, as GTIN-14; shared across pressings.
    Barcode,
    MbRecording,
    MbTrack,
    MbRelease,
    MbReleaseGroup,
    MbArtist,
    /// The item's id on Spotify, as a link rather than evidence of what it is.
    Spotify,
}

impl IdKind {
    pub const ALL: [IdKind; 8] = [
        IdKind::Isrc,
        IdKind::Barcode,
        IdKind::MbRecording,
        IdKind::MbTrack,
        IdKind::MbRelease,
        IdKind::MbReleaseGroup,
        IdKind::MbArtist,
        IdKind::Spotify,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            IdKind::Isrc => "isrc",
            IdKind::Barcode => "barcode",
            IdKind::MbRecording => "musicbrainz_recordingid",
            IdKind::MbTrack => "musicbrainz_trackid",
            IdKind::MbRelease => "musicbrainz_albumid",
            IdKind::MbReleaseGroup => "musicbrainz_releasegroupid",
            IdKind::MbArtist => "musicbrainz_artistid",
            IdKind::Spotify => "spotify",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == name)
    }

    /// Whether two different values of this kind mean two different things: MusicBrainz ids
    /// do; a code shared across pressings or remasters does not.
    pub fn is_unique(self) -> bool {
        !matches!(self, IdKind::Isrc | IdKind::Barcode)
    }

    /// `value` as it is stored and compared, or `None` when it is not one: ISRCs uppercase
    /// without separators, barcodes as 14 digits, MusicBrainz ids lowercase.
    pub fn normalize(self, value: &str) -> Option<String> {
        let value = value.trim();
        match self {
            IdKind::Isrc => {
                let isrc: String = value
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric())
                    .map(|c| c.to_ascii_uppercase())
                    .collect();
                let valid = isrc.len() == 12
                    && isrc[..2].chars().all(|c| c.is_ascii_uppercase())
                    && isrc[2..5].chars().all(|c| c.is_ascii_alphanumeric())
                    && isrc[5..].chars().all(|c| c.is_ascii_digit());
                valid.then_some(isrc)
            }
            IdKind::Barcode => {
                let digits: String = value.chars().filter(char::is_ascii_digit).collect();
                (8..=14)
                    .contains(&digits.len())
                    .then(|| format!("{digits:0>14}"))
            }
            IdKind::Spotify => (!value.is_empty()).then(|| value.to_owned()),
            _ => {
                let id = value.to_ascii_lowercase();
                let uuid = id.len() == 36
                    && id.chars().enumerate().all(|(i, c)| match i {
                        8 | 13 | 18 | 23 => c == '-',
                        _ => c.is_ascii_hexdigit(),
                    });
                uuid.then_some(id)
            }
        }
    }
}

/// Ids as a provider reports them, normalized, with ones that are not ids left out.
pub fn normalized(ids: &[(IdKind, String)]) -> Vec<(IdKind, String)> {
    let mut out: Vec<(IdKind, String)> = ids
        .iter()
        .filter_map(|(kind, value)| Some((*kind, kind.normalize(value)?)))
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Punctuation, which does not tell names apart.
fn is_punctuation(c: char) -> bool {
    c.is_ascii_punctuation() && !matches!(c, '+' | '#' | '*' | '=')
        || ('\u{2010}'..='\u{205f}').contains(&c)
        || matches!(
            c,
            '\u{00a1}' | '\u{00bf}' | '\u{00ab}' | '\u{00bb}' | '\u{00b7}'
        )
}

/// A symbol at the edge of a name is part of it: "MOTOMAMI +" is not "MOTOMAMI".
fn is_symbol(c: char) -> bool {
    !c.is_alphanumeric() && !c.is_whitespace() && !is_punctuation(c)
}

/// A name as candidates for being the same item are found and compared by: case, accents,
/// punctuation and spacing do not count; `&` is `and`; "Beatles, The" is "the beatles". A
/// symbol at either edge counts, and a name of nothing but symbols is kept as it is.
pub fn search_key(name: &str) -> String {
    let name = name.trim();
    let leading: String = name.chars().take_while(|c| is_symbol(*c)).collect();
    let trailing: String = {
        let rest: Vec<char> = name.chars().skip(leading.chars().count()).collect();
        let tail: Vec<char> = rest
            .iter()
            .rev()
            .take_while(|c| is_symbol(**c) || c.is_whitespace())
            .copied()
            .collect();
        tail.into_iter()
            .rev()
            .filter(|c| !c.is_whitespace())
            .collect()
    };
    let mut words: Vec<String> = deunicode(name)
        .to_lowercase()
        .replace(['\'', '`'], "")
        .replace('&', " and ")
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect();
    if words.is_empty() {
        return name.to_lowercase();
    }
    // "Beatles, The" reads as "The Beatles".
    if words.len() > 1 && name.to_lowercase().ends_with(", the") {
        words.pop();
        words.insert(0, "the".to_owned());
    }
    let mut key = words.join(" ");
    if !leading.is_empty() {
        key = format!("{leading} {key}");
    }
    if !trailing.is_empty() {
        key = format!("{key} {trailing}");
    }
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_compared_as_people_read_them() {
        assert_eq!(search_key("Björk"), search_key("Bjork"));
        assert_eq!(search_key("  The   Beatles "), "the beatles");
        assert_eq!(search_key("Beatles, The"), "the beatles");
        assert_eq!(search_key("Simon & Garfunkel"), "simon and garfunkel");
        assert_eq!(search_key("Don't Stop"), search_key("Dont Stop"));
        assert_eq!(search_key("Ça va?"), "ca va");
        assert_eq!(search_key("Hey—Jude…"), search_key("hey jude"));
    }

    #[test]
    fn symbols_at_the_edge_are_part_of_the_name() {
        assert_ne!(search_key("MOTOMAMI +"), search_key("MOTOMAMI"));
        assert_eq!(search_key("MOTOMAMI +"), "motomami +");
        assert_ne!(search_key("÷"), search_key("x"));
        assert_eq!(search_key("!!!"), "!!!");
        assert_eq!(search_key("÷"), "÷");
    }

    #[test]
    fn ids_are_normalized_or_refused() {
        assert_eq!(
            IdKind::Isrc.normalize("us-um7-17-03861"),
            Some("USUM71703861".to_owned())
        );
        assert_eq!(IdKind::Isrc.normalize("not an isrc"), None);
        assert_eq!(
            IdKind::Barcode.normalize("602557 382549"),
            Some("00602557382549".to_owned())
        );
        assert_eq!(IdKind::Barcode.normalize("123"), None);
        assert_eq!(
            IdKind::MbRelease.normalize("A1B2C3D4-0000-1111-2222-333344445555"),
            Some("a1b2c3d4-0000-1111-2222-333344445555".to_owned())
        );
        assert_eq!(IdKind::MbRelease.normalize("nope"), None);
        assert!(!IdKind::Isrc.is_unique() && IdKind::MbRecording.is_unique());
        assert_eq!(
            IdKind::parse("musicbrainz_albumid"),
            Some(IdKind::MbRelease)
        );
    }
}
