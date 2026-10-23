//! PCI bus ID normalization (matches the original Python agent).

/// Normalize a PCI bus ID the same way as `atlasgpu_agent.py`:
/// strip a leading domain (`0000:01:00.0` → `01:00.0`) and uppercase.
pub fn normalize_pci(value: &str) -> String {
    let value = value.trim();
    let value = if value.matches(':').count() >= 2 {
        value.split_once(':').map(|(_, rest)| rest).unwrap_or(value)
    } else {
        value
    };
    value.to_ascii_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_domain_and_uppercases() {
        assert_eq!(normalize_pci("0000:01:00.0"), "01:00.0");
        assert_eq!(normalize_pci("0000:3b:00.0"), "3B:00.0");
    }

    #[test]
    fn keeps_already_short_form() {
        assert_eq!(normalize_pci("01:00.0"), "01:00.0");
        assert_eq!(normalize_pci(" 1a:00.0 "), "1A:00.0");
    }

    #[test]
    fn single_colon_unchanged_shape() {
        // Fewer than two colons → no domain strip.
        assert_eq!(normalize_pci("01:00"), "01:00");
    }
}
