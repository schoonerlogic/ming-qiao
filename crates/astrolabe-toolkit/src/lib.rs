//! ASTROLABE Artifact Processing Toolkit
//!
//! Rust implementation of the artifact ingest pipeline and processors
//! for the ASTROLABE knowledge graph. Replaces the Python prototype
//! (`oracle/main/scripts/` and `oracle/main/processors/`).
//!
//! # Modules
//!
//! - [`processor`] — Core traits and types: `ArtifactProcessor`, `RawArtifact`, `ProcessorResult`
//! - [`dedup`] — Hash-based deduplication engine (SHA256, arXiv ID, URL)
//! - [`envelope`] — Envelope builder for quarantine and ASTROLABE ingestion

pub mod dedup;
pub mod envelope;
pub mod ingest;
pub mod processor;
pub mod processors;
