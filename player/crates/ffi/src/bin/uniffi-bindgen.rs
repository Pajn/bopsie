//! Generates the Swift and Kotlin bindings, run by `macos/scripts/build-rust.sh`.

fn main() {
    uniffi::uniffi_bindgen_main()
}
