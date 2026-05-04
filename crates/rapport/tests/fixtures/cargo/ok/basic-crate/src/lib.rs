pub fn answer() -> u8 {
    42
}

#[cfg(test)]
mod tests {
    use super::answer;

    #[test]
    fn returns_answer() {
        assert_eq!(answer(), 42);
    }
}
