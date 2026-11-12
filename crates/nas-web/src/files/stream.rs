//! Streaming asíncrono de archivos, archivos temporales auto-limpiables y rangos HTTP (RFC 9110).

use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_core::Stream;
use tokio::io::{AsyncRead, ReadBuf};

/// Stream asíncrono sobre un lector con buffer constante O(1) de 64 KiB.
pub struct ReaderStream<R> {
    reader: R,
    buf: Box<[u8]>,
    done: bool,
}

impl<R> std::fmt::Debug for ReaderStream<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReaderStream")
            .field("buf_len", &self.buf.len())
            .field("done", &self.done)
            .finish()
    }
}

impl<R> ReaderStream<R> {
    /// Crea un nuevo stream con tamaño de búfer de 64 KiB.
    pub fn new(reader: R) -> Self {
        Self::with_capacity(reader, 64 * 1024)
    }

    /// Crea un nuevo stream especificando el tamaño del búfer.
    pub fn with_capacity(reader: R, capacity: usize) -> Self {
        Self {
            reader,
            buf: vec![0u8; capacity].into_boxed_slice(),
            done: false,
        }
    }
}

impl<R: AsyncRead + Unpin> Stream for ReaderStream<R> {
    type Item = std::io::Result<axum::body::Bytes>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;
        if this.done {
            return Poll::Ready(None);
        }
        let mut read_buf = ReadBuf::new(&mut this.buf);
        match Pin::new(&mut this.reader).poll_read(cx, &mut read_buf) {
            Poll::Ready(Ok(())) => {
                let filled = read_buf.filled();
                if filled.is_empty() {
                    this.done = true;
                    Poll::Ready(None)
                } else {
                    Poll::Ready(Some(Ok(axum::body::Bytes::copy_from_slice(filled))))
                }
            }
            Poll::Ready(Err(e)) => {
                this.done = true;
                Poll::Ready(Some(Err(e)))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Archivo temporal que se elimina automáticamente al destruirse (`Drop`).
#[derive(Debug)]
pub struct TempFile {
    path: PathBuf,
}

impl TempFile {
    /// Envuelve una ruta temporal para borrado garantizado.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Ruta del archivo temporal.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Stream que sirve un archivo temporal en bloques constantes O(1) y lo elimina
/// de disco automáticamente al completarse o abortarse la conexión.
pub struct CleaningReaderStream {
    stream: ReaderStream<tokio::fs::File>,
    _guard: TempFile,
}

impl std::fmt::Debug for CleaningReaderStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CleaningReaderStream")
            .field("path", &self._guard.path())
            .finish()
    }
}

impl CleaningReaderStream {
    /// Crea el stream envolviendo el descriptor abierto y la guarda temporal.
    pub fn new(file: tokio::fs::File, guard: TempFile) -> Self {
        Self {
            stream: ReaderStream::new(file),
            _guard: guard,
        }
    }
}

impl Stream for CleaningReaderStream {
    type Item = std::io::Result<axum::body::Bytes>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.stream).poll_next(cx)
    }
}

/// Rango de bytes solicitado según HTTP Range (RFC 9110).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    /// Desplazamiento inicial inclusivo (0-indexed).
    pub start: u64,
    /// Desplazamiento final inclusivo (0-indexed).
    pub end: u64,
    /// Tamaño total del archivo en bytes.
    pub total: u64,
}

impl ByteRange {
    /// Longitud en bytes del rango solicitado.
    pub fn length(&self) -> u64 {
        self.end - self.start + 1
    }

    /// Cabecera `Content-Range: bytes start-end/total`.
    pub fn content_range_header(&self) -> String {
        format!("bytes {}-{}/{}", self.start, self.end, self.total)
    }
}

/// Error al interpretar la cabecera HTTP Range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeError {
    /// Sintaxis no reconocida o malformada (debe ignorarse según RFC 9110 y responder 200).
    Malformed,
    /// Rango insatisfactorio (debe responder 416 Range Not Satisfiable).
    Unsatisfiable,
}

