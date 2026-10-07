//! Respawn VPK reader (Titanfall 2 flavour of Valve's VPK v2).
//!
//! The directory file (`englishclient_<map>.bsp.pak000_dir.vpk`) holds the file tree;
//! data lives in `client_<map>.bsp.pak000_NNN.vpk` archives, split into parts that are
//! either stored or LZHAM-compressed (dictionary size 2^20).

use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const VPK_MAGIC: u32 = 0x55AA_1234;
const VPK_VERSION: u32 = 0x0003_0002;

#[derive(Debug, Clone)]
pub struct VpkPart {
    pub archive: u16,
    pub load_flags: u16,
    pub texture_flags: u32,
    pub offset: u64,
    pub len: u64,
    pub len_uncompressed: u64,
}

#[derive(Debug, Clone)]
pub struct VpkEntry {
    pub crc: u32,
    pub preload: Vec<u8>,
    pub parts: Vec<VpkPart>,
}

impl VpkEntry {
    pub fn size(&self) -> u64 {
        self.preload.len() as u64 + self.parts.iter().map(|p| p.len_uncompressed).sum::<u64>()
    }
}

pub struct Vpk {
    dir: PathBuf,
    /// Archive file stem, e.g. `client_mp_common.bsp.pak000`.
    archive_stem: String,
    pub entries: HashMap<String, VpkEntry>,
}

