use atomicwrites;
use common::make_io_err;
use directory::Directory;
use directory::error::{IOError, OpenWriteError, OpenReadError, DeleteError, OpenDirectoryError};
use directory::ReadOnlySource;
use directory::shared_vec_slice::SharedVecSlice;
use directory::WritePtr;
use fst::raw::MmapReadOnly;
use memmap::{Mmap, Protection};
use std::collections::hash_map::Entry as HashMapEntry;
use std::collections::HashMap;
use std::convert::From;
use std::fmt;
use std::fs::{self, File};
use std::fs::OpenOptions;
use std::io::{self, Seek, SeekFrom};
use std::io::{BufWriter, Read, Write};
use std::mem;
use std::path::{Path, PathBuf};
use std::result;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::Weak;
use tempdir::TempDir;
use rusoto_core::{DefaultCredentialsProvider, Region, default_tls_client};
use rusoto_s3::{S3, S3Client, HeadBucketRequest};

fn open_mmap(full_path: &PathBuf) -> result::Result<Option<Arc<Mmap>>, OpenReadError> {
    let file = File::open(&full_path).map_err(|e| if e.kind() ==
        io::ErrorKind::NotFound
    {
        OpenReadError::FileDoesNotExist(full_path.clone())
    } else {
        OpenReadError::IOError(IOError::with_path(full_path.to_owned(), e))
    })?;

    let meta_data = file.metadata().map_err(|e| {
        IOError::with_path(full_path.to_owned(), e)
    })?;
    if meta_data.len() == 0 {
        // if the file size is 0, it will not be possible
        // to mmap the file, so we return an anonymous mmap_cache
        // instead.
        return Ok(None);
    }
    match Mmap::open(&file, Protection::Read) {
        Ok(mmap) => Ok(Some(Arc::new(mmap))),
        Err(e) => Err(IOError::with_path(full_path.to_owned(), e))?,
    }

}

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct CacheCounters {
    // Number of time the cache prevents to call `mmap`
    pub hit: usize,
    // Number of time tantivy had to call `mmap`
    // as no entry was in the cache.
    pub miss_empty: usize,
    // Number of time tantivy had to call `mmap`
    // as the entry in the cache was evinced.
    pub miss_weak: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CacheInfo {
    pub counters: CacheCounters,
    pub mmapped: Vec<PathBuf>,
}

struct MmapCache {
    counters: CacheCounters,
    cache: HashMap<PathBuf, Weak<Mmap>>,
    purge_weak_limit: usize,
}

const STARTING_PURGE_WEAK_LIMIT: usize = 1_000;

impl Default for MmapCache {
    fn default() -> MmapCache {
        MmapCache {
            counters: CacheCounters::default(),
            cache: HashMap::new(),
            purge_weak_limit: STARTING_PURGE_WEAK_LIMIT,
        }
    }
}


impl MmapCache {
    fn cleanup(&mut self) {
        let previous_cache_size = self.cache.len();
        let mut new_cache = HashMap::new();
        mem::swap(&mut new_cache, &mut self.cache);
        self.cache = new_cache
            .into_iter()
            .filter(|&(_, ref weak_ref)| weak_ref.upgrade().is_some())
            .collect();
        if self.cache.len() == previous_cache_size {
            self.purge_weak_limit *= 2;
        }
    }

    fn get_info(&mut self) -> CacheInfo {
        self.cleanup();
        let paths: Vec<PathBuf> = self.cache.keys().cloned().collect();
        CacheInfo {
            counters: self.counters.clone(),
            mmapped: paths,
        }
    }

    fn get_mmap(&mut self, full_path: PathBuf) -> Result<Option<Arc<Mmap>>, OpenReadError> {
        // if we exceed this limit, then we go through the weak
        // and remove those that are obsolete.
        if self.cache.len() > self.purge_weak_limit {
            self.cleanup();
        }
        Ok(match self.cache.entry(full_path.clone()) {
            HashMapEntry::Occupied(mut occupied_entry) => {
                if let Some(mmap_arc) = occupied_entry.get().upgrade() {
                    self.counters.hit += 1;
                    Some(mmap_arc.clone())
                } else {
                    // The entry exists but the weak ref has been destroyed.
                    self.counters.miss_weak += 1;
                    if let Some(mmap_arc) = open_mmap(&full_path)? {
                        occupied_entry.insert(Arc::downgrade(&mmap_arc));
                        Some(mmap_arc)
                    } else {
                        None
                    }
                }
            }
            HashMapEntry::Vacant(vacant_entry) => {
                self.counters.miss_empty += 1;
                if let Some(mmap_arc) = open_mmap(&full_path)? {
                    vacant_entry.insert(Arc::downgrade(&mmap_arc));
                    Some(mmap_arc)
                } else {
                    None
                }
            }
        })
    }
}

/// Directory storing data in files, read via mmap.
///
/// The Mmap object are cached to limit the
/// system calls.
#[derive(Clone)]
pub struct S3Directory {
    root_path: PathBuf,
    bucket: String,
    region: Region,
    mmap_cache: Arc<RwLock<MmapCache>>,
    _temp_directory: Arc<Option<TempDir>>,
}

impl fmt::Debug for S3Directory {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "S3Directory({:?})", self.root_path)
    }
}

