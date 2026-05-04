pub fn beta() -> &'static str {
    "beta"
}

#[cfg(test)]
mod tests {
    use super::beta;

    #[test]
    fn returns_beta() {
        assert_eq!(beta(), "beta");
    }
}
