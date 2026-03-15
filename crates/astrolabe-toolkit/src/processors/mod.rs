//! Concrete processor implementations for different artifact types.
//!
//! Each processor handles a specific source type and transforms
//! [`RawArtifact`]s into structured [`ProcessorResult`]s.

pub mod arxiv_url;
pub mod arxiv_pdf;
pub mod pdf_generic;
pub mod web_article;
pub mod x_post;

use crate::processor::{ArtifactProcessor, ProcessorResult, ProcessorError, RawArtifact};

/// Registry of all available processors.
pub struct ProcessorRegistry {
    processors: Vec<Box<dyn ArtifactProcessor + Send + Sync>>,
}

impl ProcessorRegistry {
    /// Create a registry with all built-in processors.
    pub fn new() -> Self {
        Self {
            processors: vec![
                Box::new(arxiv_url::ArxivUrlProcessor::new()),
                Box::new(arxiv_pdf::ArxivPdfProcessor::new()),
                Box::new(pdf_generic::PdfGenericProcessor::new()),
                Box::new(web_article::WebArticleProcessor::new()),
                Box::new(x_post::XPostProcessor::new()),
            ],
        }
    }

    /// Find the first processor that can handle this artifact.
    pub fn find_processor(&self, artifact: &RawArtifact) -> Option<&(dyn ArtifactProcessor + Send + Sync)> {
        self.processors
            .iter()
            .find(|p| p.can_process(artifact))
            .map(|p| p.as_ref())
    }

    /// Process an artifact using the appropriate processor.
    pub fn process(&self, artifact: &RawArtifact) -> Result<ProcessorResult, ProcessorError> {
        let processor = self.find_processor(artifact).ok_or_else(|| {
            ProcessorError::UnsupportedType(artifact.source_type.clone())
        })?;
        processor.process(artifact)
    }
}

impl Default for ProcessorRegistry {
    fn default() -> Self {
        Self::new()
    }
}
