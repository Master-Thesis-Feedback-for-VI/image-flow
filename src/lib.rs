pub fn greet(name: &str) -> String {
    format!("Hello, {name}! Greetings from Rust 🦀")
}

pub fn fibonacci(n: u32) -> u64 {
    let (mut a, mut b) = (0u64, 1u64);
    for _ in 0..n {
        let t = a + b;
        a = b;
        b = t;
    }
    a
}

#[cfg(target_os = "android")]
mod android {
    use super::{fibonacci, greet};
    use jni::objects::{JClass, JString};
    use jni::sys::jint;
    use jni::{Env, NativeMethod};

    include!(concat!(env!("OUT_DIR"), "/native_methods.rs"));

    fn native_greet<'local>(
        env: &mut Env<'local>,
        _class: JClass<'local>,
        name: JString<'local>,
    ) -> Result<JString<'local>, jni::errors::Error> {
        let name_str = name.try_to_string(env)?;
        JString::from_str(env, greet(&name_str))
    }

    fn native_fibonacci<'local>(
        _env: &mut Env<'local>,
        _class: JClass<'local>,
        n: jint,
    ) -> Result<jint, jni::errors::Error> {
        Ok(fibonacci(n as u32) as jint)
    }
}