/// Analiza la cabecera HTTP `Range: bytes=...` contra el tamaño total del archivo.
pub fn parse_range(header_val: &str, total_size: u64) -> Result<Option<ByteRange>, RangeError> {
    let trimmed = header_val.trim();
    let Some(range_spec) = trimmed.strip_prefix("bytes=") else {
        return Ok(None);
    };

    // Según RFC 9110 sección 14.2, si se solicitan múltiples rangos (separados por coma)
    // y el servidor no genera multipart/byteranges, DEBE ignorar la cabecera Range y
    // responder 200 OK con la representación completa (retornar Ok(None)).
    if range_spec.contains(',') {
        return Ok(None);
    }

    let single_range = range_spec.trim();
    if single_range.is_empty() {
        return Err(RangeError::Malformed);
    }

    if total_size == 0 {
        return Err(RangeError::Unsatisfiable);
    }

    if let Some((start_str, end_str)) = single_range.split_once('-') {
        let start_str = start_str.trim();
        let end_str = end_str.trim();

        if start_str.is_empty() && end_str.is_empty() {
            return Err(RangeError::Malformed);
        }

        if start_str.is_empty() {
            // Caso sufijo: bytes=-500 (los últimos 500 bytes)
            let suffix_len: u64 = end_str.parse().map_err(|_| RangeError::Malformed)?;
            if suffix_len == 0 {
                return Err(RangeError::Unsatisfiable);
            }
            let length = suffix_len.min(total_size);
            let start = total_size - length;
            let end = total_size - 1;
            return Ok(Some(ByteRange {
                start,
                end,
                total: total_size,
            }));
        }

        let start: u64 = start_str.parse().map_err(|_| RangeError::Malformed)?;
        if start >= total_size {
            return Err(RangeError::Unsatisfiable);
        }

        let end = if end_str.is_empty() {
            // Caso abierto: bytes=500- (desde 500 hasta el final)
            total_size - 1
        } else {
            // Caso cerrado: bytes=500-999
            let explicit_end: u64 = end_str.parse().map_err(|_| RangeError::Malformed)?;
            if explicit_end < start {
                return Err(RangeError::Malformed);
            }
            explicit_end.min(total_size - 1)
        };

        Ok(Some(ByteRange {
            start,
            end,
            total: total_size,
        }))
    } else {
        Err(RangeError::Malformed)
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_range, ByteRange, RangeError};

    #[test]
    fn analiza_cabeceras_http_range() {
        // Rango cerrado normal
        let r = parse_range("bytes=0-499", 1000).expect("ok").expect("some");
        assert_eq!(
            r,
            ByteRange {
                start: 0,
                end: 499,
                total: 1000
            }
        );
        assert_eq!(r.length(), 500);
        assert_eq!(r.content_range_header(), "bytes 0-499/1000");

        // Rango abierto hasta el final
        let r = parse_range("bytes=500-", 1000).expect("ok").expect("some");
        assert_eq!(
            r,
            ByteRange {
                start: 500,
                end: 999,
                total: 1000
            }
        );
        assert_eq!(r.length(), 500);

        // Sufijo (últimos 200 bytes)
        let r = parse_range("bytes=-200", 1000).expect("ok").expect("some");
        assert_eq!(
            r,
            ByteRange {
                start: 800,
                end: 999,
                total: 1000
            }
        );
        assert_eq!(r.length(), 200);

        // Sufijo mayor que el tamaño total
        let r = parse_range("bytes=-1500", 1000).expect("ok").expect("some");
        assert_eq!(
            r,
            ByteRange {
                start: 0,
                end: 999,
                total: 1000
            }
        );

        // Múltiples rangos se ignoran según RFC 9110 (None -> 200)
        assert_eq!(parse_range("bytes=0-10, 20-30", 1000), Ok(None));

        // Sin prefijo bytes= se ignora (None -> 200)
        assert_eq!(parse_range("none", 1000), Ok(None));

        // Rango insatisfactorio (start >= total)
        assert_eq!(
            parse_range("bytes=1000-", 1000),
            Err(RangeError::Unsatisfiable)
        );
        assert_eq!(
            parse_range("bytes=1500-2000", 1000),
            Err(RangeError::Unsatisfiable)
        );

        // Archivo vacío es insatisfactorio
        assert_eq!(parse_range("bytes=0-10", 0), Err(RangeError::Unsatisfiable));

        // Rango invertido es malformado
        assert_eq!(
            parse_range("bytes=500-200", 1000),
            Err(RangeError::Malformed)
        );
        assert_eq!(
            parse_range("bytes=abc-def", 1000),
            Err(RangeError::Malformed)
        );
    }
}
