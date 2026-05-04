#![deny(rustdoc::broken_intra_doc_links)]

/// See [`MissingType`].
pub fn documented() -> u8 {
    1
}

#[cfg(test)]
mod tests {
    use super::documented;

    #[test]
    fn returns_value() {
        assert_eq!(documented(), 1);
    }
}
