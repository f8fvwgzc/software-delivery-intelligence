//! Local, deterministic text embeddings.
//!
//! Feature hashing over word unigrams, word bigrams and character trigrams,
//! L2-normalised. It costs nothing, needs no network and is good enough to
//! rank "which of these looks like that". Swap `embed` for a model-backed
//! implementation later; the stored vectors only need the same dimension.

/// Dimension of every stored vector. Must match the `vector(N)` columns.
pub const EMBEDDING_DIM: usize = 256;

const UNIGRAM_WEIGHT: f32 = 1.0;
const BIGRAM_WEIGHT: f32 = 0.6;
const TRIGRAM_WEIGHT: f32 = 0.25;

pub fn embed(text: &str) -> Vec<f32> {
    let mut vector = vec![0.0_f32; EMBEDDING_DIM];
    let tokens = tokenize(text);

    for token in &tokens {
        add(&mut vector, token.as_bytes(), UNIGRAM_WEIGHT);

        let chars: Vec<char> = token.chars().collect();
        for window in chars.windows(3) {
            let gram: String = window.iter().collect();
            add(&mut vector, gram.as_bytes(), TRIGRAM_WEIGHT);
        }
    }
    for pair in tokens.windows(2) {
        let gram = format!("{} {}", pair[0], pair[1]);
        add(&mut vector, gram.as_bytes(), BIGRAM_WEIGHT);
    }

    normalize(&mut vector);
    vector
}

/// Cosine similarity of two normalised vectors, clamped to `0.0..=1.0`.
pub fn cosine(a: &[f32], b: &[f32]) -> f64 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    f64::from(dot).clamp(0.0, 1.0)
}

/// pgvector text literal, e.g. `[0.1,0.2]`. Bind it as text and cast with `::vector`.
pub fn to_pgvector(vector: &[f32]) -> String {
    let mut literal = String::with_capacity(vector.len() * 8 + 2);
    literal.push('[');
    for (index, value) in vector.iter().enumerate() {
        if index > 0 {
            literal.push(',');
        }
        literal.push_str(&format!("{value:.6}"));
    }
    literal.push(']');
    literal
}

fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|token| token.len() > 1)
        .map(str::to_lowercase)
        .collect()
}

/// Signed feature hashing: the sign bit keeps collisions from only ever adding up.
fn add(vector: &mut [f32], feature: &[u8], weight: f32) {
    let hash = fnv1a(feature);
    let index = (hash % EMBEDDING_DIM as u64) as usize;
    let sign = if (hash >> 63) == 0 { 1.0 } else { -1.0 };
    vector[index] += sign * weight;
}

fn fnv1a(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    bytes
        .iter()
        .fold(OFFSET, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(PRIME))
}

fn normalize(vector: &mut [f32]) {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > f32::EPSILON {
        for value in vector.iter_mut() {
            *value /= norm;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn similar_text_scores_higher_than_unrelated_text() {
        let base = embed("React frontend with a Rust axum API and PostgreSQL deployed with Docker");
        let near = embed("Rust axum backend, PostgreSQL database, React client, Docker images");
        let far = embed("Quarterly marketing budget spreadsheet for the sales team");

        assert!(cosine(&base, &near) > cosine(&base, &far));
    }

    #[test]
    fn vectors_are_normalised_and_sized() {
        let vector = embed("terraform azure kubernetes");
        let norm: f32 = vector.iter().map(|v| v * v).sum::<f32>().sqrt();

        assert_eq!(vector.len(), EMBEDDING_DIM);
        assert!((norm - 1.0).abs() < 1e-4);
    }

    #[test]
    fn empty_text_is_a_zero_vector() {
        assert!(embed("").iter().all(|v| *v == 0.0));
    }
}
