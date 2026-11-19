# Trusted lemma for the split-even KWasm claims

K declares `>>Int` with `smtlib(shrInt)`: the SMT solver sees an
uninterpreted function, so it cannot relate the shift the shipped code
performs to arithmetic. This lemma is the only addition to KWasm's own
definitions and lemmas. It is **trusted, not proved**: K applies
`[simplification]` rules without checking them. It is a true fact about
integers (for `X ≥ 0`, an arithmetic right shift by one is floor division by
two, and K's `/Int` truncates, which agrees for non-negative operands), and
it is listed in the trusted computing base of every result that uses it.

```k
requires "kwasm-lemmas.md"

module SPLIT-EVEN-LEMMAS
    imports KWASM-LEMMAS

    rule X >>Int 1 => X /Int 2 requires 0 <=Int X [simplification]
endmodule
```
