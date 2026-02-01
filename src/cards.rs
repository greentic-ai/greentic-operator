use anyhow::Result;

/// Lightweight renderer that used to downgrade message_card payloads.
#[derive(Clone, Default)]
pub struct CardRenderer;

/// Outcome from attempting to render a provider card.
pub struct RenderOutcome {
    pub bytes: Vec<u8>,
}

impl CardRenderer {
    /// Create a no-op renderer.
    pub fn new() -> Self {
        Self
    }

    /// No-op; always return the original payload.
    pub fn render_if_needed(
        &self,
        _provider_type: &str,
        payload_bytes: &[u8],
    ) -> Result<RenderOutcome> {
        Ok(RenderOutcome {
            bytes: payload_bytes.to_vec(),
        })
    }
}
