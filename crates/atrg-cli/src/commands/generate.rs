//! `atrg generate` — generate Rust code from lexicon JSON files.

use std::path::Path;

/// Run the lexicon code generator.
pub fn run(input: &str, output: &str) -> anyhow::Result<()> {
    let input_dir = Path::new(input);
    let output_dir = Path::new(output);

    if !input_dir.exists() {
        anyhow::bail!(
            "Input directory '{}' does not exist. Create it and add your lexicon .json files.",
            input
        );
    }

    println!("Generating code from lexicons...");
    println!("  Input:  {}", input_dir.display());
    println!("  Output: {}", output_dir.display());
    println!();

    let report =
        atrg_codegen::generate(input_dir, output_dir, atrg_codegen::GenOptions::default())?;

    println!("  ✓ Processed {} lexicon file(s)", report.files_processed);
    println!("  ✓ Generated {} type(s)", report.types_generated);
    println!("  ✓ Generated {} handler stub(s)", report.stubs_generated);

    for file in &report.output_files {
        println!("  → {}", file);
    }

    println!();
    println!("Add `mod generated;` to your src/lib.rs or src/main.rs to use the generated code.");

    Ok(())
}
