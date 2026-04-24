//! Text chunking utilities for streaming TTS.
//!
//! Provides intelligent text segmentation optimized for
//! real-time streaming with minimal time-to-first-audio.

/// Split text into chunks optimized for streaming.
///
/// The first chunk is split at the earliest clause boundary within
/// `first_max` characters to minimize time-to-first-audio.
/// Remaining text uses normal sentence-level chunking with `rest_max`
/// as the size limit.
///
/// # Arguments
/// * `text` - Input text to chunk
/// * `first_max` - Maximum size for first chunk (default: 100)
/// * `rest_max` - Maximum size for remaining chunks (default: 400)
///
/// # Returns
/// Vector of text chunks in order
///
/// # Example
/// ```rust
/// let text = "Hello, world! This is a test. And more text here.";
/// let chunks = chunk_text_streaming(text, 100, 400);
/// assert!(chunks.len() > 1);
/// ```
pub fn chunk_text_streaming(text: &str, first_max: usize, rest_max: usize) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }

    // If text fits in first chunk, return as single chunk
    if text.len() <= first_max {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();

    // Find best split point for first chunk
    let first_chunk_end = find_early_clause_boundary(text, first_max);

    // Extract first chunk
    chunks.push(text[..first_chunk_end].trim().to_string());

    // Process remaining text with sentence-level chunking
    let remaining = &text[first_chunk_end..];
    chunks.extend(chunk_sentences(remaining.trim(), rest_max));

    chunks
}

/// Find the earliest clause boundary within max_length.
///
/// Clause boundaries are: ，。；：！？,.:!; etc.
/// This minimizes time-to-first-audio by sending a small,
/// natural chunk as quickly as possible.
fn find_early_clause_boundary(text: &str, max_length: usize) -> usize {
    let delimiters = ['，', '。', '；', '：', '！', '？', ',', '.', ';', ':', '!', '?'];

    // Search for delimiter in reverse from max_length
    let search_end = max_length.min(text.len());
    for i in (0..search_end).rev() {
        if let Some(ch) = text.chars().nth(i) {
            if delimiters.contains(&ch) {
                // Return position after the delimiter
                return text.char_indices().nth(i + 1)
                    .map(|(pos, _)| pos)
                    .unwrap_or(search_end);
            }
        }
    }

    // No delimiter found, fall back to word boundary or hard limit
    find_word_boundary(text, search_end)
}

/// Find word boundary (space) near the target position.
fn find_word_boundary(text: &str, target_pos: usize) -> usize {
    // Search backwards for space from target position
    for i in (0..target_pos).rev() {
        if let Some(ch) = text.chars().nth(i) {
            if ch.is_whitespace() {
                return text.char_indices().nth(i)
                    .map(|(pos, _)| pos)
                    .unwrap_or(target_pos);
            }
        }
    }

    // Hard limit fallback
    target_pos
}

/// Chunk remaining text into sentence-level chunks.
fn chunk_sentences(text: &str, max_size: usize) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }

    if text.len() <= max_size {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        let chunk_end = find_sentence_end(remaining, max_size);
        chunks.push(remaining[..chunk_end].trim().to_string());
        remaining = remaining[chunk_end..].trim();
    }

    chunks
}

/// Find the end of a sentence within max_size.
fn find_sentence_end(text: &str, max_size: usize) -> usize {
    let sentence_delimiters = ['。', '！', '？', '.', '!', '?'];

    let search_end = max_size.min(text.len());

    // Look for sentence delimiter
    for i in 0..search_end {
        if let Some(ch) = text.chars().nth(i) {
            if sentence_delimiters.contains(&ch) {
                return text.char_indices().nth(i + 1)
                    .map(|(pos, _)| pos)
                    .unwrap_or(search_end);
            }
        }
    }

    // No sentence end found, use word boundary
    find_word_boundary(text, search_end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_text_short() {
        let text = "Hello, world!";
        let chunks = chunk_text_streaming(text, 100, 400);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], text);
    }

    #[test]
    fn test_chunk_text_with_delimiters() {
        let text = "Hello, world! This is a test. And more.";
        let chunks = chunk_text_streaming(text, 10, 20);
        // Should split at first delimiter
        assert!(chunks.len() > 1);
        assert!(chunks[0].ends_with('!') || chunks[0].ends_with('，'));
    }

    #[test]
    fn test_chunk_text_long() {
        let text = "Hello, world! This is a very long text that should be split into multiple chunks. And here is more text to ensure multiple chunks.";
        let chunks = chunk_text_streaming(text, 20, 50);
        assert!(chunks.len() > 1);
        // Verify no empty chunks
        for chunk in &chunks {
            assert!(!chunk.trim().is_empty());
        }
    }

    #[test]
    fn test_find_early_clause_boundary() {
        let text = "Hello, world! This is a test";
        let pos = find_early_clause_boundary(text, 15);
        assert!(pos < text.len());
        // Should split at comma or exclamation
        assert!(text[..pos].contains(',') || text[..pos].contains('!'));
    }
}