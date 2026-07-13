pub(crate) fn to_plural(s: &str) -> String {
    if s.ends_with('s') || s.ends_with('x') || s.ends_with('z')
        || s.ends_with("ch") || s.ends_with("sh")
    {
        format!("{}es", s)
    } else if s.ends_with('o')
        && !s.ends_with("eo") && !s.ends_with("io") && !s.ends_with("oo") && !s.ends_with("uo")
    {
        // consonant + o → oes (hero → heroes)
        format!("{}es", s)
    } else if s.ends_with('y')
        && !s.ends_with("ay") && !s.ends_with("ey") && !s.ends_with("oy") && !s.ends_with("uy")
    {
        format!("{}ies", &s[..s.len() - 1])
    } else {
        format!("{}s", s)
    }
}

pub(crate) fn to_pascal_singular(s: &str) -> String {
    s.split('_')
        .filter(|seg| !seg.is_empty())
        .map(|seg| {
            let mut chars = seg.chars();
            match chars.next() {
                None => String::new(),
                Some(first_char) => first_char.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plural_basic() {
        assert_eq!(to_plural("hero"), "heroes");
        assert_eq!(to_plural("mission"), "missions");
        assert_eq!(to_plural("team"), "teams");
    }

    #[test]
    fn test_plural_s_ending() {
        assert_eq!(to_plural("alias"), "aliases");
    }

    #[test]
    fn test_plural_y_ending() {
        assert_eq!(to_plural("category"), "categories");
    }

    #[test]
    fn test_plural_vowel_y_unchanged() {
        assert_eq!(to_plural("boy"), "boys");
    }

    #[test]
    fn test_pascal_singular() {
        assert_eq!(to_pascal_singular("hero"), "Hero");
        assert_eq!(to_pascal_singular("team"), "Team");
        assert_eq!(to_pascal_singular("secret_lair"), "SecretLair");
        assert_eq!(to_pascal_singular(""), "");
    }
}
