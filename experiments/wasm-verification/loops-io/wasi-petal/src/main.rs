//! loops-io experiment 2: a stdin -> stdout Petal in the whitepaper's sense.
//!
//! A WASI command whose `_start` copies stdin to stdout with ASCII letters
//! upper-cased. The proof target is the module's real `_start` export; there
//! is no verification-only export or shim. Only `fd_read` and `fd_write` are
//! imported. `no_std` keeps Rust's std runtime (args, env, allocator, locks)
//! out of the first measurement; see ../README.md for the std comparison.
#![no_std]
#![no_main]

#[repr(C)]
struct IoVec {
    buf: *mut u8,
    len: usize,
}

#[link(wasm_import_module = "wasi_snapshot_preview1")]
unsafe extern "C" {
    fn fd_read(fd: i32, iovs: *const IoVec, iovs_len: usize, nread: *mut usize) -> u16;
    fn fd_write(fd: i32, iovs: *const IoVec, iovs_len: usize, nwritten: *mut usize) -> u16;
}

const STDIN: i32 = 0;
const STDOUT: i32 = 1;
const CHUNK: usize = 256;

static mut BUF: [u8; CHUNK] = [0; CHUNK];

/// Writes `bytes`, retrying partial writes. Returns false on a host error.
fn write_all(mut ptr: *mut u8, mut len: usize) -> bool {
    while len > 0 {
        let iov = IoVec { buf: ptr, len };
        let mut written = 0usize;
        if unsafe { fd_write(STDOUT, &iov, 1, &mut written) } != 0 || written == 0 {
            return false;
        }
        let written = written.min(len);
        ptr = unsafe { ptr.add(written) };
        len -= written;
    }
    true
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    let buf = &raw mut BUF as *mut u8;
    loop {
        let iov = IoVec { buf, len: CHUNK };
        let mut n = 0usize;
        if unsafe { fd_read(STDIN, &iov, 1, &mut n) } != 0 || n == 0 {
            return;
        }
        let n = n.min(CHUNK);
        for i in 0..n {
            let b = unsafe { buf.add(i) };
            unsafe { *b = (*b).to_ascii_uppercase() };
        }
        if !write_all(buf, n) {
            return;
        }
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}
