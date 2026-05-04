pub fn answer() -> u8 {
    41
}

#[cfg(test)]
mod tests {
    use super::answer;

    #[test]
    fn expects_the_answer() {
        assert_eq!(answer(), 42);
    }
}
