//! Foundation crate. HRS-010 adds the session-store persistence slice
//! (specs/persistence-shim.md §6) behind the HRS-002 boundary.

pub mod persistence;

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
