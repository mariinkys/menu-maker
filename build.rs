//! Compiles the Slint UI with the gettext translations in `translations/`
//! bundled into the binary (no runtime files needed).

fn main() {
    let config = slint_build::CompilerConfiguration::new()
        .with_style("fluent".into())
        .with_bundled_translations("translations")
        // One translation per string, not per component (matches `slint-tr-extractor --no-default-translation-context`).
        .with_default_translation_context(slint_build::DefaultTranslationContext::None);
    slint_build::compile_with_config("ui/app.slint", config).expect("Slint UI failed to compile");
}
