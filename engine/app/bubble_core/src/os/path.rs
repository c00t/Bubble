use std::collections::HashMap;

/// Given a desired file name `desired_name` (without any directory or extension part), map it to
/// a "similar" name that is safe to use on most file systems.
///
/// Because that these files may be used in different file systems, we need to make sure that the name is safe to use on all file systems.
pub fn file_system_safe_name(desired_name: &str) -> Option<String> {
    // 1. Limit base name to 128 characters
    let mut result = desired_name.chars().take(128).collect::<String>();

    // 2. Replace reserved characters with underscores
    const RESERVED_CHARS: &str = r#"/<>:"\|?*"#;
    result = result
        .chars()
        .map(|c| {
            if (c as u8 >= 1 && c as u8 <= 31) || RESERVED_CHARS.contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();

    // 3. Remove trailing spaces and periods
    while result.ends_with(' ') || result.ends_with('.') {
        result.pop();
    }

    // 4. Handle DOS reserved names
    const RESERVED_NAMES: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];

    if RESERVED_NAMES
        .iter()
        .any(|&name| result.eq_ignore_ascii_case(name))
    {
        result.push_str("_reserved");
    }

    // if it's empty, let caller handle it
    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

/// Represents a string with optional ordinal number
#[derive(Clone, Debug)]
struct OrdinalName {
    stem: String,
    ordinal: Option<u32>,
}

impl OrdinalName {
    /// Parse a name into stem and optional ordinal
    fn parse(name: &str) -> Self {
        // Find last parenthesized number if it exists
        let mut stem = name.to_string();
        let mut ordinal = None;

        if let Some(last_paren) = name.rfind('(') {
            if name.ends_with(')') {
                let ordinal_str = &name[last_paren + 1..name.len() - 1];
                // Only parse as ordinal if all characters are digits
                if ordinal_str.chars().all(|c| c.is_ascii_digit()) {
                    if let Ok(num) = ordinal_str.parse::<u32>() {
                        // Valid ordinal found
                        stem = name[..last_paren].trim_end().to_string();
                        ordinal = Some(num);
                    }
                }
            }
        }

        Self { stem, ordinal }
    }
}

/// Manages name ordinals and duplicates
#[derive(Default)]
pub struct OrdinalNameManager {
    ordinals: HashMap<String, u32>,
}

impl OrdinalNameManager {
    /// Create new manager
    pub fn new() -> Self {
        Self::default()
    }

    /// update an ordinal for a name
    pub fn update_ordinal(&mut self, name: &str) {
        let parsed = OrdinalName::parse(name);
        let key = parsed.stem.to_lowercase();

        if let Some(ordinal) = parsed.ordinal {
            let entry = self.ordinals.entry(key).or_insert(0);
            *entry = (*entry).max(ordinal);
        } else {
            self.ordinals.entry(key).or_insert(0);
        }
    }

    /// Generate name for duplicate
    pub fn get_ordinal_name(&mut self, original: &str) -> String {
        let parsed = OrdinalName::parse(original);
        let key = parsed.stem.to_lowercase();

        if !self.ordinals.contains_key(&key) {
            return parsed.stem;
        }

        let next_ordinal = {
            let entry = self.ordinals.entry(key).or_insert(0);
            *entry += 1;
            *entry
        };

        format!("{} ({})", parsed.stem, next_ordinal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_first_use_returns_original() {
        let mut manager = OrdinalNameManager::new();
        assert_eq!(manager.get_ordinal_name("bork"), "bork");
    }

    #[test]
    fn test_basic_duplicate() {
        let mut manager = OrdinalNameManager::new();
        manager.update_ordinal("bork");
        assert_eq!(manager.get_ordinal_name("bork"), "bork (1)");
    }

    #[test]
    fn test_existing_ordinal_first_use() {
        let mut manager = OrdinalNameManager::new();
        assert_eq!(manager.get_ordinal_name("bork (1)"), "bork");
    }

    #[test]
    fn test_existing_ordinal_with_seen() {
        let mut manager = OrdinalNameManager::new();
        manager.update_ordinal("bork");
        assert_eq!(manager.get_ordinal_name("bork (1)"), "bork (1)");
    }

    #[test]
    fn test_increment_from_existing_ordinal() {
        let mut manager = OrdinalNameManager::new();
        manager.update_ordinal("bork (1)");
        assert_eq!(manager.get_ordinal_name("bork (1)"), "bork (2)");
    }

    #[test]
    fn test_increment_from_high_ordinal() {
        let mut manager = OrdinalNameManager::new();
        manager.update_ordinal("bork (271)");
        assert_eq!(manager.get_ordinal_name("bork (000)"), "bork (272)");
    }

    #[test]
    fn test_name_with_dots() {
        let mut manager = OrdinalNameManager::new();
        manager.update_ordinal("bork_1.1");
        assert_eq!(manager.get_ordinal_name("bork_1.1"), "bork_1.1 (1)");
    }

    #[test]
    fn test_name_with_spaced_ordinal() {
        let mut manager = OrdinalNameManager::new();
        manager.update_ordinal("bork ( 1)");
        assert_eq!(manager.get_ordinal_name("bork ( 1)"), "bork ( 1) (1)");
    }

    #[test]
    fn test_increment_from_previous_high() {
        let mut manager = OrdinalNameManager::new();
        manager.update_ordinal("bork (7)");
        assert_eq!(manager.get_ordinal_name("bork"), "bork (8)");
    }

    #[test]
    fn test_case_insensitive_matching() {
        let mut manager = OrdinalNameManager::new();
        manager.update_ordinal("bork (7)");
        assert_eq!(manager.get_ordinal_name("BORK"), "BORK (8)");
    }

    #[test]
    fn test_multiple_duplicates() {
        let mut manager = OrdinalNameManager::new();
        manager.update_ordinal("bork");
        manager.update_ordinal("bork (1)");
        manager.update_ordinal("bork (2)");
        assert_eq!(manager.get_ordinal_name("bork"), "bork (3)");
    }
}
