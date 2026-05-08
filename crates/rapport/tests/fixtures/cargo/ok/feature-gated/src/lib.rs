#[cfg(not(feature = "extra"))]
compile_error!("rapport must pass the conventional feature set for this package");

pub fn answer() -> u8 {
    42
}
