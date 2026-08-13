uniffi::setup_scaffolding!();

#[allow(clippy::too_long_first_doc_paragraph)]
pub mod context_recognition;

// #[uniffi::export]
// pub fn greet(name: &str) -> String {
//     format!("Hello, {name}! Greetings from Rust 🦀")
// }

// #[uniffi::export]
// pub fn fibonacci(n: u32) -> u64 {
//     let (mut a, mut b) = (0u64, 1u64);
//     for _ in 0..n {
//         let t = a + b;
//         a = b;
//         b = t;
//     }
//     a
// }

// #[uniffi::export]
// pub fn great_add(a: i32, b: i32) -> i64 {
//     (a + b).into()
// }

pub use context_recognition::DangerLevel;