struct Cursor<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Cursor<'a> {
    fn u16(&mut self) -> Result<u16> {
        let v = self.b.get(self.p..self.p + 2).context("vpk tree truncated")?;
        self.p += 2;
        Ok(u16::from_le_bytes(v.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32> {
        let v = self.b.get(self.p..self.p + 4).context("vpk tree truncated")?;
        self.p += 4;
        Ok(u32::from_le_bytes(v.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64> {
        let v = self.b.get(self.p..self.p + 8).context("vpk tree truncated")?;
        self.p += 8;
        Ok(u64::from_le_bytes(v.try_into().unwrap()))
    }
    fn cstr(&mut self) -> Result<&'a str> {
        let rest = &self.b[self.p..];
        let n = rest.iter().position(|&c| c == 0).context("unterminated string")?;
        self.p += n + 1;
        Ok(std::str::from_utf8(&rest[..n])?)
    }
    fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        let v = self.b.get(self.p..self.p + n).context("vpk tree truncated")?;
        self.p += n;
        Ok(v)
    }
}

impl Vpk {
    /// Open a `*_dir.vpk` file.
    pub fn open(dir_path: impl AsRef<Path>) -> Result<Self> {
        let dir_path = dir_path.as_ref();
        let mut f = File::open(dir_path).with_context(|| format!("open {}", dir_path.display()))?;
        let mut hdr = [0u8; 16];
        f.read_exact(&mut hdr)?;
        let magic = u32::from_le_bytes(hdr[0..4].try_into().unwrap());
        let version = u32::from_le_bytes(hdr[4..8].try_into().unwrap());
        let tree_size = u32::from_le_bytes(hdr[8..12].try_into().unwrap()) as usize;
        if magic != VPK_MAGIC || version != VPK_VERSION {
            bail!("{} is not a Respawn VPK (magic {magic:#x}, version {version:#x})", dir_path.display());
        }
        let mut tree = vec![0u8; tree_size];
        f.read_exact(&mut tree)?;

        let mut c = Cursor { b: &tree, p: 0 };
        let mut entries = HashMap::new();
        loop {
            let ext = c.cstr()?;
            if ext.is_empty() {
                break;
            }
            loop {
                let path = c.cstr()?;
                if path.is_empty() {
                    break;
                }
                loop {
                    let name = c.cstr()?;
                    if name.is_empty() {
                        break;
                    }
                    let crc = c.u32()?;
                    let preload_len = c.u16()? as usize;
                    let mut parts = Vec::new();
                    loop {
                        let archive = c.u16()?;
                        if archive == 0xFFFF {
                            break;
                        }
                        parts.push(VpkPart {
                            archive,
                            load_flags: c.u16()?,
                            texture_flags: c.u32()?,
                            offset: c.u64()?,
                            len: c.u64()?,
                            len_uncompressed: c.u64()?,
                        });
                    }
                    let preload = c.bytes(preload_len)?.to_vec();
                    let full = if path == " " {
                        format!("{name}.{ext}")
                    } else {
                        format!("{path}/{name}.{ext}")
                    };
                    entries.insert(full.to_ascii_lowercase(), VpkEntry { crc, preload, parts });
                }
            }
        }

        let file_name = dir_path.file_name().unwrap().to_string_lossy().to_string();
        let stem = file_name.trim_end_matches("_dir.vpk");
        // Strip the language prefix: englishclient_x -> client_x.
        let archive_stem = match stem.find("client_").or_else(|| stem.find("server_")) {
            Some(i) => stem[i..].to_string(),
            None => stem.to_string(),
        };
        Ok(Self {
            dir: dir_path.parent().unwrap().to_path_buf(),
            archive_stem,
            entries,
        })
    }

    pub fn contains(&self, path: &str) -> bool {
        self.entries.contains_key(&normalize(path))
    }

    pub fn read(&self, path: &str) -> Result<Vec<u8>> {
        let key = normalize(path);
        let e = self.entries.get(&key).with_context(|| format!("{path} not in vpk"))?;
        let mut out = Vec::with_capacity(e.size() as usize);
        out.extend_from_slice(&e.preload);
        // Each part is compressed on its own, so large entries (a map's .bsp is ~200 MB) are
        // read and decompressed on every core.
        let parts: Vec<&_> = e.parts.iter().filter(|p| p.len_uncompressed != 0).collect();
        let threads = if parts.len() >= 8 { std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(16) } else { 1 };
        let next = std::sync::atomic::AtomicUsize::new(0);
        let mut chunks: Vec<(usize, Result<Vec<u8>>)> = std::thread::scope(|scope| {
            let workers: Vec<_> = (0..threads)
                .map(|_| {
                    scope.spawn(|| {
                        let mut open: Option<(u16, File)> = None;
                        let mut done = Vec::new();
                        loop {
                            let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            let Some(part) = parts.get(i) else { break };
                            let r = (|| -> Result<Vec<u8>> {
                                if open.as_ref().map(|o| o.0) != Some(part.archive) {
                                    let p = self.dir.join(format!("{}_{:03}.vpk", self.archive_stem, part.archive));
                                    open = Some((part.archive, File::open(&p).with_context(|| format!("open {}", p.display()))?));
                                }
                                let f = &mut open.as_mut().unwrap().1;
                                f.seek(SeekFrom::Start(part.offset))?;
                                let mut buf = vec![0u8; part.len as usize];
                                f.read_exact(&mut buf)?;
                                if part.len == part.len_uncompressed {
                                    Ok(buf)
                                } else {
                                    lzham_decompress(&buf, part.len_uncompressed as usize)
                                }
                            })();
                            done.push((i, r));
                        }
                        done
                    })
                })
                .collect();
            workers.into_iter().flat_map(|w| w.join().unwrap_or_default()).collect()
        });
        if chunks.len() != parts.len() {
            bail!("reading {path}: a worker failed");
        }
        chunks.sort_by_key(|(i, _)| *i);
        for (_, c) in chunks {
            out.extend_from_slice(&c?);
        }
        if crc32fast::hash(&out) != e.crc {
            bail!("crc mismatch reading {path}");
        }
        Ok(out)
    }
}

fn normalize(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches('/').to_ascii_lowercase()
}

#[repr(C)]
struct LzhamDecompressParams {
    struct_size: u32,
    dict_size_log2: u32,
    flags: u32,
    num_seed_bytes: u32,
    seed_bytes: *const u8,
}

extern "C" {
    fn tf_lzham_decompress_memory(
        params: *const LzhamDecompressParams,
        dst: *mut u8,
        dst_len: *mut usize,
        src: *const u8,
        src_len: usize,
        adler32: *mut u32,
    ) -> i32;
}

const LZHAM_DECOMP_FLAG_OUTPUT_UNBUFFERED: u32 = 1;
const LZHAM_DECOMP_FLAG_COMPUTE_ADLER32: u32 = 2;
const LZHAM_DECOMP_STATUS_SUCCESS: i32 = 3;

fn lzham_decompress(src: &[u8], dst_len: usize) -> Result<Vec<u8>> {
    let params = LzhamDecompressParams {
        struct_size: std::mem::size_of::<LzhamDecompressParams>() as u32,
        dict_size_log2: 20,
        flags: LZHAM_DECOMP_FLAG_OUTPUT_UNBUFFERED | LZHAM_DECOMP_FLAG_COMPUTE_ADLER32,
        num_seed_bytes: 0,
        seed_bytes: std::ptr::null(),
    };
    let mut dst = vec![0u8; dst_len];
    let mut out_len = dst_len;
    let mut adler = 0u32;
    // SAFETY: buffers are valid for the given lengths; the decompressor never writes past dst_len.
    let status = unsafe {
        tf_lzham_decompress_memory(&params, dst.as_mut_ptr(), &mut out_len, src.as_ptr(), src.len(), &mut adler)
    };
    if status != LZHAM_DECOMP_STATUS_SUCCESS {
        bail!("lzham decompression failed (status {status})");
    }
    dst.truncate(out_len);
    Ok(dst)
}
