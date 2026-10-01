use crate::{FileStore, write_atomic};
use babble_types::{Error, Hash, Result};
use std::{
    fs::File,
    io::{ErrorKind, Read},
};

#[derive(Debug, thiserror::Error)]
pub enum BlobReadError {
    #[error("blob exceeds buffered read limit of {max_bytes} bytes")]
    TooLarge { max_bytes: usize },
    #[error(transparent)]
    Storage(#[from] Error),
}

impl FileStore {
    pub fn put_blob(&self, bytes: &[u8]) -> Result<Hash> {
        let hash = Hash::from_bytes(bytes);
        write_atomic(&self.blob_path(&hash)?, bytes)?;
        Ok(hash)
    }

    /// Trusted native callers choose their own resource policy. Network callers
    /// must use get_blob_bounded before buffering a response.
    pub fn get_blob(&self, hash: &Hash) -> Result<Option<Vec<u8>>> {
        let Some(mut file) = self.open_blob(hash)? else {
            return Ok(None);
        };
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|error| read_error(hash, error))?;
        verify(hash, &bytes)?;
        Ok(Some(bytes))
    }

    pub fn get_blob_bounded(
        &self,
        hash: &Hash,
        max_bytes: usize,
    ) -> std::result::Result<Option<Vec<u8>>, BlobReadError> {
        let Some(mut file) = self.open_blob(hash)? else {
            return Ok(None);
        };
        // Inspect the opened handle, not a path that could now name another file.
        let length = file
            .metadata()
            .map_err(|error| read_error(hash, error))?
            .len();
        if length > max_bytes as u64 {
            return Err(BlobReadError::TooLarge { max_bytes });
        }
        let bytes = read_bounded(&mut file, max_bytes, hash)?;
        verify(hash, &bytes)?;
        Ok(Some(bytes))
    }

    fn open_blob(&self, hash: &Hash) -> Result<Option<File>> {
        hash.validate()?;
        let file = match File::open(self.blob_path(hash)?) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(read_error(hash, error)),
        };
        if !file
            .metadata()
            .map_err(|error| read_error(hash, error))?
            .is_file()
        {
            return Err(Error::Conflict(format!(
                "blob is not a regular file: {hash}"
            )));
        }
        Ok(Some(file))
    }
}

fn read_bounded(
    reader: &mut impl Read,
    max_bytes: usize,
    hash: &Hash,
) -> std::result::Result<Vec<u8>, BlobReadError> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        // Read at most one byte beyond the limit, including if the file grows
        // after metadata inspection. Oversized data never enters the output Vec.
        let remaining = max_bytes - bytes.len();
        let read_size = buffer.len().min(remaining.saturating_add(1));
        let read = match reader.read(&mut buffer[..read_size]) {
            Ok(read) => read,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => return Err(read_error(hash, error).into()),
        };
        if read == 0 {
            return Ok(bytes);
        }
        if read > remaining {
            return Err(BlobReadError::TooLarge { max_bytes });
        }
        let required = bytes.len() + read;
        if bytes.capacity() < required {
            let capacity = buffer
                .len()
                .max(bytes.capacity().saturating_mul(2))
                .max(required)
                .min(max_bytes);
            bytes
                .try_reserve_exact(capacity - bytes.len())
                .map_err(|_| Error::Conflict("cannot allocate bounded blob response".into()))?;
        }
        bytes.extend_from_slice(&buffer[..read]);
    }
}

fn verify(hash: &Hash, bytes: &[u8]) -> Result<()> {
    if Hash::from_bytes(bytes) != *hash {
        return Err(Error::Conflict(format!("blob integrity mismatch: {hash}")));
    }
    Ok(())
}

fn read_error(hash: &Hash, error: std::io::Error) -> Error {
    Error::Conflict(format!("read blob {hash}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn growing_reader_stops_at_one_byte_past_limit() {
        let hash = Hash::from_bytes(b"ignored");
        let mut reader = std::io::repeat(42).take(1_000_000);
        assert!(matches!(
            read_bounded(&mut reader, 17, &hash),
            Err(BlobReadError::TooLarge { max_bytes: 17 })
        ));
        assert_eq!(reader.limit(), 1_000_000 - 18);
    }

    #[test]
    fn bounded_reader_accepts_exact_and_zero_limits() {
        let hash = Hash::from_bytes(b"bytes");
        assert_eq!(
            read_bounded(&mut &b"bytes"[..], 5, &hash).unwrap(),
            b"bytes"
        );
        assert_eq!(read_bounded(&mut &b""[..], 0, &hash).unwrap(), b"");
        assert!(matches!(
            read_bounded(&mut &b"x"[..], 0, &hash),
            Err(BlobReadError::TooLarge { max_bytes: 0 })
        ));
    }
}
