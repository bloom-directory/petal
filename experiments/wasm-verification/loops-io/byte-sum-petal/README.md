# byte-sum

Verification fixture for loops-io experiment 1. One read-only route,
`sum/<text>.json`, returns `{"bytes", "sum"}`. The loop in `route/src/sum.rs`
is the proof target (`../rocq/ByteSum*.v`).
