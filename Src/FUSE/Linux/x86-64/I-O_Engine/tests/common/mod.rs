#![allow(dead_code)]
use std::alloc::{alloc_zeroed, dealloc, Layout};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};

use cafs_io::fault::FaultDevice;
use cafs_io::safety::{check_target, Signature, TargetInfo};
use cafs_io::{ByteOffset, CafsCtx, CafsError, MountMode};

pub const SB_OFF: u64 = 16384;
pub const CFG_OFF: u64 = 20480;
pub const PARITY_OFF: u64 = 24576;
pub const SMART_OFF: u64 = 36864;

/// A freshly formatted image, produced by the real `mkfs_cafs` binary.
pub fn formatted_image(size: u64) -> Vec<u8> {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("f.img");
    let ok = Command::new(env!("CARGO_BIN_EXE_mkfs_cafs"))
        .arg(&p)
        .arg(size.to_string())
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok, "mkfs_cafs failed");
    std::fs::read(&p).unwrap()
}

pub fn mount_image(img: Vec<u8>, write: bool) -> Result<(FaultDevice, CafsCtx), cafs_io::CafsError> {
    let dev = FaultDevice::from_image(img);
    let ctx = CafsCtx::mount_device(Box::new(dev.clone()), write)?;
    Ok((dev, ctx))
}

pub fn blake3_128_ok(block: &[u8], hashed_len: usize) -> bool {
    let mut h = [0u8; 16];
    blake3::Hasher::new().update(&block[..hashed_len]).finalize_xof().fill(&mut h);
    h[..] == block[hashed_len..hashed_len + 16]
}

// ---- Adaptive target: disk image by default, real block device on request ----
//
//   CAFS_TEST_DEVICE=/dev/sdXN  CAFS_TEST_DEVICE_I_AGREE=/dev/sdXN  cargo test ...
//
// The device is DESTROYED. check_target() additionally refuses a mounted or
// swap-active device, a whole disk with partitions, a device with holders,
// and any non-empty/non-swap/non-CAFS filesystem signature.

static DEVICE_LOCK: Mutex<()> = Mutex::new(());

pub struct Target {
    pub path: String,
    pub size: u64,
    pub info: Option<TargetInfo>,
    _dir: Option<tempfile::TempDir>,
    _guard: Option<MutexGuard<'static, ()>>,
}

impl Target {
    pub fn acquire(image_size: u64) -> Target {
        let dev = std::env::var("CAFS_TEST_DEVICE").unwrap_or_default();
        if dev.is_empty() {
            let d = tempfile::tempdir().unwrap();
            let path = d.path().join("t.img").to_str().unwrap().to_string();
            std::fs::File::create(&path).unwrap().set_len(image_size).unwrap();
            return Target { path, size: image_size, info: None, _dir: Some(d), _guard: None };
        }
        let agree = std::env::var("CAFS_TEST_DEVICE_I_AGREE").unwrap_or_default();
        assert_eq!(
            agree, dev,
            "CAFS_TEST_DEVICE={dev} will be DESTROYED. Re-run with CAFS_TEST_DEVICE_I_AGREE={dev} to confirm."
        );
        let guard = DEVICE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let info = check_target(&dev, &Signature::OVERWRITABLE).unwrap_or_else(|e| panic!("CAFS_TEST_DEVICE refused: {e}"));
        assert!(info.size >= 64 * 1024 * 1024, "device smaller than 64 MiB");
        Target { path: dev, size: info.size, info: Some(info), _dir: None, _guard: Some(guard) }
    }

    pub fn is_device(&self) -> bool {
        self.info.is_some()
    }

    pub fn describe(&self) -> String {
        match &self.info {
            None => format!("disk image ({} bytes, page-cached host file: not device numbers)", self.size),
            Some(i) => format!(
                "{} ({}) {} bytes, logical/physical sector {:?}/{:?}, rotational {:?}, signature before: {:?}",
                self.path,
                i.kernel_name,
                self.size,
                i.logical_sector,
                i.physical_sector,
                i.rotational,
                i.signature
            ),
        }
    }

    pub fn bypass(&self, write: bool) -> CafsCtx {
        CafsCtx::mount_with(&self.path, write, MountMode::Bypass).expect("bypass open")
    }

    pub fn mount(&self, write: bool) -> Result<CafsCtx, CafsError> {
        CafsCtx::mount(&self.path, write)
    }

    /// Wipe both anchor regions (device only), then run the real mkfs.
    pub fn format(&self) {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_mkfs_cafs"));
        cmd.arg(&self.path);
        if self.is_device() {
            let mut c = self.bypass(true);
            let zeros = vec![0u8; 64 * 1024];
            c.write_block(ByteOffset(0), &zeros).unwrap();
            c.write_block(ByteOffset(self.size - zeros.len() as u64), &zeros).unwrap();
            c.sync().unwrap();
            drop(c);
            cmd.arg("--force-device");
        } else {
            cmd.arg(self.size.to_string());
        }
        let out = cmd.output().unwrap();
        assert!(out.status.success(), "mkfs_cafs: {}", String::from_utf8_lossy(&out.stderr));
    }

    pub fn peek(&self, off: u64, len: usize) -> Vec<u8> {
        let mut b = vec![0u8; len];
        self.bypass(false).read_block(ByteOffset(off), &mut b).unwrap();
        b
    }

    pub fn poke(&self, off: u64, bytes: &[u8]) {
        let mut c = self.bypass(true);
        c.write_block(ByteOffset(off), bytes).unwrap();
        c.sync().unwrap();
    }

    /// Read bypassing the page cache (O_DIRECT, 4096-aligned buffer) so
    /// the bytes come from the device, not from RAM. Falls back to a
    /// buffered read where O_DIRECT is unsupported (e.g. tmpfs).
    pub fn peek_media(&self, off: u64, len: usize) -> (Vec<u8>, &'static str) {
        let a_off = off / 4096 * 4096;
        let a_end = (off + len as u64).div_ceil(4096) * 4096;
        let n = (a_end - a_off) as usize;
        if let Ok(fd) = rustix::fs::open(self.path.as_str(), rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::DIRECT, rustix::fs::Mode::empty()) {
            let layout = Layout::from_size_align(n, 4096).unwrap();
            // SAFETY: layout is non-zero-sized and 4096-aligned; the buffer is freed below.
            let ptr = unsafe { alloc_zeroed(layout) };
            let buf = unsafe { std::slice::from_raw_parts_mut(ptr, n) };
            let ok = matches!(rustix::io::pread(&fd, &mut *buf, a_off), Ok(got) if got == n);
            let out = buf[(off - a_off) as usize..(off - a_off) as usize + len].to_vec();
            unsafe { dealloc(ptr, layout) };
            if ok {
                return (out, "O_DIRECT");
            }
        }
        (self.peek(off, len), "buffered")
    }
}

impl Drop for Target {
    /// Leave a device in the known formatted state so the next test's
    /// safety check still recognises it. Skipped while panicking, so a
    /// failing test leaves its evidence on the device.
    fn drop(&mut self) {
        if self.is_device() && !std::thread::panicking() {
            self.format();
        }
    }
}
