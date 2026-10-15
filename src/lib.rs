//! Minimal foundation crate; project functionality is not implemented yet.

/// Returns the current project-stage label for the command-line scaffold.
pub fn scaffold_status() -> &'static str {
    "Hermes RS scaffold: no agent functionality implemented"
}

#[cfg(test)]
mod tests {
    use super::scaffold_status;

    #[test]
    fn reports_scaffold_status() {
        assert_eq!(
            scaffold_status(),
            "Hermes RS scaffold: no agent functionality implemented"
        );
    }
}
