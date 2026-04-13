/// Unified API client that wraps either Anthropic or OpenAI client.
/// Provides the same `create_message_stream` interface regardless of backend.

use std::pin::Pin;
use std::task::{Context, Poll};
use futures::stream::Stream;

use super::client::{AnthropicClient, ApiClientConfig, ApiError, ApiStreamEvent, CreateMessageRequest, SseStream};
use super::openai_client::{OpenAiClient, OpenAiSseStream};

/// Unified stream that wraps either Anthropic SseStream or OpenAI stream.
pub enum UnifiedStream {
    Anthropic(SseStream),
    OpenAi(OpenAiSseStream),
}

impl Stream for UnifiedStream {
    type Item = Result<ApiStreamEvent, ApiError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // SAFETY: we never move the inner stream
        unsafe {
            match self.get_unchecked_mut() {
                UnifiedStream::Anthropic(s) => Pin::new_unchecked(s).poll_next(cx),
                UnifiedStream::OpenAi(s) => Pin::new_unchecked(s).poll_next(cx),
            }
        }
    }
}

/// Unified API client.
pub enum UnifiedClient {
    Anthropic(AnthropicClient),
    OpenAi(OpenAiClient),
}

impl UnifiedClient {
    pub fn new_anthropic(config: ApiClientConfig) -> Self {
        Self::Anthropic(AnthropicClient::new(config))
    }

    pub fn new_openai(config: ApiClientConfig) -> Self {
        Self::OpenAi(OpenAiClient::new(config))
    }

    pub async fn create_message_stream(
        &self,
        request: CreateMessageRequest,
    ) -> Result<UnifiedStream, ApiError> {
        match self {
            Self::Anthropic(c) => {
                let stream = c.create_message_stream(request).await?;
                Ok(UnifiedStream::Anthropic(stream))
            }
            Self::OpenAi(c) => {
                let stream = c.create_message_stream(request).await?;
                Ok(UnifiedStream::OpenAi(stream))
            }
        }
    }
}
