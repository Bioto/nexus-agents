pub mod openai_responses;

pub use openai_responses::{ResponsesClient, UploadedPdf};

use crate::models::{ChatCompletionRequest, ChatCompletionResponse, ChatCompletionChunk};
use crate::models::Result;
use async_trait::async_trait;
use std::pin::Pin;
use tokio_stream::Stream;


#[async_trait]
pub trait LLMClient {
    async fn chat(&self, request: ChatCompletionRequest) -> Result<ChatCompletionResponse>;
    async fn chat_stream(&self, request: ChatCompletionRequest) -> Result<Pin<Box<dyn Stream<Item = Result<ChatCompletionChunk>> + Send>>>;
    async fn upload_pdf(&self, file_path: &std::path::Path) -> Result<UploadedPdf>;
}