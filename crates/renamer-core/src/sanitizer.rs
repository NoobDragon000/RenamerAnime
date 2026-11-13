use regex::Regex;
use std::sync::OnceLock;

const MAX_COMPONENT_BYTES: usize = 240;

const WINDOWS_RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Sanitize a single filename or folder name component so it is valid on
/// Windows (NTFS), Fedora Linux (ext4/btrfs), and Android (SAF / F2FS).
pub fn sanitize_filename_component(input: &str, extension: &str) -> String {
    static COMPACT_SLASH_RE: OnceLock<Regex> = OnceLock::new();
    static SEP_RE: OnceLock<Regex> = OnceLock::new();
    static FORBIDDEN_RE: OnceLock<Regex> = OnceLock::new();
    static WS_RE: OnceLock<Regex> = OnceLock::new();

    // Preserve tight slashes inside words like "In/Spectre" -> "In-Spectre"
    let compact_slash_re =
        COMPACT_SLASH_RE.get_or_init(|| Regex::new(r"([[:alnum:]])/([[:alnum:]])").unwrap());
    let sep_re = SEP_RE.get_or_init(|| Regex::new(r"[:/\\|]+").unwrap());
    let forbidden_re = FORBIDDEN_RE.get_or_init(|| Regex::new(r#"[?*"<>]+"#).unwrap());
    let ws_re = WS_RE.get_or_init(|| Regex::new(r"\s+").unwrap());

    // Strip control characters (0x00..=0x1F and 0x7F)
    let no_ctrl: String = input
        .chars()
        .filter(|&c| !c.is_control())
        .collect();

    let slash_fixed = compact_slash_re.replace_all(&no_ctrl, "$1-$2");
    // Replace remaining path separators and colons with " - "
    let replaced = sep_re.replace_all(&slash_fixed, " - ");
    let stripped = forbidden_re.replace_all(&replaced, "");
    let collapsed = ws_re.replace_all(&stripped, " ");

    // Windows forbids trailing spaces and trailing dots on folder/file names
    let mut stem = collapsed.trim().trim_end_matches('.').trim().to_string();

    if stem.is_empty() {
        return String::new();
    }

    // Guard Windows reserved device names (CON, PRN, AUX, NUL, COM1-9, LPT1-9)
    let upper = stem.to_ascii_uppercase();
    if WINDOWS_RESERVED_NAMES.contains(&upper.as_str()) {
        stem.push('_');
    }

    // Truncate stem on UTF-8 char boundaries so stem + extension <= 240 bytes
    let ext_bytes = extension.len();
    let max_stem_bytes = MAX_COMPONENT_BYTES.saturating_sub(ext_bytes);
    if stem.len() > max_stem_bytes {
        let mut end = max_stem_bytes;
        while end > 0 && !stem.is_char_boundary(end) {
            end -= 1;
        }
        stem.truncate(end);
        stem = stem.trim_end_matches([' ', '.']).to_string();
    }

    format!("{stem}{extension}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_colons_and_strips_forbidden_chars() {
        assert_eq!(
            sanitize_filename_component("Re:ZERO -Starting Life?*", ""),
            "Re - ZERO -Starting Life"
        );
        assert_eq!(
            sanitize_filename_component("In/Spectre", ""),
            "In-Spectre"
        );
    }

    #[test]
    fn strips_trailing_dots_and_spaces() {
        assert_eq!(
            sanitize_filename_component("Steins;Gate... ", ""),
            "Steins;Gate"
        );
    }

    #[test]
    fn guards_windows_reserved_names() {
        assert_eq!(sanitize_filename_component("CON", ".mkv"), "CON_.mkv");
        assert_eq!(sanitize_filename_component("aux", ""), "aux_");
    }

    #[test]
    fn truncates_utf8_safely_preserving_extension() {
        let long_cjk = "記憶管理局".repeat(40);
        let result = sanitize_filename_component(&long_cjk, ".mkv");
        assert!(result.len() <= MAX_COMPONENT_BYTES);
        assert!(result.ends_with(".mkv"));
    }
}
