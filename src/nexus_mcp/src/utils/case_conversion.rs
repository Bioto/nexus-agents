//! Case conversion utilities for string transformation.

/// Convert a string to PascalCase.
///
/// Handles hyphen-separated, underscore-separated, and camelCase inputs.
///
/// # Examples
/// ```
/// use nexus_mcp::utils::to_pascal_case;
///
/// assert_eq!(to_pascal_case("hello_world"), "HelloWorld");
/// assert_eq!(to_pascal_case("hello-world"), "HelloWorld");
/// assert_eq!(to_pascal_case("helloWorld"), "HelloWorld");
/// ```
#[must_use]
pub fn to_pascal_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize = true;
    for c in s.chars() {
        if c == '_' || c == '-' {
            capitalize = true;
        } else if capitalize {
            result.extend(c.to_uppercase());
            capitalize = false;
        } else {
            result.push(c);
        }
    }
    result
}

/// Convert a string to snake_case.
///
/// Converts hyphens and uppercase letters to snake_case format.
///
/// # Examples
/// ```
/// use nexus_mcp::utils::to_snake_case;
///
/// assert_eq!(to_snake_case("helloWorld"), "hello_world");
/// assert_eq!(to_snake_case("hello-world"), "hello_world");
/// assert_eq!(to_snake_case("HelloWorld"), "hello_world");
/// ```
#[must_use]
pub fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    let mut prev_was_separator = false;

    for (i, c) in s.chars().enumerate() {
        if c == '-' || c == '_' {
            if !prev_was_separator && i > 0 {
                result.push('_');
            }
            prev_was_separator = true;
        } else if c.is_uppercase() {
            if !prev_was_separator && i > 0 {
                result.push('_');
            }
            result.extend(c.to_lowercase());
            prev_was_separator = false;
        } else {
            result.push(c);
            prev_was_separator = false;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_snake_case_simple() {
        assert_eq!(to_snake_case("helloWorld"), "hello_world");
        assert_eq!(to_snake_case("HelloWorld"), "hello_world");
        assert_eq!(to_snake_case("hello"), "hello");
    }

    #[test]
    fn test_to_snake_case_with_hyphens() {
        assert_eq!(to_snake_case("hello-world"), "hello_world");
        assert_eq!(to_snake_case("get-library-docs"), "get_library_docs");
    }

    #[test]
    fn test_to_snake_case_with_underscores() {
        assert_eq!(to_snake_case("hello_world"), "hello_world");
        assert_eq!(to_snake_case("HELLO_WORLD"), "h_e_l_l_o_w_o_r_l_d");
    }

    #[test]
    fn test_to_snake_case_consecutive_separators() {
        assert_eq!(to_snake_case("hello--world"), "hello_world");
        assert_eq!(to_snake_case("hello__world"), "hello_world");
    }

    #[test]
    fn test_to_pascal_case_simple() {
        assert_eq!(to_pascal_case("hello_world"), "HelloWorld");
        assert_eq!(to_pascal_case("hello-world"), "HelloWorld");
        assert_eq!(to_pascal_case("hello"), "Hello");
    }

    #[test]
    fn test_to_pascal_case_already_pascal() {
        assert_eq!(to_pascal_case("HelloWorld"), "HelloWorld");
    }

    #[test]
    fn test_to_pascal_case_unicode() {
        // German sharp s (ß) in the middle of a word stays as-is
        assert_eq!(to_pascal_case("straße"), "Straße");
        // Test uppercase at word boundary - ß.to_uppercase() yields SS
        assert_eq!(to_pascal_case("ße_test"), "SSeTest");
    }

    #[test]
    fn test_to_snake_case_unicode() {
        // Turkish dotted I lowercases properly
        assert_eq!(to_snake_case("İstanbul"), "i̇stanbul");
    }
}
