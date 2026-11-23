From Stdlib Require Import List ZArith NArith Lia.
From Wasm Require Import operations.
Import ListNotations.

(* Arithmetic and memory facts used by the execution proof. *)

Section Facts.
Context `{bum : BlockUpdateMemory}.
Open Scope Z_scope.

Definition u32 (x : i32) : Z := Wasm_int.Int32.unsigned x.
Definition u64 (x : i64) : Z := Wasm_int.Int64.unsigned x.

(* The byte at address [a] as a number, read with [mem_lookup]. *)
Definition byte_at (m : meminst) (a : N) : Z :=
  match mem_lookup a (meminst_data m) with
  | Some b => Integers.Byte.unsigned b
  | None => 0
  end.

(* sum_{k < n} byte_at m (base + k), as a mathematical integer. *)
Fixpoint sum_bytes (m : meminst) (base : N) (n : nat) : Z :=
  match n with
  | O => 0
  | S k => sum_bytes m base k + byte_at m (base + N.of_nat k)%N
  end.

Definition mem_sum (m : meminst) (base len : N) : Z :=
  sum_bytes m base (N.to_nat len).

Lemma byte_at_range m a : 0 <= byte_at m a <= 255.
Proof.
  unfold byte_at. destruct (mem_lookup a (meminst_data m)) as [b|]; [|lia].
  pose proof (Integers.Byte.unsigned_range b) as H.
  change Integers.Byte.modulus with 256 in H. lia.
Qed.

Lemma sum_bytes_range m base n : 0 <= sum_bytes m base n <= 255 * Z.of_nat n.
Proof.
  induction n as [|n IH]; simpl; [lia|].
  pose proof (byte_at_range m (base + N.of_nat n)%N). lia.
Qed.

(* No overflow: the exact sum is at most 255 * len, which is below 2^64 for
   any 32-bit length. *)
Theorem mem_sum_no_overflow m base (len : i32) :
  0 <= mem_sum m base (Wasm_int.N_of_uint i32m len)
    <= 255 * Z.of_N (Wasm_int.N_of_uint i32m len) /\
  255 * Z.of_N (Wasm_int.N_of_uint i32m len) < 2 ^ 64.
Proof.
  unfold mem_sum.
  pose proof (sum_bytes_range m base (N.to_nat (Wasm_int.N_of_uint i32m len))).
  rewrite N_nat_Z in H.
  pose proof (Wasm_int.Int32.unsigned_range len) as Hr.
  change (Wasm_int.N_of_uint i32m len) with (Z.to_N (Wasm_int.Int32.unsigned len)).
  change (Wasm_int.N_of_uint i32m len) with (Z.to_N (Wasm_int.Int32.unsigned len)) in H.
  change Wasm_int.Int32.modulus with 4294967296 in Hr.
  rewrite Z2N.id by lia. rewrite Z2N.id in H by lia. lia.
Qed.

(* [mem_sum] unfolds to the expected finite sum. *)
Example mem_sum_3 m base :
  mem_sum m base 3%N =
    byte_at m base + byte_at m (base + 1)%N + byte_at m (base + 2)%N.
Proof.
  unfold mem_sum. simpl. rewrite N.add_0_r. lia.
Qed.

Example mem_sum_0 m base : mem_sum m base 0%N = 0.
Proof. reflexivity. Qed.

(* Sum indexed by an integer offset from an integer base. *)
Definition sumZ m (P J : Z) : Z := sum_bytes m (Z.to_N P) (Z.to_nat J).
Definition B m (z : Z) : Z := byte_at m (Z.to_N z).

Lemma sumZ_0 m P : sumZ m P 0 = 0.
Proof. reflexivity. Qed.

Lemma sumZ_step m P J : 0 <= P -> 0 <= J -> sumZ m P (J + 1) = sumZ m P J + B m (P + J).
Proof.
  intros HP HJ. unfold sumZ, B.
  rewrite Z2Nat.inj_add by lia. rewrite Nat.add_1_r. simpl.
  f_equal. f_equal. lia.
Qed.

Lemma sumZ_range m P J : 0 <= J -> 0 <= sumZ m P J <= 255 * J.
Proof.
  intros HJ. unfold sumZ. pose proof (sum_bytes_range m (Z.to_N P) (Z.to_nat J)). lia.
Qed.

Lemma B_range m z : 0 <= B m z <= 255.
Proof. apply byte_at_range. Qed.

(* ---------- machine integers ---------- *)

Lemma u32_range x : 0 <= u32 x < 2 ^ 32.
Proof. apply Wasm_int.Int32.unsigned_range. Qed.

Lemma u64_range x : 0 <= u64 x < 2 ^ 64.
Proof. apply Wasm_int.Int64.unsigned_range. Qed.

Lemma u32_repr z : 0 <= z < 2 ^ 32 -> u32 (Wasm_int.Int32.repr z) = z.
Proof.
  intros H. apply Wasm_int.Int32.unsigned_repr.
  change Wasm_int.Int32.max_unsigned with (2 ^ 32 - 1). lia.
Qed.

Lemma u64_repr z : 0 <= z < 2 ^ 64 -> u64 (Wasm_int.Int64.repr z) = z.
Proof.
  intros H. apply Wasm_int.Int64.unsigned_repr.
  change Wasm_int.Int64.max_unsigned with (2 ^ 64 - 1). lia.
Qed.

Lemma u32_add x y : u32 x + u32 y < 2 ^ 32 ->
  u32 (Wasm_int.Int32.add x y) = u32 x + u32 y.
Proof.
  intros H. unfold u32. rewrite Wasm_int.Int32.add_unsigned.
  apply u32_repr. pose proof (u32_range x); pose proof (u32_range y).
  unfold u32 in *. lia.
Qed.

Lemma u32_add_repr x k : 0 <= k -> u32 x + k < 2 ^ 32 ->
  u32 (Wasm_int.Int32.add x (Wasm_int.Int32.repr k)) = u32 x + k.
Proof.
  intros Hk H. rewrite u32_add; rewrite u32_repr; try lia.
  all: pose proof (u32_range x); lia.
Qed.

Lemma u32_dec x : 1 <= u32 x ->
  u32 (Wasm_int.Int32.add x (Wasm_int.Int32.repr (-1))) = u32 x - 1.
Proof.
  intros H. unfold u32. rewrite Wasm_int.Int32.add_unsigned.
  change (Wasm_int.Int32.unsigned (Wasm_int.Int32.repr (-1))) with (2 ^ 32 - 1).
  rewrite Wasm_int.Int32.unsigned_repr_eq.
  change Wasm_int.Int32.modulus with (2 ^ 32).
  pose proof (u32_range x). unfold u32 in *.
  replace (Wasm_int.Int32.unsigned x + (2 ^ 32 - 1)) with
    ((Wasm_int.Int32.unsigned x - 1) + 1 * 2 ^ 32) by lia.
  rewrite Z.mod_add by lia. apply Z.mod_small. lia.
Qed.

Lemma u64_add_byte t b : 0 <= b -> u64 t + b < 2 ^ 64 ->
  u64 (Wasm_int.Int64.add t (Wasm_int.Int64.repr b)) = u64 t + b.
Proof.
  intros Hb H. unfold u64. rewrite Wasm_int.Int64.add_unsigned.
  pose proof (u64_range t). unfold u64 in *.
  rewrite (Wasm_int.Int64.unsigned_repr b).
  2: change Wasm_int.Int64.max_unsigned with (2 ^ 64 - 1); lia.
  apply u64_repr. lia.
Qed.

Lemma i32_eq_u32 x y : u32 x = u32 y -> x = y.
Proof.
  intros H. rewrite <- (Wasm_int.Int32.repr_unsigned x),
    <- (Wasm_int.Int32.repr_unsigned y). unfold u32 in H. rewrite H. reflexivity.
Qed.

Lemma i32_neq_u32 x y : u32 x <> u32 y -> x <> y.
Proof. intros H E. subst. apply H. reflexivity. Qed.

Lemma i32_eqb_false x y : u32 x <> u32 y -> Wasm_int.Int32.eq x y = false.
Proof.
  intros H. apply Wasm_int.Int32.eq_false. apply i32_neq_u32. exact H.
Qed.

Lemma i32_eqb_true x y : u32 x = u32 y -> Wasm_int.Int32.eq x y = true.
Proof.
  intros H. rewrite (i32_eq_u32 x y H). apply Wasm_int.Int32.eq_true.
Qed.

Lemma i32_ltu x y : Wasm_int.Int32.ltu x y = (u32 x <? u32 y).
Proof.
  unfold Wasm_int.Int32.ltu, u32.
  destruct (Coqlib.zlt _ _); symmetry; [apply Z.ltb_lt | apply Z.ltb_ge]; lia.
Qed.

Lemma u32_and3 x : u32 (Wasm_int.Int32.and x (Wasm_int.Int32.repr 3)) = u32 x mod 4.
Proof.
  unfold u32, Wasm_int.Int32.and.
  change (Wasm_int.Int32.unsigned (Wasm_int.Int32.repr 3)) with (Z.ones 2).
  rewrite Z.land_ones by lia.
  change (2 ^ 2) with 4. apply u32_repr.
  pose proof (Z.mod_pos_bound (Wasm_int.Int32.unsigned x) 4 ltac:(lia)). lia.
Qed.

Lemma u32_andm4 x :
  u32 (Wasm_int.Int32.and x (Wasm_int.Int32.repr (-4))) = u32 x - u32 x mod 4.
Proof.
  set (A := Wasm_int.Int32.and x (Wasm_int.Int32.repr (-4))).
  assert (Hsum : Wasm_int.Int32.add A (Wasm_int.Int32.and x (Wasm_int.Int32.repr 3)) = x).
  { unfold A. rewrite Wasm_int.Int32.add_and
      by (apply Wasm_int.Int32.same_if_eq; vm_compute; reflexivity).
    replace (Wasm_int.Int32.or (Wasm_int.Int32.repr (-4)) (Wasm_int.Int32.repr 3))
      with Wasm_int.Int32.mone by (apply Wasm_int.Int32.same_if_eq; vm_compute; reflexivity).
    apply Wasm_int.Int32.and_mone. }
  assert (HA4 : u32 A mod 4 = 0).
  { rewrite <- u32_and3. unfold A. rewrite Wasm_int.Int32.and_assoc.
    replace (Wasm_int.Int32.and (Wasm_int.Int32.repr (-4)) (Wasm_int.Int32.repr 3))
      with Wasm_int.Int32.zero by (apply Wasm_int.Int32.same_if_eq; vm_compute; reflexivity).
    rewrite Wasm_int.Int32.and_zero. reflexivity. }
  pose proof (u32_range A) as HA. pose proof (u32_range x) as Hx.
  assert (Hle : u32 A <= 2 ^ 32 - 4).
  { pose proof (Z.div_mod (u32 A) 4 ltac:(lia)). lia. }
  apply (f_equal u32) in Hsum.
  rewrite u32_add in Hsum; rewrite u32_and3 in *;
    pose proof (Z.mod_pos_bound (u32 x) 4); lia.
Qed.

(* ---------- loads ---------- *)

Lemma decode_byte b :
  Memdata.decode_int (bytes_takefill #00 8 [b]) = Integers.Byte.unsigned b.
Proof.
  unfold bytes_takefill. simpl.
  unfold Memdata.decode_int, Memdata.rev_if_be. simpl.
  change Archi.big_endian with false. cbv iota. simpl Memdata.int_of_bytes.
  change (Integers.Byte.unsigned #00) with 0. lia.
Qed.

Lemma load8u_ok m k :
  (k + 1 <= operations.mem_length m)%N ->
  load_packed SX_U m k 0%N (tp_length Tp_i8) (tnum_length T_i64) =
    Some (bytes_takefill #00 8
      [match mem_lookup k (meminst_data m) with Some b => b | None => #00 end]) /\
  exists b, mem_lookup k (meminst_data m) = Some b.
Proof.
  intros H.
  destruct (mem_lookup k (meminst_data m)) as [b|] eqn:Hb.
  2: { exfalso. eapply mem_lookup_ib; [| exact Hb].
       unfold operations.mem_length in H. lia. }
  split; [| eauto].
  unfold load_packed, load. simpl tp_length.
  replace (N.leb (k + (0 + 1)) (operations.mem_length m)) with true
    by (symmetry; apply N.leb_le; lia).
  unfold read_bytes. simpl. rewrite N.add_0_r, N.add_0_r, Hb. reflexivity.
Qed.

End Facts.
