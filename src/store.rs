//! Content-addressed blob storage in `.dvault/objects/`.
//!
//! Layout matches Git: the first two hex chars of the SHA-256 are a directory,
//! the remaining 62 chars are the filename. Blobs over `COMPRESS_THRESHOLD`
//! bytes are zlib-compressed; the `compressed` flag is recorded per file in the
//! database so retrieval knows whether to inflate.

use anyhow::{Context, Result, bail};
use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Files larger than this (uncompressed) are zlib-compressed on disk.
pub const COMPRESS_THRESHOLD: usize = 100 * 1024;

/// Compute the SHA-256 of `bytes` as a lowercase hex string.
pub fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn object_path(objects_dir: &Path, hash: &str) -> PathBuf {
    let (prefix, rest) = hash.split_at(2);
    objects_dir.join(prefix).join(rest)
}

/// Write `bytes` to the blob store and return `(hash, compressed)`.
///
/// Writing is idempotent: if the object already exists we skip it, since the
/// content hash uniquely identifies the bytes.
pub fn write_blob(objects_dir: &Path, bytes: &[u8]) -> Result<(String, bool)> {
    let hash = hash_bytes(bytes);
    let path = object_path(objects_dir, &hash);
    let compressed = bytes.len() > COMPRESS_THRESHOLD;

    if path.exists() {
        return Ok((hash, compressed));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("could not create object dir: {}", parent.display()))?;
    }

    let payload = if compressed {
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
        enc.write_all(bytes).context("zlib compression failed")?;
        enc.finish().context("zlib compression failed")?
    } else {
        bytes.to_vec()
    };

    std::fs::write(&path, payload)
        .with_context(|| format!("could not write object: {}", path.display()))?;
    Ok((hash, compressed))
}

/// Read a blob back, inflating if it was stored compressed.
pub fn read_blob(objects_dir: &Path, hash: &str, compressed: bool) -> Result<Vec<u8>> {
    let path = object_path(objects_dir, hash);
    let raw =
        std::fs::read(&path).with_context(|| format!("missing object {hash} (corrupt vault?)"))?;
    if !compressed {
        return Ok(raw);
    }
    let mut out = Vec::new();
    ZlibDecoder::new(&raw[..])
        .read_to_end(&mut out)
        .context("zlib decompression failed")?;
    Ok(out)
}

/// Verify that a stored blob's bytes still hash to its key.
#[allow(dead_code)]
pub fn verify_blob(objects_dir: &Path, hash: &str, compressed: bool) -> Result<()> {
    let bytes = read_blob(objects_dir, hash, compressed)?;
    if hash_bytes(&bytes) != hash {
        bail!("object {hash} is corrupt: content hash mismatch");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_objects() -> PathBuf {
        // Unique-ish dir from the test thread; std::env::temp_dir + pid + name.
        let mut dir = std::env::temp_dir();
        dir.push(format!("dvault-test-{}", std::process::id()));
        dir.push(format!("obj-{:?}", std::thread::current().id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn small_blob_roundtrips_uncompressed() {
        let dir = temp_objects();
        let data = b"a small file under the threshold";
        let (hash, compressed) = write_blob(&dir, data).unwrap();
        assert!(!compressed, "small files should not be compressed");
        assert_eq!(read_blob(&dir, &hash, compressed).unwrap(), data);
    }

    #[test]
    fn large_blob_roundtrips_compressed() {
        let dir = temp_objects();
        // Highly compressible payload well over the threshold.
        let data = vec![b'x'; COMPRESS_THRESHOLD + 1];
        let (hash, compressed) = write_blob(&dir, &data).unwrap();
        assert!(compressed, "large files should be compressed");
        // Stored bytes should be smaller than the original.
        let (prefix, rest) = hash.split_at(2);
        let stored = std::fs::read(dir.join(prefix).join(rest)).unwrap();
        assert!(stored.len() < data.len());
        // ...but inflate back to the exact original.
        assert_eq!(read_blob(&dir, &hash, compressed).unwrap(), data);
    }

    #[test]
    fn identical_content_yields_identical_hash() {
        assert_eq!(hash_bytes(b"hello"), hash_bytes(b"hello"));
        assert_ne!(hash_bytes(b"hello"), hash_bytes(b"world"));
    }
}
