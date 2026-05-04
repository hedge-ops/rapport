pub fn alpha() -> &'static str {
    "alpha"
}

#[cfg(test)]
mod tests {
    use super::alpha;

    #[test]
    fn returns_alpha() {
        assert_eq!(alpha(), "alpha");
    }
}
