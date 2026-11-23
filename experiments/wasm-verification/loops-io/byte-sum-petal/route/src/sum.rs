//! Loop verification target for loops-io experiment 1.
//!
//! The emitted implementation of `byte_sum_target` inside the packaged
//! component is the proof target. Like split-even, it is exported only so a
//! decoder can find it in a symbol-stripped build; the route calls it normally.

/// Returns the sum of the `len` bytes at `ptr`, as an unsigned integer.
///
/// # Safety
/// `ptr..ptr + len` must be readable.
#[inline(never)]
#[unsafe(export_name = "bloom_verify_byte_sum")]
pub unsafe extern "C" fn byte_sum_target(ptr: *const u8, len: usize) -> u64 {
    let mut total: u64 = 0;
    let mut i = 0;
    while i < len {
        total += u64::from(unsafe { *ptr.add(i) });
        i += 1;
    }
    total
}

/// Safe view of [`byte_sum_target`].
pub fn byte_sum(bytes: &[u8]) -> u64 {
    unsafe { byte_sum_target(bytes.as_ptr(), bytes.len()) }
}
