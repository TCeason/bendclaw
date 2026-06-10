//! Code-aware ranking signals applied on top of the BM25 base score.
//!
//! These cheap heuristics recover most of the quality a semantic embedder
//! would add, without any model: definition boosts, identifier-stem matching,
//! and noise penalties for test/legacy/declaration files.

use super::chunk::split_identifier;
use super::chunk::Chunk;

/// Boost for query stems that match the chunk's defining symbol stems.
/// Rewards `parse config` -> a chunk that defines `parseConfig` / `ConfigParser`.
pub fn stem_boost(query_stems: &[String], chunk: &Chunk) -> f32 {
    let Some(name) = &chunk.defines else {
        return 0.0;
    };
    let def_stems = split_identifier(name);
    let matches = query_stems
        .iter()
        .filter(|qs| {
            def_stems
                .iter()
                .any(|ds| ds.starts_with(qs.as_str()) || qs.starts_with(ds.as_str()))
        })
        .count();
    matches as f32 * 0.5
}

/// Boost a chunk that *defines* a symbol named like the query (vs. merely using it).
pub fn definition_boost(query_stems: &[String], chunk: &Chunk) -> f32 {
    let Some(name) = &chunk.defines else {
        return 0.0;
    };
    let def_stems = split_identifier(name);
    if def_stems.is_empty() {
        return 0.0;
    }
    // All query stems are covered by the definition name -> strong signal.
    let all_covered = query_stems
        .iter()
        .all(|qs| def_stems.iter().any(|ds| ds.contains(qs.as_str())));
    if all_covered && !query_stems.is_empty() {
        2.0
    } else {
        // It's a definition at least — small baseline boost.
        0.3
    }
}

/// Penalize low-signal files so canonical implementations surface first.
pub fn noise_penalty(file_path: &str) -> f32 {
    let p = file_path.to_lowercase();
    if p.ends_with(".d.ts") || p.ends_with(".min.js") {
        return -2.0;
    }
    if p.contains("/test")
        || p.contains("test/")
        || p.contains("_test.")
        || p.contains(".test.")
        || p.contains("/tests/")
        || p.contains("/spec/")
        || p.contains("_spec.")
    {
        return -1.5;
    }
    if p.contains("/compat")
        || p.contains("/legacy")
        || p.contains("/vendor/")
        || p.contains("/examples/")
        || p.contains("/example/")
    {
        return -0.8;
    }
    0.0
}
