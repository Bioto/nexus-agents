//! Code generation for MCP tools.
//!
//! This module provides functionality to generate Python client code
//! for interacting with MCP servers.

mod generator;
mod python_templates;
mod schema_converter;

pub use generator::CodeGenerator;
pub use schema_converter::{SchemaConverter, SchemaError};
