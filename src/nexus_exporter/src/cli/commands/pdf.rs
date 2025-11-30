use crate::error::Result;
use crate::services::{PdfExportConfig, PdfExporter};
use clap::Args;
use std::path::PathBuf;

#[derive(Args, Debug)]
#[command(about = "Generate a PDF document from text or JSON content")]
pub struct PdfArgs {
    /// Output file path (default: output/export.pdf)
    #[arg(short, long, default_value = "output/export.pdf")]
    pub output: PathBuf,

    /// Input file path (text or JSON file to convert to PDF)
    #[arg(short, long)]
    pub input: Option<PathBuf>,

    /// Text content to include in the PDF (if no input file provided)
    #[arg(short, long)]
    pub text: Option<String>,

    /// Title of the PDF document
    #[arg(long, default_value = "Exported Document")]
    pub title: String,

    /// Author of the PDF document
    #[arg(short = 'a', long)]
    pub author: Option<String>,

    /// Page width in millimeters (default: 210mm for A4)
    #[arg(long, default_value = "210.0")]
    pub width: f64,

    /// Page height in millimeters (default: 297mm for A4)
    #[arg(long, default_value = "297.0")]
    pub height: f64,
}

pub fn run_pdf(args: PdfArgs) -> Result<()> {
    // Ensure output directory exists
    if let Some(parent) = args.output.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| crate::error::ExporterError::Io(e))?;
    }

    // Determine content source
    let content = if let Some(input_path) = &args.input {
        // Read from input file
        std::fs::read_to_string(input_path)
            .map_err(|e| crate::error::ExporterError::Io(e))?
    } else if let Some(text) = &args.text {
        // Use provided text
        text.clone()
    } else {
        return Err(crate::error::ExporterError::Config(
            "Either --input or --text must be provided".to_string(),
        ));
    };

    // Create PDF export configuration
    let config = PdfExportConfig {
        page_width_mm: args.width,
        page_height_mm: args.height,
        title: args.title.clone(),
        author: args.author.clone(),
    };

    // Create exporter and generate PDF
    let exporter = PdfExporter::new(config);
    exporter.export_text(&args.output, &content)?;

    log::info!("PDF generated successfully: {}", args.output.display());
    println!("✅ PDF generated: {}", args.output.display());

    Ok(())
}

