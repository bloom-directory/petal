From Stdlib Require Import ZArith Lia.
From Wasm Require Import numerics.

Module I := Wasm_int.Int64.
Open Scope Z_scope.

Definition half (total : i64) : i64 := I.ishr_u total I.one.
Definition remainder (total : i64) : i64 := I.isub total (half total).

Definition shares_correct (total a b : Z) : Prop :=
  0 <= a < 2^64 /\ 0 <= b < 2^64 /\
  a + b = total /\ a <= b /\ 0 <= b - a <= 1.

Lemma half_unsigned t : I.unsigned (half t) = I.unsigned t / 2.
Proof.
  unfold half, I.ishr_u.
  change (I.unsigned (I.shru t I.one) = I.unsigned t / 2).
  rewrite I.shru_div_two_p.
  change (I.unsigned (I.repr (I.unsigned t / 2)) = I.unsigned t / 2).
  apply I.unsigned_repr.
  pose proof (I.unsigned_range_2 t) as Hrange.
  assert (0 <= I.unsigned t / 2) by (apply Z.div_pos; lia).
  assert (I.unsigned t / 2 <= I.unsigned t).
  { apply Z.div_le_upper_bound; lia. }
  lia.
Qed.

Lemma remainder_unsigned t :
  I.unsigned (remainder t) = I.unsigned t - I.unsigned t / 2.
Proof.
  unfold remainder, I.isub, I.sub.
  rewrite half_unsigned.
  apply I.unsigned_repr.
  pose proof (I.unsigned_range_2 t) as Hrange.
  assert (0 <= I.unsigned t / 2) by (apply Z.div_pos; lia).
  assert (I.unsigned t / 2 <= I.unsigned t).
  { apply Z.div_le_upper_bound; lia. }
  lia.
Qed.

Theorem split_arithmetic t :
  shares_correct (I.unsigned t)
    (I.unsigned (half t)) (I.unsigned (remainder t)).
Proof.
  unfold shares_correct.
  pose proof (I.unsigned_range (half t)) as Ha.
  pose proof (I.unsigned_range (remainder t)) as Hb.
  change (0 <= I.unsigned (half t) < 2^64) in Ha.
  change (0 <= I.unsigned (remainder t) < 2^64) in Hb.
  rewrite half_unsigned, remainder_unsigned in *.
  pose proof (Z.div_mod (I.unsigned t) 2 ltac:(lia)) as Hdiv.
  pose proof (Z.mod_pos_bound (I.unsigned t) 2 ltac:(lia)) as Hmod.
  lia.
Qed.

Example regression_zero :
  (I.unsigned (half (I.repr 0)), I.unsigned (remainder (I.repr 0))) = (0, 0).
Proof. vm_compute. reflexivity. Qed.

Example regression_one :
  (I.unsigned (half (I.repr 1)), I.unsigned (remainder (I.repr 1))) = (0, 1).
Proof. vm_compute. reflexivity. Qed.

Example regression_seven :
  (I.unsigned (half (I.repr 7)), I.unsigned (remainder (I.repr 7))) = (3, 4).
Proof. vm_compute. reflexivity. Qed.

Example regression_max :
  (I.unsigned (half (I.repr (2^64-1))),
   I.unsigned (remainder (I.repr (2^64-1)))) = (2^63-1, 2^63).
Proof. vm_compute. reflexivity. Qed.

Example faulty_conservation_counterexample :
  I.unsigned (half (I.repr 1)) + I.unsigned (half (I.repr 1)) <> 1.
Proof. vm_compute. discriminate. Qed.

Print Assumptions split_arithmetic.
