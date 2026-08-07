fn main() {
    let java_type =
        std::env::var("JAVA_TYPE").unwrap_or_else(|_| "com.example.myapp.MainActivity".into());
    let out_dir = std::env::var("OUT_DIR").unwrap();

    let generated = format!(
        r#"
const _NATIVE_METHODS: &[jni::NativeMethod] = &[
    jni::native_method! {{
        java_type = "{java_type}",
        static extern fn native_greet(name: JString) -> JString,
    }},
    jni::native_method! {{
        java_type = "{java_type}",
        static extern fn native_fibonacci(n: jint) -> jint,
    }},
];
"#
    );

    std::fs::write(format!("{out_dir}/native_methods.rs"), generated).unwrap();
    println!("cargo:rerun-if-env-changed=JAVA_TYPE");
}
