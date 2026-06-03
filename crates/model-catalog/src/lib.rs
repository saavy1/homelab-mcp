pub fn crate_ready() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crate_ready() {
        assert!(crate_ready());
    }
}
