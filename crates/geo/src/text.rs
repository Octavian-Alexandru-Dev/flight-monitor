/// Lowercases and strips common diacritics so "Città" matches "citta".
pub fn fold(input: &str) -> String {
    input
        .trim()
        .chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à' | 'á' | 'â' | 'ä' | 'ã' | 'å' => 'a',
            'è' | 'é' | 'ê' | 'ë' => 'e',
            'ì' | 'í' | 'î' | 'ï' => 'i',
            'ò' | 'ó' | 'ô' | 'ö' | 'õ' => 'o',
            'ù' | 'ú' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            'ș' | 'ş' => 's',
            'ț' | 'ţ' => 't',
            other => other,
        })
        .collect()
}

/// True when `place` is `name` or starts with it followed by a separator,
/// so "London, Essex" and "Paris (Orly)" match "london" and "paris".
pub fn names_place(name: &str, place: &str) -> bool {
    place
        .strip_prefix(name)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', ',', '(', '-', '/']))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_accents_and_case() {
        assert_eq!(fold(" Città del Messico "), "citta del messico");
        assert_eq!(fold("São Paulo"), "sao paulo");
    }

    #[test]
    fn matches_place_prefixes_on_word_boundary() {
        assert!(names_place("london", "london, essex"));
        assert!(names_place("paris", "paris (orly, val-de-marne)"));
        assert!(!names_place("rome", "romeoville"));
    }
}
