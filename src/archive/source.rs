use std::{
    fs::File,
    io::{self, BufReader, Cursor},
    sync::Arc,
};

pub trait RandomAccessRead: Send + Sync {
    fn len(&self) -> io::Result<u64>;
    fn read_exact_at(&self, offset: u64, dst: &mut [u8]) -> io::Result<()>;

    fn is_empty(&self) -> io::Result<bool> {
        Ok(self.len()? == 0)
    }
}

impl RandomAccessRead for File {
    fn len(&self) -> io::Result<u64> {
        Ok(self.metadata()?.len())
    }

    fn read_exact_at(&self, mut offset: u64, mut dst: &mut [u8]) -> io::Result<()> {
        while !dst.is_empty() {
            let read = match read_at(self, dst, offset) {
                Ok(read) => read,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            };
            if read == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "failed to fill whole buffer",
                ));
            }
            offset = offset
                .checked_add(
                    u64::try_from(read).map_err(|_| io::Error::other("read size overflow"))?,
                )
                .ok_or_else(|| io::Error::other("read offset overflow"))?;
            dst = &mut dst[read..];
        }
        Ok(())
    }
}

#[cfg(unix)]
fn read_at(file: &File, dst: &mut [u8], offset: u64) -> io::Result<usize> {
    use std::os::unix::fs::FileExt;
    file.read_at(dst, offset)
}

#[cfg(windows)]
fn read_at(file: &File, dst: &mut [u8], offset: u64) -> io::Result<usize> {
    use std::os::windows::fs::FileExt;
    file.seek_read(dst, offset)
}

impl RandomAccessRead for BufReader<File> {
    fn len(&self) -> io::Result<u64> {
        RandomAccessRead::len(self.get_ref())
    }

    fn read_exact_at(&self, offset: u64, dst: &mut [u8]) -> io::Result<()> {
        RandomAccessRead::read_exact_at(self.get_ref(), offset, dst)
    }
}

impl<T> RandomAccessRead for Cursor<T>
where
    T: AsRef<[u8]> + Send + Sync,
{
    fn len(&self) -> io::Result<u64> {
        u64::try_from(self.get_ref().as_ref().len())
            .map_err(|_| io::Error::other("source length overflow"))
    }

    fn read_exact_at(&self, offset: u64, dst: &mut [u8]) -> io::Result<()> {
        let start = usize::try_from(offset)
            .map_err(|_| io::Error::new(io::ErrorKind::UnexpectedEof, "read offset too large"))?;
        let end = start
            .checked_add(dst.len())
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "read range overflow"))?;
        let source = self.get_ref().as_ref().get(start..end).ok_or_else(|| {
            io::Error::new(io::ErrorKind::UnexpectedEof, "failed to fill whole buffer")
        })?;
        dst.copy_from_slice(source);
        Ok(())
    }
}

impl RandomAccessRead for &[u8] {
    fn len(&self) -> io::Result<u64> {
        u64::try_from(<[u8]>::len(self)).map_err(|_| io::Error::other("source length overflow"))
    }

    fn read_exact_at(&self, offset: u64, dst: &mut [u8]) -> io::Result<()> {
        read_slice_exact(self, offset, dst)
    }
}

impl RandomAccessRead for Arc<[u8]> {
    fn len(&self) -> io::Result<u64> {
        u64::try_from(self.as_ref().len()).map_err(|_| io::Error::other("source length overflow"))
    }

    fn read_exact_at(&self, offset: u64, dst: &mut [u8]) -> io::Result<()> {
        read_slice_exact(self, offset, dst)
    }
}

fn read_slice_exact(source: &[u8], offset: u64, dst: &mut [u8]) -> io::Result<()> {
    let start = usize::try_from(offset)
        .map_err(|_| io::Error::new(io::ErrorKind::UnexpectedEof, "read offset too large"))?;
    let end = start
        .checked_add(dst.len())
        .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "read range overflow"))?;
    let bytes = source.get(start..end).ok_or_else(|| {
        io::Error::new(io::ErrorKind::UnexpectedEof, "failed to fill whole buffer")
    })?;
    dst.copy_from_slice(bytes);
    Ok(())
}
