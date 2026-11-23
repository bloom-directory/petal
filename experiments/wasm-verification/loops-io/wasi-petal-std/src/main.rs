//! Size comparison for loops-io experiment 2: the same filter with Rust std.
//! Not a proof target; see ../README.md.
use std::io::{Read, Write};

fn main() {
    let mut stdin = std::io::stdin().lock();
    let mut stdout = std::io::stdout().lock();
    let mut buf = [0u8; 256];
    loop {
        let n = match stdin.read(&mut buf) {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        buf[..n].make_ascii_uppercase();
        if stdout.write_all(&buf[..n]).is_err() {
            return;
        }
    }
}
