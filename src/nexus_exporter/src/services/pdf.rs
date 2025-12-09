use crate::error::{ExporterError, Result};
use printpdf::*;
use std::fs::File;
use std::io::BufWriter;

/// Configuration for PDF export operations.
#[derive(Debug, Clone)]
pub struct PdfExportConfig {
    /// Page width in millimeters (default: A4 width = 210mm)
    pub page_width_mm: f64,
    /// Page height in millimeters (default: A4 height = 297mm)
    pub page_height_mm: f64,
    /// Title of the document
    pub title: String,
    /// Author of the document
    pub author: Option<String>,
}

impl Default for PdfExportConfig {
    fn default() -> Self {
        Self {
            page_width_mm: 210.0,  // A4 width
            page_height_mm: 297.0, // A4 height
            title: "Exported Document".to_string(),
            author: None,
        }
    }
}

/// PDF exporter service for generating PDF documents.
pub struct PdfExporter {
    config: PdfExportConfig,
}

impl PdfExporter {
    /// Create a new PDF exporter with the given configuration.
    pub fn new(config: PdfExportConfig) -> Self {
        Self { config }
    }

    /// Create a new PDF exporter with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(PdfExportConfig::default())
    }

    /// Generate a PDF file from text content.
    ///
    /// # Arguments
    /// * `output_path` - Path where the PDF file will be saved
    /// * `content` - Text content to include in the PDF
    ///
    /// # Errors
    /// Returns an error if file creation or PDF generation fails.
    pub fn export_text(&self, output_path: &std::path::Path, content: &str) -> Result<()> {
        // Create PDF document
        let (mut doc, page, layer) = PdfDocument::new(
            &self.config.title,
            Mm(self.config.page_width_mm as f32),
            Mm(self.config.page_height_mm as f32),
            "Layer 1",
        );

        // Add document metadata (chain the methods since they return Self)
        if let Some(author) = &self.config.author {
            doc = doc.with_author(author.clone());
        }
        doc = doc.with_title(&self.config.title);

        // Get the actual layer object
        let current_layer = doc.get_page(page).get_layer(layer);

        // Add Helvetica font
        let font = doc
            .add_builtin_font(BuiltinFont::Helvetica)
            .map_err(|e| ExporterError::Pdf(format!("Failed to add font: {}", e)))?;

        // Split content into lines and add to PDF
        let lines: Vec<&str> = content.lines().collect();
        let line_height = 12.0;
        let margin = 20.0;
        let mut y_position = (self.config.page_height_mm - margin) as f32;

        for line in lines {
            if y_position < margin as f32 {
                // Would need to add a new page here if we want multi-page support
                break;
            }

            current_layer.use_text(line, line_height, Mm(margin as f32), Mm(y_position), &font);

            y_position -= line_height * 1.5;
        }

        // Save PDF to file
        let file = File::create(output_path).map_err(|e| ExporterError::Io(e))?;
        let mut writer = BufWriter::new(file);
        doc.save(&mut writer)
            .map_err(|e| ExporterError::Pdf(format!("Failed to save PDF: {}", e)))?;

        Ok(())
    }

    /// Generate a PDF file from structured data (JSON).
    ///
    /// # Arguments
    /// * `output_path` - Path where the PDF file will be saved
    /// * `data` - JSON data to format and include in the PDF
    ///
    /// # Errors
    /// Returns an error if file creation, JSON parsing, or PDF generation fails.
    pub fn export_json(
        &self,
        output_path: &std::path::Path,
        data: &serde_json::Value,
    ) -> Result<()> {
        // Format JSON as pretty-printed text
        let formatted = serde_json::to_string_pretty(data)
            .map_err(|e| ExporterError::Other(format!("Failed to format JSON: {}", e)))?;

        self.export_text(output_path, &formatted)
    }
}
