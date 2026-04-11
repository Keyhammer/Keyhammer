/// Diacritics and accent normalization.
///
/// Strips common accented characters to their ASCII base form.
/// Covers Latin, Germanic, and Romance language accents.

/// Strip diacritics from a byte slice. Returns ASCII-normalized bytes.
/// Works on UTF-8 input by mapping common multi-byte sequences to ASCII.
pub fn strip_diacritics(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch == 'ß' {
            out.push_str("ss");
        } else {
            out.push(normalize_char(ch));
        }
    }
    out
}

fn normalize_char(c: char) -> char {
    match c {
        'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' | 'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => {
            if c.is_uppercase() { 'A' } else { 'a' }
        }
        'È' | 'É' | 'Ê' | 'Ë' | 'è' | 'é' | 'ê' | 'ë' => {
            if c.is_uppercase() { 'E' } else { 'e' }
        }
        'Ì' | 'Í' | 'Î' | 'Ï' | 'ì' | 'í' | 'î' | 'ï' => {
            if c.is_uppercase() { 'I' } else { 'i' }
        }
        'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'ò' | 'ó' | 'ô' | 'õ' | 'ö' => {
            if c.is_uppercase() { 'O' } else { 'o' }
        }
        'Ù' | 'Ú' | 'Û' | 'Ü' | 'ù' | 'ú' | 'û' | 'ü' => {
            if c.is_uppercase() { 'U' } else { 'u' }
        }
        'Ñ' | 'ñ' => if c.is_uppercase() { 'N' } else { 'n' },
        'Ç' | 'ç' => if c.is_uppercase() { 'C' } else { 'c' },
        'Ÿ' | 'ÿ' | 'Ý' | 'ý' => if c.is_uppercase() { 'Y' } else { 'y' },
        'Ð' | 'ð' => if c.is_uppercase() { 'D' } else { 'd' },
        'Þ' | 'þ' => if c.is_uppercase() { 'T' } else { 't' },
        'ß' => 's',
        'Ø' | 'ø' => if c.is_uppercase() { 'O' } else { 'o' },
        'Æ' | 'æ' => if c.is_uppercase() { 'A' } else { 'a' },
        'Œ' | 'œ' => if c.is_uppercase() { 'O' } else { 'o' },
        'Ł' | 'ł' => if c.is_uppercase() { 'L' } else { 'l' },
        'Ž' | 'ž' => if c.is_uppercase() { 'Z' } else { 'z' },
        'Š' | 'š' => if c.is_uppercase() { 'S' } else { 's' },
        'Č' | 'č' => if c.is_uppercase() { 'C' } else { 'c' },
        'Ř' | 'ř' => if c.is_uppercase() { 'R' } else { 'r' },
        'Ź' | 'ź' | 'Ż' | 'ż' => if c.is_uppercase() { 'Z' } else { 'z' },
        'Ć' | 'ć' => if c.is_uppercase() { 'C' } else { 'c' },
        'Ń' | 'ń' => if c.is_uppercase() { 'N' } else { 'n' },
        'Ą' | 'ą' => if c.is_uppercase() { 'A' } else { 'a' },
        'Ę' | 'ę' => if c.is_uppercase() { 'E' } else { 'e' },
        'Ğ' | 'ğ' => if c.is_uppercase() { 'G' } else { 'g' },
        'İ' | 'ı' => if c.is_uppercase() { 'I' } else { 'i' },
        'Ş' | 'ş' => if c.is_uppercase() { 'S' } else { 's' },
        _ => c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_accents() {
        assert_eq!(strip_diacritics("café"), "cafe");
        assert_eq!(strip_diacritics("naïve"), "naive");
        assert_eq!(strip_diacritics("résumé"), "resume");
    }

    #[test]
    fn german() {
        assert_eq!(strip_diacritics("über"), "uber");
        assert_eq!(strip_diacritics("straße"), "strasse");
        assert_eq!(strip_diacritics("Ärger"), "Arger");
    }

    #[test]
    fn spanish() {
        assert_eq!(strip_diacritics("España"), "Espana");
        assert_eq!(strip_diacritics("niño"), "nino");
    }

    #[test]
    fn no_accents_unchanged() {
        assert_eq!(strip_diacritics("hello world"), "hello world");
        assert_eq!(strip_diacritics("12345"), "12345");
    }

    #[test]
    fn mixed() {
        assert_eq!(strip_diacritics("Łódź"), "Lodz");
        assert_eq!(strip_diacritics("Zürich"), "Zurich");
    }
}
