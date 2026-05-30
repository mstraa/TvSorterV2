/// Human-readable byte sizes using French-style units (o, Ko, Mo, Go, To)
/// with a comma decimal separator. Mirrors the V1 `human_file_size`.
pub fn human_file_size(size: Option<i64>) -> String {
    let size = match size {
        Some(value) => value.max(0),
        None => return String::new(),
    };
    let units = ["o", "Ko", "Mo", "Go", "To"];
    let mut value = size as f64;
    let mut unit_index = 0usize;
    while value >= 1024.0 && unit_index < units.len() - 1 {
        value /= 1024.0;
        unit_index += 1;
    }
    let decimals = if unit_index <= 2 { 0 } else { 2 };
    let formatted = format!("{value:.decimals$}");
    let trimmed = if formatted.contains('.') {
        formatted
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    } else {
        formatted
    };
    format!("{} {}", trimmed.replace('.', ","), units[unit_index])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_units() {
        assert_eq!(human_file_size(None), "");
        assert_eq!(human_file_size(Some(0)), "0 o");
        assert_eq!(human_file_size(Some(512)), "512 o");
        assert_eq!(human_file_size(Some(22 * 1024 * 1024)), "22 Mo");
        // 1.34 Go style with comma decimal
        let go = (1.34 * 1024.0 * 1024.0 * 1024.0) as i64;
        assert_eq!(human_file_size(Some(go)), "1,34 Go");
    }
}