impl S3Directory {
    /// Opens a S3Directory in a bucket.
    ///
    /// Returns an error if the `bucket` does not
    /// exist or if it is not a directory.
    pub fn open(
        region: String,
        bucket: String,
        directory_path: &Path,
    ) -> Result<S3Directory, OpenDirectoryError> {
        // TODO: should I use a different error type? probably

        let region = Region::from_str(&region).map_err(|_| {
            OpenDirectoryError::DoesNotExist(PathBuf::from("/bad/region"))
        })?;

        // TODO: handle missing creds
        let client = default_tls_client().map_err(|_| {
            OpenDirectoryError::DoesNotExist(PathBuf::from("/bad/tls/client"))
        })?;

        let provider = DefaultCredentialsProvider::new().map_err(|_| {
            OpenDirectoryError::DoesNotExist(PathBuf::from("/bad/creds"))
        })?;

        let s3 = S3Client::new(client, provider, region.clone());

        // does bucket exist?
        s3.head_bucket(&HeadBucketRequest { bucket: bucket.clone() })
            .map_err(|_| {
                OpenDirectoryError::DoesNotExist(PathBuf::from("/no/bucket"))
            })?;

        // TODO: how to store the client?
        Ok(S3Directory {
            bucket,
            region: region,
            root_path: PathBuf::from(directory_path),
            mmap_cache: Arc::new(RwLock::new(MmapCache::default())),
            _temp_directory: Arc::new(None),
        })

    }

    /// Joins a relative_path to the directory `root_path`
    /// to create a proper complete `filepath`.
    fn resolve_path(&self, relative_path: &Path) -> PathBuf {
        self.root_path.join(relative_path)
    }

    /// Returns some statistical information
    /// about the Mmap cache.
    ///
    /// The `MmapDirectory` embeds a `MmapDirectory`
    /// to avoid multiplying the `mmap` system calls.
    pub fn get_cache_info(&mut self) -> CacheInfo {
        self.mmap_cache
            .write()
            .expect("Mmap cache lock is poisoned.")
            .get_info()
    }
}

/// This Write wraps a File, but has the specificity of
/// call `sync_all` on flush.
struct SafeFileWriter(File);

impl SafeFileWriter {
    fn new(file: File) -> SafeFileWriter {
        SafeFileWriter(file)
    }
}

impl Write for SafeFileWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        try!(self.0.flush());
        self.0.sync_all()
    }
}

impl Seek for SafeFileWriter {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.0.seek(pos)
    }
}


impl Directory for S3Directory {
    fn open_read(&self, path: &Path) -> result::Result<ReadOnlySource, OpenReadError> {
        debug!("Open Read {:?}", path);
        let full_path = self.resolve_path(path);

        let mut mmap_cache = self.mmap_cache.write().map_err(|_| {
            let msg = format!(
                "Failed to acquired write lock \
                                            on mmap cache while reading {:?}",
                path
            );
            IOError::with_path(path.to_owned(), make_io_err(msg))
        })?;

        Ok(
            mmap_cache
                .get_mmap(full_path)?
                .map(MmapReadOnly::from)
                .map(ReadOnlySource::Mmap)
                .unwrap_or_else(|| ReadOnlySource::Anonymous(SharedVecSlice::empty())),
        )
    }

