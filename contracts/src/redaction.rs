//! Structural redaction (M0.6).
//!
//! A [`Secret`] value cannot be printed. There is exactly one way to read it --
//! [`Secret::expose`] -- and CI greps to ensure that call never appears in a
//! logging module. Redaction is a property of the type, not of programmer
//! discipline.

/// A value that refuses to render itself.
#[derive(Clone)]
pub struct Secret<T>(T);

impl<T> std::fmt::Debug for Secret<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[redacted]")
    }
}

impl<T> std::fmt::Display for Secret<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[redacted]")
    }
}

impl<T> Secret<T> {
    pub fn new(v: T) -> Self {
        Self(v)
    }

    /// The only way out. Grep for this in review; it must never appear in
    /// logging code.
    pub fn expose(&self) -> &T {
        &self.0
    }

    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> From<T> for Secret<T> {
    fn from(v: T) -> Self {
        Self::new(v)
    }
}

pub type Token = Secret<String>;
pub type Password = Secret<String>;

/// Presigned URLs carry credentials in the query string. Strip it before
/// logging.
pub fn sanitize_url(u: &str) -> String {
    match u.split_once('?') {
        Some((base, _)) => format!("{base}?[redacted]"),
        None => u.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_never_renders_its_value() {
        let t: Token = Secret::new("wJalrXUtnFEMI/K7MDENG".to_string());
        assert_eq!(format!("{t}"), "[redacted]");
        assert_eq!(format!("{t:?}"), "[redacted]");
        assert_eq!(t.expose(), "wJalrXUtnFEMI/K7MDENG");
    }

    #[test]
    fn sanitize_url_strips_query_string() {
        assert_eq!(
            sanitize_url("https://s3.example.com/chunk?X-Amz-Signature=abcdef"),
            "https://s3.example.com/chunk?[redacted]"
        );
        assert_eq!(
            sanitize_url("https://s3.example.com/chunk"),
            "https://s3.example.com/chunk"
        );
    }
}
