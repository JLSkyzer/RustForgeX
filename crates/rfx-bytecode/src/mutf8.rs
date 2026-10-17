//! UTF-8 modifie de la JVM (JVMS §4.4.7).
//!
//! Deux ecarts a l'UTF-8 : le caractere nul s'ecrit sur deux octets (`C0 80`), et un
//! caractere hors du plan de base s'ecrit en deux demi-codets de trois octets chacun.
//! Les octets sont donc lus en codets UTF-16, puis assembles.

/// Decode une chaine, ou rend `None` si elle est invalide.
pub(crate) fn decode(bytes: &[u8]) -> Option<String> {
    // Cas courant : des noms de classes et de methodes en ASCII.
    if bytes.iter().all(|&b| b != 0 && b < 0x80) {
        return std::str::from_utf8(bytes).ok().map(str::to_owned);
    }
    let mut units = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == 0 {
            return None;
        }
        if b < 0x80 {
            units.push(u16::from(b));
            i += 1;
        } else if b & 0xE0 == 0xC0 {
            let c = continuation(bytes, i + 1)?;
            units.push((u16::from(b & 0x1F) << 6) | c);
            i += 2;
        } else if b & 0xF0 == 0xE0 {
            let c = continuation(bytes, i + 1)?;
            let d = continuation(bytes, i + 2)?;
            units.push((u16::from(b & 0x0F) << 12) | (c << 6) | d);
            i += 3;
        } else {
            return None;
        }
    }
    String::from_utf16(&units).ok()
}

/// Six bits utiles d'un octet de continuation `10xxxxxx`.
fn continuation(bytes: &[u8], at: usize) -> Option<u16> {
    let byte = *bytes.get(at)?;
    (byte & 0xC0 == 0x80).then_some(u16::from(byte & 0x3F))
}

#[cfg(test)]
mod tests {
    use super::decode;

    #[test]
    fn ascii_and_two_and_three_byte_forms_decode() {
        assert_eq!(
            decode(b"net/minecraft/Foo").as_deref(),
            Some("net/minecraft/Foo")
        );
        // « é » sur deux octets, « € » sur trois, et le nul en forme longue.
        assert_eq!(decode(&[0xC3, 0xA9]).as_deref(), Some("é"));
        assert_eq!(decode(&[0xE2, 0x82, 0xAC]).as_deref(), Some("€"));
        assert_eq!(decode(&[0x61, 0xC0, 0x80, 0x62]).as_deref(), Some("a\0b"));
    }

    #[test]
    fn supplementary_characters_come_as_two_surrogates() {
        // U+1F600 : D83D DE00, chaque demi-codet sur trois octets.
        let bytes = [0xED, 0xA0, 0xBD, 0xED, 0xB8, 0x80];
        assert_eq!(decode(&bytes).as_deref(), Some("\u{1F600}"));
    }

    #[test]
    fn invalid_sequences_are_refused() {
        assert_eq!(decode(&[0x00]), None, "nul sur un octet");
        assert_eq!(decode(&[0xC3]), None, "tronque");
        assert_eq!(decode(&[0xC3, 0x41]), None, "continuation absente");
        assert_eq!(
            decode(&[0xF0, 0x9F, 0x98, 0x80]),
            None,
            "forme UTF-8 a quatre octets"
        );
        assert_eq!(decode(&[0xED, 0xA0, 0xBD]), None, "demi-codet isole");
    }
}
