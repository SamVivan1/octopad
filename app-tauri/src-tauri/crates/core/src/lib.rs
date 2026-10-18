pub mod platform;
pub mod proto;
pub mod config_paths;

pub fn version() -> &'static str {
    "0.1.0"
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_non_empty() {
        assert!(!super::version().is_empty());
    }
}