    fn open_write(&mut self, path: &Path) -> Result<WritePtr, OpenWriteError> {
        debug!("Open Write {:?}", path);
        let full_path = self.resolve_path(path);

        let open_res = OpenOptions::new().write(true).create_new(true).open(
            full_path,
        );

        let mut file = open_res.map_err(|err| if err.kind() ==
            io::ErrorKind::AlreadyExists
        {
            OpenWriteError::FileAlreadyExists(path.to_owned())
        } else {
            IOError::with_path(path.to_owned(), err).into()
        })?;

        // making sure the file is created.
        file.flush().map_err(
            |e| IOError::with_path(path.to_owned(), e),
        )?;

        let writer = SafeFileWriter::new(file);
        Ok(BufWriter::new(Box::new(writer)))
    }

    fn delete(&self, path: &Path) -> result::Result<(), DeleteError> {
        debug!("Deleting file {:?}", path);
        let full_path = self.resolve_path(path);
        let mut mmap_cache = self.mmap_cache.write().map_err(|_| {
            let msg = format!(
                "Failed to acquired write lock \
                                            on mmap cache while deleting {:?}",
                path
            );
            IOError::with_path(path.to_owned(), make_io_err(msg))
        })?;
        // Removing the entry in the MMap cache.
        // The munmap will appear on Drop,
        // when the last reference is gone.
        mmap_cache.cache.remove(&full_path);
        match fs::remove_file(&full_path) {
            Ok(_) => Ok(()),
            Err(e) => {
                if e.kind() == io::ErrorKind::NotFound {
                    Err(DeleteError::FileDoesNotExist(path.to_owned()))
                } else {
                    Err(IOError::with_path(path.to_owned(), e).into())
                }
            }
        }
    }

    fn exists(&self, path: &Path) -> bool {
        let full_path = self.resolve_path(path);
        full_path.exists()
    }

    fn atomic_read(&self, path: &Path) -> Result<Vec<u8>, OpenReadError> {
        let full_path = self.resolve_path(path);
        let mut buffer = Vec::new();
        match File::open(&full_path) {
            Ok(mut file) => {
                file.read_to_end(&mut buffer).map_err(|e| {
                    IOError::with_path(path.to_owned(), e)
                })?;
                Ok(buffer)
            }
            Err(e) => {
                if e.kind() == io::ErrorKind::NotFound {
                    Err(OpenReadError::FileDoesNotExist(path.to_owned()))
                } else {
                    Err(IOError::with_path(path.to_owned(), e).into())
                }
            }
        }

    }

    fn atomic_write(&mut self, path: &Path, data: &[u8]) -> io::Result<()> {
        debug!("Atomic Write {:?}", path);
        let full_path = self.resolve_path(path);
        let meta_file = atomicwrites::AtomicFile::new(full_path, atomicwrites::AllowOverwrite);
        try!(meta_file.write(|f| f.write_all(data)));
        Ok(())
    }

    fn box_clone(&self) -> Box<Directory> {
        Box::new(self.clone())
    }
}




#[cfg(test)]
mod tests {

    // There are more tests in directory/mod.rs
    // The following tests are specific to the S3Directory

    use super::*;

    #[test]
    fn bad_region() {
        // empty file is actually an edge case because those
        // cannot be mmapped.
        //
        // In that case the directory returns a SharedVecSlice.
        let mut s3dir = S3Directory::open(
            "us-nowhere-1".to_string(),
            "tantivy-test-bucket".to_string(),
            &PathBuf::from("/"),
        ).unwrap();

    }

    #[test]
    fn no_bucket() {

        let mut s3dir = S3Directory::open(
            "us-nowhere-1".to_string(),
            "tantivy-test-bucket-nope".to_string(),
            &PathBuf::from("/"),
        ).unwrap();

    }

    #[test]
    fn test_open_empty() {
        // empty file is actually an edge case because those
        // cannot be mmapped.
        //
        // In that case the directory returns a SharedVecSlice.
        let mut s3dir = S3Directory::open(
            "us-east-1".to_string(),
            "tantivy-test-bucket".to_string(),
            &PathBuf::from("/"),
        ).unwrap();

    }

    #[test]
    fn test_cache() {}

}
