//! Embeddings: `multilingual-e5-small` (int8) on the CPU with ONNX Runtime, compiled into the binary.
//! Everything else talks to the `Embedder` trait, so tests and the index run on a fake.

use anyhow::{anyhow, Result};
use ort::{session::Session, value::Tensor};
use tokenizers::Tokenizer;

pub const MODEL_ID: &str = "multilingual-e5-small-int8";
pub const DIM: usize = 384;
/// The model's context limit, special tokens included.
pub const MAX_TOKENS: usize = 512;

static MODEL: &[u8] = include_bytes!("../assets/model.onnx");
static TOKENIZER: &[u8] = include_bytes!("../assets/tokenizer.json");

pub trait Embedder {
    /// Embeds chunks as e5 "passages". Vectors have length `DIM` and unit norm.
    fn embed_passages(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;
    /// Embeds a question or description as an e5 "query".
    fn embed_query(&mut self, text: &str) -> Result<Vec<f32>>;
    /// Length of `text` in model tokens, without special tokens.
    fn count_tokens(&self, text: &str) -> usize;
}

fn ort_err<R>(e: ort::Error<R>) -> anyhow::Error {
    anyhow!("onnx runtime: {e}")
}

pub struct Onnx {
    session: Session,
    tokenizer: Tokenizer,
}

impl Onnx {
    pub fn load() -> Result<Self> {
        let mut tokenizer =
            Tokenizer::from_bytes(TOKENIZER).map_err(|e| anyhow!("tokenizer: {e}"))?;
        tokenizer
            .with_truncation(Some(tokenizers::TruncationParams {
                max_length: MAX_TOKENS,
                ..Default::default()
            }))
            .map_err(|e| anyhow!("tokenizer: {e}"))?;
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
        let env = ort::init().build().map_err(ort_err)?;
        let session = Session::builder(&env)
            .map_err(ort_err)?
            .with_intra_threads(threads)
            .map_err(ort_err)?
            .commit_from_memory(MODEL)
            .map_err(ort_err)?;
        Ok(Self { session, tokenizer })
    }

    fn embed(&mut self, prefix: &str, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let inputs: Vec<String> = texts.iter().map(|t| format!("{prefix}{t}")).collect();
        let encodings = self
            .tokenizer
            .encode_batch(inputs, true)
            .map_err(|e| anyhow!("tokenizer: {e}"))?;
        // Longest first, so a batch holds similar lengths and padding stays small.
        let mut order: Vec<usize> = (0..encodings.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(encodings[i].len()));
        let mut out = vec![Vec::new(); encodings.len()];
        for batch in order.chunks(16) {
            let width = encodings[batch[0]].len();
            let mut ids = vec![1i64; batch.len() * width]; // 1 = <pad>
            let mut mask = vec![0i64; batch.len() * width];
            for (row, &i) in batch.iter().enumerate() {
                for (col, (&id, &m)) in encodings[i]
                    .get_ids()
                    .iter()
                    .zip(encodings[i].get_attention_mask())
                    .enumerate()
                {
                    ids[row * width + col] = id as i64;
                    mask[row * width + col] = m as i64;
                }
            }
            let outputs = self
                .session
                .run(ort::inputs![
                    "input_ids" => Tensor::from_array(([batch.len(), width], ids)).map_err(ort_err)?,
                    "attention_mask" => Tensor::from_array(([batch.len(), width], mask.clone())).map_err(ort_err)?,
                    "token_type_ids" => Tensor::from_array(([batch.len(), width], vec![0i64; batch.len() * width])).map_err(ort_err)?,
                ])
                .map_err(ort_err)?;
            let (shape, hidden) = outputs[0].try_extract_tensor::<f32>().map_err(ort_err)?;
            let dim = shape[2] as usize;
            if dim != DIM {
                return Err(anyhow!("model returned {dim} dimensions, expected {DIM}"));
            }
            for (row, &i) in batch.iter().enumerate() {
                // Mean over the real tokens, then unit length.
                let mut v = vec![0f32; DIM];
                let mut n = 0f32;
                for col in 0..width {
                    if mask[row * width + col] == 0 {
                        continue;
                    }
                    n += 1.0;
                    let tok = &hidden[(row * width + col) * dim..(row * width + col + 1) * dim];
                    for (a, b) in v.iter_mut().zip(tok) {
                        *a += b;
                    }
                }
                for a in &mut v {
                    *a /= n.max(1.0);
                }
                normalize(&mut v);
                out[i] = v;
            }
        }
        Ok(out)
    }
}

impl Embedder for Onnx {
    fn embed_passages(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        self.embed("passage: ", texts)
    }

    fn embed_query(&mut self, text: &str) -> Result<Vec<f32>> {
        Ok(self.embed("query: ", &[text])?.remove(0))
    }

    fn count_tokens(&self, text: &str) -> usize {
        self.tokenizer
            .encode_char_offsets(text, false)
            .map_or(text.len() / 4 + 1, |e| e.len())
    }
}

pub fn normalize(v: &mut [f32]) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v {
            *x /= norm;
        }
    }
}

/// A deterministic stand-in: hashed bag of words. Same words, similar vectors; no model needed.
#[cfg(test)]
pub struct Fake;

#[cfg(test)]
impl Embedder for Fake {
    fn embed_passages(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|t| fake_vector(t)).collect())
    }
    fn embed_query(&mut self, text: &str) -> Result<Vec<f32>> {
        Ok(fake_vector(text))
    }
    fn count_tokens(&self, text: &str) -> usize {
        text.split_whitespace().count()
    }
}

#[cfg(test)]
fn fake_vector(text: &str) -> Vec<f32> {
    use sha2::{Digest, Sha256};
    let mut v = vec![0f32; DIM];
    for word in text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
    {
        let h = Sha256::digest(word.to_lowercase().as_bytes());
        v[(u16::from_le_bytes([h[0], h[1]]) as usize) % DIM] += 1.0;
    }
    normalize(&mut v);
    v
}
