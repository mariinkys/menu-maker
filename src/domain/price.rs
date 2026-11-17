//! Price parsing and Spanish-style formatting.
//!
//! Prices are stored exactly as the user typed them, so free text such as
//! "S/M" (según mercado) keeps working. When the text is a number it is
//! normalised to `12,50 €`.

use super::style::PriceStyle;

/// Parses user input like `12`, `12.5`, `12,50`, `12,50 €` or `€12.50` into cents.
pub fn parse_cents(input: &str) -> Option<i64> {
    let cleaned: String = input
        .trim()
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '€' && *c != '$' && *c != '£')
        .collect();
    if cleaned.is_empty() {
        return None;
    }
    let normalised = cleaned.replace(',', ".");
    // Reject things like "1.2.3".
    if normalised.matches('.').count() > 1 {
        return None;
    }
    if !normalised.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    let value: f64 = normalised.parse().ok()?;
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    Some((value * 100.0).round() as i64)
}

/// Returns the text to print for a price, or `None` when the field is empty.
pub fn format_price(input: &str, style: &PriceStyle) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    let Some(cents) = parse_cents(trimmed) else {
        return Some(trimmed.to_string());
    };
    let units = cents / 100;
    let rest = cents % 100;
    let number = if rest == 0 && !style.always_show_decimals {
        group_thousands(units)
    } else {
        format!("{},{:02}", group_thousands(units), rest)
    };
    let symbol = style.currency_symbol.trim();
    Some(if symbol.is_empty() {
        number
    } else if style.symbol_before {
        format!("{symbol}{number}")
    } else {
        format!("{number}\u{a0}{symbol}")
    })
}

fn group_thousands(value: i64) -> String {
    let digits = value.to_string();
    if digits.len() <= 4 {
        // Spanish style does not group 4-digit numbers.
        return digits;
    }
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('.');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(always: bool) -> PriceStyle {
        PriceStyle {
            currency_symbol: "€".into(),
            symbol_before: false,
            always_show_decimals: always,
            dot_leaders: true,
        }
    }

    #[test]
    fn parses_common_inputs() {
        assert_eq!(parse_cents("12"), Some(1200));
        assert_eq!(parse_cents("12,5"), Some(1250));
        assert_eq!(parse_cents("12.50 €"), Some(1250));
        assert_eq!(parse_cents("S/M"), None);
        assert_eq!(parse_cents("1.2.3"), None);
        assert_eq!(parse_cents(""), None);
    }

    #[test]
    fn formats_spanish_style() {
        assert_eq!(format_price("12", &style(false)).unwrap(), "12\u{a0}€");
        assert_eq!(format_price("12", &style(true)).unwrap(), "12,00\u{a0}€");
        assert_eq!(format_price("9,5", &style(false)).unwrap(), "9,50\u{a0}€");
        assert_eq!(
            format_price("12500", &style(false)).unwrap(),
            "12.500\u{a0}€"
        );
        assert_eq!(format_price("S/M", &style(false)).unwrap(), "S/M");
        assert_eq!(format_price("  ", &style(false)), None);
    }
}
