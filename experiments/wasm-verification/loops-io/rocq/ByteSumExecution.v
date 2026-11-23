From Stdlib Require Import List ZArith NArith Lia Relations.Relation_Operators.
From mathcomp Require Import seq.
From Wasm Require Import opsem.
Require Import ByteSumArtifact ByteSumLib ByteSumFacts.
Import ListNotations.

Section Exec.
Context {hfc : host_function_class} {bum : BlockUpdateMemory}
        {ho : @host hfc bum}.
Open Scope Z_scope.

Notation add := Wasm_int.Int32.add.
Notation rep := Wasm_int.Int32.repr.

(* ---------------- specialised straight-line steps ---------------- *)

Section Steps.
Variables (hs : host_state) (s : store_record).

Lemma c_i32 f st z is fin :
  reduce_trans (hs, s, f, V (VAL_num (VAL_int32 (rep z)) :: st) ++ to_e_list is) fin ->
  reduce_trans (hs, s, f, V st ++ to_e_list (i32c z :: is)) fin.
Proof. intros H. exact (cps_const hs s f st _ is fin H). Qed.

Lemma c_i64 f st z is fin :
  reduce_trans (hs, s, f, V (VAL_num (VAL_int64 (Wasm_int.Int64.repr z)) :: st)
                          ++ to_e_list is) fin ->
  reduce_trans (hs, s, f, V st ++ to_e_list (i64c z :: is)) fin.
Proof. intros H. exact (cps_const hs s f st _ is fin H). Qed.

Lemma c_add32 f st x y is fin :
  reduce_trans (hs, s, f, V (VAL_num (VAL_int32 (add x y)) :: st) ++ to_e_list is) fin ->
  reduce_trans (hs, s, f, V (VAL_num (VAL_int32 y) :: VAL_num (VAL_int32 x) :: st)
                          ++ to_e_list (add32 :: is)) fin.
Proof. intros H. eapply cps_binop; [reflexivity | reflexivity | exact H]. Qed.

Lemma c_and32 f st x y is fin :
  reduce_trans (hs, s, f, V (VAL_num (VAL_int32 (Wasm_int.Int32.and x y)) :: st)
                          ++ to_e_list is) fin ->
  reduce_trans (hs, s, f, V (VAL_num (VAL_int32 y) :: VAL_num (VAL_int32 x) :: st)
                          ++ to_e_list (and32 :: is)) fin.
Proof. intros H. eapply cps_binop; [reflexivity | reflexivity | exact H]. Qed.

Lemma c_add64 f st x y is fin :
  reduce_trans (hs, s, f, V (VAL_num (VAL_int64 (Wasm_int.Int64.add x y)) :: st)
                          ++ to_e_list is) fin ->
  reduce_trans (hs, s, f, V (VAL_num (VAL_int64 y) :: VAL_num (VAL_int64 x) :: st)
                          ++ to_e_list (add64 :: is)) fin.
Proof. intros H. eapply cps_binop; [reflexivity | reflexivity | exact H]. Qed.

Lemma c_ne32 f st x y is fin :
  reduce_trans (hs, s, f, V (VAL_num (VAL_int32 (wasm_bool
                   (negb (Wasm_int.Int32.eq x y)))) :: st) ++ to_e_list is) fin ->
  reduce_trans (hs, s, f, V (VAL_num (VAL_int32 y) :: VAL_num (VAL_int32 x) :: st)
                          ++ to_e_list (BI_relop T_i32 (Relop_i ROI_ne) :: is)) fin.
Proof. intros H. eapply cps_relop; [reflexivity | exact H]. Qed.

Lemma c_geu32 f st x y is fin :
  reduce_trans (hs, s, f, V (VAL_num (VAL_int32 (wasm_bool
                   (negb (Wasm_int.Int32.ltu x y)))) :: st) ++ to_e_list is) fin ->
  reduce_trans (hs, s, f, V (VAL_num (VAL_int32 y) :: VAL_num (VAL_int32 x) :: st)
                 ++ to_e_list (BI_relop T_i32 (Relop_i (ROI_ge SX_U)) :: is)) fin.
Proof. intros H. eapply cps_relop; [reflexivity | exact H]. Qed.

Lemma c_eqz f st x is fin :
  reduce_trans (hs, s, f, V (VAL_num (VAL_int32 (wasm_bool
                   (Wasm_int.Int32.eq Wasm_int.Int32.zero x))) :: st) ++ to_e_list is) fin ->
  reduce_trans (hs, s, f, V (VAL_num (VAL_int32 x) :: st)
                 ++ to_e_list (BI_testop T_i32 TO_eqz :: is)) fin.
Proof. intros H. exact (cps_eqz32 hs s f st x is fin H). Qed.

Lemma c_brif_nz f st c j is fin :
  u32 c <> 0 ->
  reduce_trans (hs, s, f, V st ++ to_e_list (BI_br j :: is)) fin ->
  reduce_trans (hs, s, f, V (VAL_num (VAL_int32 c) :: st) ++
                          to_e_list (BI_br_if j :: is)) fin.
Proof.
  intros Hc H. apply cps_br_if_true; [| exact H].
  intros E. apply Hc. rewrite E. reflexivity.
Qed.

Lemma c_brif_z f st c j is fin :
  u32 c = 0 ->
  reduce_trans (hs, s, f, V st ++ to_e_list is) fin ->
  reduce_trans (hs, s, f, V (VAL_num (VAL_int32 c) :: st) ++
                          to_e_list (BI_br_if j :: is)) fin.
Proof.
  intros Hc H. apply cps_br_if_false; [| exact H].
  apply i32_eq_u32. rewrite Hc. reflexivity.
Qed.

Lemma u32_bool b : u32 (wasm_bool b) = if b then 1 else 0.
Proof. destruct b; reflexivity. Qed.

Variable m : meminst.
Definition bv (k : i32) : i64 :=
  Wasm_int.Int64.repr (byte_at m (Wasm_int.N_of_uint i32m k)).

Lemma bv_B k : u64 (bv k) = B m (u32 k).
Proof.
  unfold bv, B. rewrite u64_repr; [reflexivity|].
  pose proof (byte_at_range m (Wasm_int.N_of_uint i32m k)). lia.
Qed.

Lemma c_load f st k is fin :
  smem s (f_inst f) = Some m ->
  u32 k + 1 <= Z.of_N (operations.mem_length m) ->
  reduce_trans (hs, s, f, V (VAL_num (VAL_int64 (bv k)) :: st) ++ to_e_list is) fin ->
  reduce_trans (hs, s, f, V (VAL_num (VAL_int32 k) :: st) ++ to_e_list (ld8u :: is)) fin.
Proof.
  intros Hm Hk H. rewrite V_cons in H. rewrite V_cons, to_e_cons.
  assert (Hk' : (Wasm_int.N_of_uint i32m k + 1 <= operations.mem_length m)%N).
  { change (Wasm_int.N_of_uint i32m k) with (Z.to_N (u32 k)).
    pose proof (u32_range k). apply N2Z.inj_le. rewrite N2Z.inj_add, Z2N.id; lia. }
  destruct (load8u_ok m _ Hk') as [Hl [b Hb]]. rewrite Hb in Hl.
  apply (step_ctx_cps hs s f f (List.rev st) [$VN (VAL_int32 k); AI_basic ld8u]
           [$VN (VAL_int64 (bv k))] (to_e_list is) fin); [| exact H].
  assert (Hd : wasm_deserialise (bytes_takefill #00 8 [b]) T_i64 = VAL_int64 (bv k)).
  { unfold wasm_deserialise. rewrite decode_byte. unfold bv, byte_at. rewrite Hb.
    reflexivity. }
  rewrite <- Hd. eapply r_load_packed_success; [exact Hm | exact Hl].
Qed.

End Steps.

(* ---------------- the six locals ---------------- *)

Variables (hs : host_state) (s : store_record) (inst : moduleinst) (m : meminst).
Hypothesis Hsmem : smem s inst = Some m.

Definition fr p a b c (t : i64) d : frame :=
  Build_frame [VAL_num (VAL_int32 p); VAL_num (VAL_int32 a); VAL_num (VAL_int32 b);
               VAL_num (VAL_int32 c); VAL_num (VAL_int64 t); VAL_num (VAL_int32 d)] inst.

Lemma c_get f st j v is fin :
  lookup_N (f_locs f) j = Some v ->
  reduce_trans (hs, s, f, V (v :: st) ++ to_e_list is) fin ->
  reduce_trans (hs, s, f, V st ++ to_e_list (BI_local_get j :: is)) fin.
Proof. apply cps_get. Qed.

Ltac sset := intros H; apply cps_set; [simpl; lia | exact H].

Lemma c_set1 p a b c t d x st is fin :
  reduce_trans (hs, s, fr p x b c t d, V st ++ to_e_list is) fin ->
  reduce_trans (hs, s, fr p a b c t d, V (VAL_num (VAL_int32 x) :: st)
                 ++ to_e_list (BI_local_set 1%N :: is)) fin.
Proof. sset. Qed.
Lemma c_set2 p a b c t d x st is fin :
  reduce_trans (hs, s, fr p a x c t d, V st ++ to_e_list is) fin ->
  reduce_trans (hs, s, fr p a b c t d, V (VAL_num (VAL_int32 x) :: st)
                 ++ to_e_list (BI_local_set 2%N :: is)) fin.
Proof. sset. Qed.
Lemma c_set3 p a b c t d x st is fin :
  reduce_trans (hs, s, fr p a b x t d, V st ++ to_e_list is) fin ->
  reduce_trans (hs, s, fr p a b c t d, V (VAL_num (VAL_int32 x) :: st)
                 ++ to_e_list (BI_local_set 3%N :: is)) fin.
Proof. sset. Qed.
Lemma c_set4 p a b c t d x st is fin :
  reduce_trans (hs, s, fr p a b c x d, V st ++ to_e_list is) fin ->
  reduce_trans (hs, s, fr p a b c t d, V (VAL_num (VAL_int64 x) :: st)
                 ++ to_e_list (BI_local_set 4%N :: is)) fin.
Proof. sset. Qed.
Lemma c_set5 p a b c t d x st is fin :
  reduce_trans (hs, s, fr p a b c t x, V st ++ to_e_list is) fin ->
  reduce_trans (hs, s, fr p a b c t d, V (VAL_num (VAL_int32 x) :: st)
                 ++ to_e_list (BI_local_set 5%N :: is)) fin.
Proof. sset. Qed.

Ltac stp := first
  [ eapply c_get; [reflexivity |]
  | apply c_i32 | apply c_i64 | apply c_add32 | apply c_add64 | apply c_and32
  | apply c_ne32 | apply c_geu32 | apply c_eqz
  | apply cps_tee
  | apply c_set1 | apply c_set2 | apply c_set3 | apply c_set4 | apply c_set5 ].

Ltac ld := apply (c_load hs s m); [exact Hsmem | |].

Lemma start f es fin :
  reduce_trans (hs, s, f, V [] ++ es) fin -> reduce_trans (hs, s, f, es) fin.
Proof. exact (fun H => H). Qed.

(* ---------------- remainder loop ---------------- *)

Lemma l2_iter p a b c t d fin :
  u32 a + 1 <= Z.of_N (operations.mem_length m) ->
  reduce_trans (hs, s, fr p (add a (rep 1)) (add b (rep (-1))) c
                  (Wasm_int.Int64.add t (bv m a)) d,
                V [VAL_num (VAL_int32 (add b (rep (-1))))] ++ to_e_list [BI_br_if 0%N]) fin ->
  reduce_trans (hs, s, fr p a b c t d, to_e_list l2_body) fin.
Proof.
  intros Ha H. apply start. unfold l2_body.
  do 2 stp. ld; [exact Ha |]. repeat stp. exact H.
Qed.

Definition ML := Z.of_N (operations.mem_length m).

Lemma u64_add_bv t k : u64 t + B m (u32 k) < 2 ^ 64 ->
  u64 (Wasm_int.Int64.add t (bv m k)) = u64 t + B m (u32 k).
Proof.
  intros H. unfold bv. apply u64_add_byte; [apply byte_at_range | exact H].
Qed.

Lemma l2_run (r : nat) : forall p a b c t d J,
  (1 <= r)%nat -> u32 b = Z.of_nat r -> u32 a = u32 p + J -> 0 <= J ->
  u32 p + J + Z.of_nat r <= ML -> u32 p + J + Z.of_nat r <= 2 ^ 32 ->
  u64 t = sumZ m (u32 p) J ->
  exists a' b' t',
    reduce_trans (hs, s, fr p a b c t d, [AI_basic l2_loop])
                 (hs, s, fr p a' b' c t' d, []) /\
    u64 t' = sumZ m (u32 p) (J + Z.of_nat r).
Proof.
  unfold l2_loop, bt0.
  induction r as [|r IH]; intros p a b c t d J Hr Hb Ha HJ Hm Hw Ht; [lia|].
  pose proof (u32_range p) as Hp.
  pose proof (sumZ_range m (u32 p) J HJ) as HS.
  pose proof (B_range m (u32 a)) as HB.
  assert (Hsum : u64 (Wasm_int.Int64.add t (bv m a)) = sumZ m (u32 p) (J + 1)).
  { rewrite u64_add_bv by lia. rewrite sumZ_step by lia. rewrite Ha. lia. }
  destruct r as [|r].
  - exists (add a (rep 1)), (add b (rep (-1))), (Wasm_int.Int64.add t (bv m a)).
    split; [| rewrite Hsum; f_equal; lia].
    apply loop_exit. apply l2_iter; [unfold ML in Hm; lia |].
    apply c_brif_z; [rewrite u32_dec; lia |]. apply rt_refl.
  - destruct (IH p (add a (rep 1)) (add b (rep (-1))) c (Wasm_int.Int64.add t (bv m a))
                d (J + 1)) as (a' & b' & t' & Hrun & Hs); try lia.
    + rewrite u32_dec; lia.
    + rewrite u32_add_repr; lia.
    + exists a', b', t'. split; [| rewrite Hs; f_equal; lia].
      apply loop_again with (rest := [])
        (f' := fr p (add a (rep 1)) (add b (rep (-1))) c (Wasm_int.Int64.add t (bv m a)) d);
        [| exact Hrun].
      apply l2_iter; [unfold ML in Hm; lia |].
      apply c_brif_nz; [rewrite u32_dec; lia |]. apply rt_refl.
Qed.

(* ---------------- unrolled main loop ---------------- *)

Definition acc4 t a :=
  Wasm_int.Int64.add (Wasm_int.Int64.add (Wasm_int.Int64.add
    (Wasm_int.Int64.add t (bv m a)) (bv m (add a (rep 1))))
    (bv m (add a (rep 2)))) (bv m (add a (rep 3))).

Lemma l1_iter p l1 rem i t n4 fin :
  u32 p + u32 i + 4 <= ML -> u32 p + u32 i + 4 <= 2 ^ 32 ->
  reduce_trans (hs, s, fr p (add p i) rem (add i (rep 4)) (acc4 t (add p i)) n4,
                V [VAL_num (VAL_int32 (wasm_bool (negb (Wasm_int.Int32.eq n4 (add i (rep 4))))))]
                ++ to_e_list [BI_br_if 0%N]) fin ->
  reduce_trans (hs, s, fr p l1 rem i t n4, to_e_list l1_body) fin.
Proof.
  intros Hm Hw H. unfold ML in Hm.
  pose proof (u32_range p) as Rp; pose proof (u32_range i) as Ri.
  assert (Ha : u32 (add p i) = u32 p + u32 i) by (apply u32_add; lia).
  assert (H1 : u32 (add (add p i) (rep 1)) = u32 p + u32 i + 1)
    by (rewrite u32_add_repr; lia).
  assert (H2 : u32 (add (add p i) (rep 2)) = u32 p + u32 i + 2)
    by (rewrite u32_add_repr; lia).
  assert (H3 : u32 (add (add p i) (rep 3)) = u32 p + u32 i + 3)
    by (rewrite u32_add_repr; lia).
  apply start. unfold l1_body.
  repeat stp. ld; [lia |].
  repeat stp. ld; [lia |].
  repeat stp. ld; [lia |].
  repeat stp. ld; [lia |].
  repeat stp. exact H.
Qed.

Lemma acc4_sum t a :
  u32 a + 3 < 2 ^ 32 -> u64 t <= 255 * 2 ^ 32 ->
  u64 (acc4 t a) = u64 t + B m (u32 a) + B m (u32 a + 1) + B m (u32 a + 2) + B m (u32 a + 3).
Proof.
  intros Ha Ht. unfold acc4.
  pose proof (u32_range a).
  pose proof (B_range m (u32 a)); pose proof (B_range m (u32 a + 1));
  pose proof (B_range m (u32 a + 2)); pose proof (B_range m (u32 a + 3)).
  assert (E1 : u32 (add a (rep 1)) = u32 a + 1) by (rewrite u32_add_repr; lia).
  assert (E2 : u32 (add a (rep 2)) = u32 a + 2) by (rewrite u32_add_repr; lia).
  assert (E3 : u32 (add a (rep 3)) = u32 a + 3) by (rewrite u32_add_repr; lia).
  assert (S1 : u64 (Wasm_int.Int64.add t (bv m a)) = u64 t + B m (u32 a))
    by (apply u64_add_bv; lia).
  assert (S2 : u64 (Wasm_int.Int64.add (Wasm_int.Int64.add t (bv m a)) (bv m (add a (rep 1))))
               = u64 t + B m (u32 a) + B m (u32 a + 1))
    by (rewrite u64_add_bv; rewrite S1, E1; lia).
  assert (S3 : u64 (Wasm_int.Int64.add (Wasm_int.Int64.add (Wasm_int.Int64.add t (bv m a))
                 (bv m (add a (rep 1)))) (bv m (add a (rep 2))))
               = u64 t + B m (u32 a) + B m (u32 a + 1) + B m (u32 a + 2))
    by (rewrite u64_add_bv; rewrite S2, E2; lia).
  rewrite u64_add_bv; rewrite S3, E3; lia.
Qed.

Lemma sumZ_4 P I : 0 <= P -> 0 <= I ->
  sumZ m P (I + 4) = sumZ m P I + B m (P + I) + B m (P + I + 1) + B m (P + I + 2)
                     + B m (P + I + 3).
Proof.
  intros HP HI.
  replace (I + 4) with ((((I + 1) + 1) + 1) + 1) by lia.
  rewrite !sumZ_step by lia.
  replace (P + (I + 1)) with (P + I + 1) by lia.
  replace (P + (I + 1 + 1)) with (P + I + 2) by lia.
  replace (P + (I + 1 + 1 + 1)) with (P + I + 3) by lia.
  reflexivity.
Qed.

Section L1.
Variables (p rem n4 : i32).

Lemma l1_run (r : nat) : forall l1 i t I,
  (1 <= r)%nat -> u32 i = I -> 0 <= I -> u32 n4 = I + 4 * Z.of_nat r ->
  u32 p + u32 n4 <= ML -> u32 p + u32 n4 <= 2 ^ 32 ->
  u64 t = sumZ m (u32 p) I ->
  exists l1' t',
    reduce_trans (hs, s, fr p l1 rem i t n4, [AI_basic l1_loop])
                 (hs, s, fr p l1' rem n4 t' n4, []) /\
    u64 t' = sumZ m (u32 p) (u32 n4).
Proof.
  unfold l1_loop, bt0.
  induction r as [|r IH]; intros l1 i t I Hr Hi HI Hn Hm Hw Ht; [lia|].
  pose proof (u32_range p) as Hp. pose proof (u32_range n4) as Hn4.
  pose proof (sumZ_range m (u32 p) I HI) as HS.
  assert (Ha : u32 (add p i) = u32 p + u32 i) by (apply u32_add; lia).
  assert (Hi4 : u32 (add i (rep 4)) = I + 4) by (rewrite u32_add_repr; lia).
  assert (Hsum : u64 (acc4 t (add p i)) = sumZ m (u32 p) (I + 4)).
  { rewrite acc4_sum by lia. rewrite sumZ_4 by lia. rewrite Ha, Ht, Hi. lia. }
  destruct r as [|r].
  - assert (E : add i (rep 4) = n4) by (apply i32_eq_u32; lia).
    exists (add p i), (acc4 t (add p i)).
    split; [| rewrite Hsum; f_equal; lia].
    apply loop_exit. apply l1_iter; [lia | lia |].
    rewrite E, Wasm_int.Int32.eq_true.
    apply c_brif_z; [reflexivity |]. apply rt_refl.
  - destruct (IH (add p i) (add i (rep 4)) (acc4 t (add p i)) (I + 4))
      as (l1' & t' & Hrun & Hs); try lia.
    exists l1', t'. split; [| exact Hs].
    apply loop_again with (rest := [])
      (f' := fr p (add p i) rem (add i (rep 4)) (acc4 t (add p i)) n4); [| exact Hrun].
    apply l1_iter; [lia | lia |].
    rewrite (i32_eqb_false n4 (add i (rep 4))); [| lia].
    apply c_brif_nz; [vm_compute; discriminate |]. apply rt_refl.
Qed.
End L1.

(* ---------------- blocks ---------------- *)

Lemma geu_true x : 4 <= u32 x ->
  u32 (wasm_bool (negb (Wasm_int.Int32.ltu x (rep 4)))) <> 0.
Proof.
  intros H. rewrite i32_ltu. rewrite (u32_repr 4) by lia.
  replace (u32 x <? 4) with false by (symmetry; apply Z.ltb_ge; lia).
  vm_compute; discriminate.
Qed.

Lemma geu_false x : u32 x < 4 ->
  u32 (wasm_bool (negb (Wasm_int.Int32.ltu x (rep 4)))) = 0.
Proof.
  intros H. rewrite i32_ltu. rewrite (u32_repr 4) by lia.
  replace (u32 x <? 4) with true by (symmetry; apply Z.ltb_lt; lia).
  reflexivity.
Qed.

Lemma u32_zero : u32 Wasm_int.Int32.zero = 0.
Proof. reflexivity. Qed.

Lemma b0_pass p len b c t d : u32 len <> 0 ->
  reduce_trans (hs, s, fr p len b c t d, [AI_basic (BI_block bt0 b0_body)])
               (hs, s, fr p len b c t d, []).
Proof.
  intros H. unfold bt0. apply block_enter.
  eapply label_br0; [| apply rt_refl].
  apply start. unfold b0_body. repeat stp.
  apply c_brif_nz; [exact H |]. apply rt_refl.
Qed.

Lemma b3_big p len b c t d : 4 <= u32 len ->
  reduce_trans (hs, s, fr p len b c t d, [AI_basic (BI_block bt0 b3_body)])
               (hs, s, fr p len b c t d, []).
Proof.
  intros H. unfold bt0. apply block_enter.
  eapply label_br0; [| apply rt_refl].
  apply start. unfold b3_body. repeat stp.
  apply c_brif_nz; [apply geu_true; exact H |]. apply rt_refl.
Qed.

(* len < 4: the innermost block falls through and branches past the second
   block, skipping the main loop. *)
Lemma b2_small p len b c t d : u32 len < 4 ->
  reduce_trans (hs, s, fr p len b c t d, [AI_basic (BI_block bt0 b2_body)])
               (hs, s, fr p len b (rep 0) (Wasm_int.Int64.repr 0) d, []).
Proof.
  intros H. unfold bt0. apply block_enter.
  change (to_e_list b2_body) with
    (V [] ++ to_e_list (BI_block (BT_valtype None) b3_body :: List.tl b2_body)).
  apply label_block_br1 with (rest := [])
    (f' := fr p len b (rep 0) (Wasm_int.Int64.repr 0) d); [| apply rt_refl].
  apply rt_label. apply start. unfold b3_body. repeat stp.
  apply c_brif_z; [apply geu_false; exact H |]. repeat stp. apply rt_refl.
Qed.

Definition n4of len := Wasm_int.Int32.and len (rep (-4)).

Lemma n4_facts len : 4 <= u32 len ->
  u32 (n4of len) = u32 len - u32 len mod 4 /\
  u32 (n4of len) = 0 + 4 * Z.of_nat (Z.to_nat (u32 len / 4)) /\
  (1 <= Z.to_nat (u32 len / 4))%nat.
Proof.
  intros H. unfold n4of. rewrite u32_andm4.
  pose proof (Z.div_mod (u32 len) 4 ltac:(lia)).
  pose proof (Z.mod_pos_bound (u32 len) 4 ltac:(lia)).
  assert (1 <= u32 len / 4) by (apply Z.div_le_lower_bound; lia).
  rewrite Z2Nat.id by lia. lia.
Qed.

(* len >= 4: run the main loop; [rem_nz] says whether the remainder loop
   follows ([br_if 1] not taken). *)
Lemma b2_main p len rem c t d fin
  (Hc : forall l1' t', u64 t' = sumZ m (u32 p) (u32 (n4of len)) ->
        reduce_trans (hs, s, fr p l1' rem (n4of len) t' (n4of len),
                      V [VAL_num (VAL_int32 (wasm_bool (Wasm_int.Int32.eq Wasm_int.Int32.zero rem)))]
                      ++ to_e_list [BI_br_if 1%N]) fin) :
  4 <= u32 len -> u32 p + u32 len <= ML -> u32 p + u32 len <= 2 ^ 32 ->
  reduce_trans (hs, s, fr p len rem c t d, to_e_list b2_body) fin.
Proof.
  intros H4 Hm Hw.
  destruct (n4_facts len H4) as (Hn & Hn' & Hr).
  destruct (l1_run p rem (n4of len) (Z.to_nat (u32 len / 4)) len (rep 0)
              (Wasm_int.Int64.repr 0) 0) as (l1' & t' & Hrun & Hs);
    try lia; try reflexivity.
  { pose proof (Z.mod_pos_bound (u32 len) 4 ltac:(lia)). lia. }
  { pose proof (Z.mod_pos_bound (u32 len) 4 ltac:(lia)). lia. }
  apply start. unfold b2_body.
  apply cps_sub with (f' := fr p len rem c t d); [apply b3_big; exact H4 |].
  repeat stp. fold (n4of len).
  apply cps_sub with (f' := fr p l1' rem (n4of len) t' (n4of len)); [exact Hrun |].
  repeat stp. apply Hc. exact Hs.
Qed.

Lemma eqz_u32 rem : u32 rem <> 0 ->
  u32 (wasm_bool (Wasm_int.Int32.eq Wasm_int.Int32.zero rem)) = 0.
Proof.
  intros H. rewrite (i32_eqb_false Wasm_int.Int32.zero rem); [reflexivity |].
  rewrite u32_zero. lia.
Qed.

Lemma eqz_u32' rem : u32 rem = 0 ->
  u32 (wasm_bool (Wasm_int.Int32.eq Wasm_int.Int32.zero rem)) <> 0.
Proof.
  intros H. rewrite (i32_eqb_true Wasm_int.Int32.zero rem); [discriminate |].
  rewrite u32_zero. lia.
Qed.

Lemma b1_run p len rem c t d :
  1 <= u32 len -> u32 rem = u32 len mod 4 ->
  u32 p + u32 len <= ML -> u32 p + u32 len <= 2 ^ 32 ->
  exists a' b' c' t' d',
    reduce_trans (hs, s, fr p len rem c t d, [AI_basic (BI_block bt0 b1_body)])
                 (hs, s, fr p a' b' c' t' d', []) /\
    u64 t' = sumZ m (u32 p) (u32 len).
Proof.
  intros H1 Hrem Hm Hw.
  pose proof (u32_range p) as Hp. pose proof (u32_range len) as Hl.
  pose proof (Z.mod_pos_bound (u32 len) 4 ltac:(lia)) as Hmod.
  destruct (Z_lt_le_dec (u32 len) 4) as [Hsmall | Hbig].
  - (* len < 4: remainder loop only *)
    rewrite Z.mod_small in Hrem by lia.
    destruct (l2_run (Z.to_nat (u32 len)) p (add p (rep 0)) rem (rep 0)
                (Wasm_int.Int64.repr 0) d 0) as (a' & b' & t' & Hrun & Hs);
      try rewrite Z2Nat.id; try lia; try reflexivity.
    { rewrite u32_add_repr; lia. }
    exists a', b', (rep 0), t', d. split; [| rewrite Hs; f_equal; rewrite Z2Nat.id; lia].
    unfold bt0. apply block_enter. apply label_done.
    apply start. unfold b1_body.
    apply cps_sub with (f' := fr p len rem (rep 0) (Wasm_int.Int64.repr 0) d);
      [apply b2_small; exact Hsmall |].
    repeat stp.
    apply cps_sub with (f' := fr p a' b' (rep 0) t' d); [exact Hrun |].
    apply rt_refl.
  - destruct (n4_facts len Hbig) as (Hn & _ & _).
    destruct (Z.eq_dec (u32 rem) 0) as [Hz | Hnz].
    + (* remainder 0: leave both blocks after the main loop *)
      assert (Hl1 : exists l1' t',
        reduce_trans (hs, s, fr p len rem c t d, to_e_list b2_body)
          (hs, s, fr p l1' rem (n4of len) t' (n4of len), [AI_basic (BI_br 1%N)]) /\
        u64 t' = sumZ m (u32 p) (u32 (n4of len))).
      { (* the final frame is only known inside the continuation *)
        destruct (n4_facts len Hbig) as (_ & Hn' & Hr).
        destruct (l1_run p rem (n4of len) (Z.to_nat (u32 len / 4)) len (rep 0)
                    (Wasm_int.Int64.repr 0) 0) as (l1' & t' & Hrun & Hs);
          try lia; try reflexivity.
        exists l1', t'. split; [| exact Hs].
        apply start. unfold b2_body.
        apply cps_sub with (f' := fr p len rem c t d); [apply b3_big; exact Hbig |].
        repeat stp. fold (n4of len).
        apply cps_sub with (f' := fr p l1' rem (n4of len) t' (n4of len)); [exact Hrun |].
        repeat stp. apply c_brif_nz; [apply eqz_u32'; exact Hz |]. apply rt_refl. }
      destruct Hl1 as (l1' & t' & Hrun & Hs).
      exists l1', rem, (n4of len), t', (n4of len).
      split; [| rewrite Hs, Hn; f_equal; lia].
      unfold bt0. apply block_enter.
      change (to_e_list b1_body) with
        (V [] ++ to_e_list (BI_block (BT_valtype None) b2_body :: List.tl b1_body)).
      apply label_block_br1 with (rest := [])
        (f' := fr p l1' rem (n4of len) t' (n4of len)); [| apply rt_refl].
      apply rt_label. exact Hrun.
    + (* main loop, then remainder loop *)
      assert (Hl1 : exists l1' t',
        reduce_trans (hs, s, fr p len rem c t d, to_e_list b2_body)
          (hs, s, fr p l1' rem (n4of len) t' (n4of len), []) /\
        u64 t' = sumZ m (u32 p) (u32 (n4of len))).
      { destruct (n4_facts len Hbig) as (_ & Hn' & Hr).
        destruct (l1_run p rem (n4of len) (Z.to_nat (u32 len / 4)) len (rep 0)
                    (Wasm_int.Int64.repr 0) 0) as (l1' & t' & Hrun & Hs);
          try lia; try reflexivity.
        exists l1', t'. split; [| exact Hs].
        apply start. unfold b2_body.
        apply cps_sub with (f' := fr p len rem c t d); [apply b3_big; exact Hbig |].
        repeat stp. fold (n4of len).
        apply cps_sub with (f' := fr p l1' rem (n4of len) t' (n4of len)); [exact Hrun |].
        repeat stp. apply c_brif_z; [apply eqz_u32; exact Hnz |]. apply rt_refl. }
      destruct Hl1 as (l1' & t1 & Hrun1 & Hs1).
      pose proof (u32_range (n4of len)) as HN.
      destruct (l2_run (Z.to_nat (u32 rem)) p (add p (n4of len)) rem (n4of len) t1
                  (n4of len) (u32 (n4of len))) as (a' & b' & t' & Hrun2 & Hs2);
        try rewrite Z2Nat.id; try lia.
      { apply u32_add. lia. }
      exists a', b', (n4of len), t', (n4of len).
      split; [| rewrite Hs2; f_equal; rewrite Z2Nat.id; lia].
      unfold bt0. apply block_enter. apply label_done.
      apply start. unfold b1_body.
      apply cps_sub with (f' := fr p l1' rem (n4of len) t1 (n4of len));
        [unfold bt0; apply block_enter; apply label_done; exact Hrun1 |].
      repeat stp.
      apply cps_sub with (f' := fr p a' b' (n4of len) t' (n4of len)); [exact Hrun2 |].
      apply rt_refl.
Qed.

Lemma body_run p len b c t d :
  1 <= u32 len -> u32 p + u32 len <= ML -> u32 p + u32 len <= 2 ^ 32 ->
  exists f' r,
    reduce_trans (hs, s, fr p len b c t d, to_e_list byte_sum_body)
                 (hs, s, f', [$VN (VAL_int64 r)]) /\
    u64 r = sumZ m (u32 p) (u32 len).
Proof.
  intros H1 Hm Hw.
  destruct (b1_run p len (Wasm_int.Int32.and len (rep 3)) c t d)
    as (a' & b' & c' & t' & d' & Hrun & Hs); try lia.
  { apply u32_and3. }
  exists (fr p a' b' c' t' d'), t'. split; [| exact Hs].
  apply start. unfold byte_sum_body.
  apply cps_sub with (f' := fr p len b c t d); [apply b0_pass; lia |].
  repeat stp.
  apply cps_sub with (f' := fr p a' b' c' t' d'); [exact Hrun |].
  repeat stp. apply rt_refl.
Qed.

Lemma body_zero caller p len b c t d : u32 len = 0 ->
  reduce_trans (hs, s, caller, [AI_frame 1 (fr p len b c t d)
                                  [AI_label 1 [] (to_e_list byte_sum_body)]])
               (hs, s, caller, [$VN (VAL_int64 (Wasm_int.Int64.repr 0))]).
Proof.
  intros H0.
  eapply run_trans.
  { apply rt_frame. apply rt_label.
    change (to_e_list byte_sum_body) with
      (v_to_e_list [] ++ [AI_basic (BI_block bt0 b0_body)] ++ to_e_list (List.tl byte_sum_body)).
    eapply run_trans.
    { apply rt_ctx. apply rt_step.
      eapply r_block with (vs := []) (t1s := []) (t2s := []); reflexivity. }
    apply rt_ctx. apply rt_label. apply start. unfold b0_body. repeat stp.
    apply c_brif_z; [exact H0 |]. repeat stp. apply rt_refl. }
  apply rt_step, r_simple.
  eapply rs_return with (vs := [$VN (VAL_int64 (Wasm_int.Int64.repr 0))])
    (lh := LH_rec [] 1 [] (LH_rec [] 0 [] (LH_base [] []) (to_e_list (List.tl byte_sum_body))) []);
    reflexivity.
Qed.

Lemma invoke_run caller addr p len r :
  lookup_N (s_funcs s) addr =
    Some (FC_func_native (Tf [T_num T_i32; T_num T_i32] [T_num T_i64]) inst byte_sum_code) ->
  reduce_trans (hs, s, caller,
     [AI_frame 1 (fr p len Wasm_int.Int32.zero Wasm_int.Int32.zero Wasm_int.Int64.zero
                     Wasm_int.Int32.zero) [AI_label 1 [] (to_e_list byte_sum_body)]])
    (hs, s, caller, [$VN (VAL_int64 r)]) ->
  reduce_trans (hs, s, caller, [$VN (VAL_int32 p); $VN (VAL_int32 len); AI_invoke addr])
               (hs, s, caller, [$VN (VAL_int64 r)]).
Proof.
  intros Hfunc H.
  eapply run_step; [| exact H].
  eapply r_invoke_native with
    (cl := FC_func_native (Tf [T_num T_i32; T_num T_i32] [T_num T_i64]) inst byte_sum_code)
    (ts1 := [T_num T_i32; T_num T_i32]) (ts2 := [T_num T_i64])
    (code := byte_sum_code) (x := 16%N) (ts := byte_sum_locals)
    (es := byte_sum_body) (inst := inst)
    (ves := [$VN (VAL_int32 p); $VN (VAL_int32 len)])
    (vs := [VAL_num (VAL_int32 p); VAL_num (VAL_int32 len)])
    (n := 2%nat) (m := 1%nat) (k := 4%nat)
    (defaults := [VAL_num (VAL_int32 Wasm_int.Int32.zero); VAL_num (VAL_int32 Wasm_int.Int32.zero);
                  VAL_num (VAL_int64 Wasm_int.Int64.zero); VAL_num (VAL_int32 Wasm_int.Int32.zero)]);
    try reflexivity; exact Hfunc.
Qed.

Lemma call_run caller funcaddr p len :
  lookup_N (inst_funcs (f_inst caller)) 67 = Some funcaddr ->
  lookup_N (s_funcs s) funcaddr =
    Some (FC_func_native (Tf [T_num T_i32; T_num T_i32] [T_num T_i64]) inst byte_sum_code) ->
  u32 p + u32 len <= ML -> u32 p + u32 len <= 2 ^ 32 ->
  exists r,
    reduce_trans (hs, s, caller,
        [$VN (VAL_int32 p); $VN (VAL_int32 len); AI_basic (BI_call 67%N)])
      (hs, s, caller, [$VN (VAL_int64 r)]) /\
    u64 r = sumZ m (u32 p) (u32 len).
Proof.
  intros Hcall Hfunc Hm Hw.
  assert (Hinv : forall r,
    reduce_trans (hs, s, caller,
     [AI_frame 1 (fr p len Wasm_int.Int32.zero Wasm_int.Int32.zero Wasm_int.Int64.zero
                     Wasm_int.Int32.zero) [AI_label 1 [] (to_e_list byte_sum_body)]])
    (hs, s, caller, [$VN (VAL_int64 r)]) ->
    reduce_trans (hs, s, caller,
        [$VN (VAL_int32 p); $VN (VAL_int32 len); AI_basic (BI_call 67%N)])
      (hs, s, caller, [$VN (VAL_int64 r)])).
  { intros r H.
    change [$VN (VAL_int32 p); $VN (VAL_int32 len); AI_basic (BI_call 67%N)] with
      (v_to_e_list [VAL_num (VAL_int32 p); VAL_num (VAL_int32 len)]
        ++ [AI_basic (BI_call 67%N)] ++ []).
    eapply step_ctx_cps; [apply r_call; exact Hcall |].
    apply invoke_run; [exact Hfunc | exact H]. }
  destruct (Z.eq_dec (u32 len) 0) as [H0 | Hnz].
  - exists (Wasm_int.Int64.repr 0). split.
    + apply Hinv. apply body_zero. exact H0.
    + rewrite H0. reflexivity.
  - destruct (body_run p len Wasm_int.Int32.zero Wasm_int.Int32.zero Wasm_int.Int64.zero
                Wasm_int.Int32.zero) as (f' & r & Hrun & Hs); try lia.
    { pose proof (u32_range len). lia. }
    exists r. split; [| exact Hs].
    apply Hinv.
    eapply run_trans; [apply rt_frame, rt_label; exact Hrun |].
    eapply run_step; [apply r_frame, r_simple, rs_label_const; reflexivity |].
    apply rt_step, r_simple, rs_local_const; reflexivity.
Qed.

End Exec.

(* ---------------- the function guarantee ---------------- *)

Section Guarantee.
Context {hfc : host_function_class} {bum : BlockUpdateMemory}
        {ho : @host hfc bum}.

Definition byte_sum_guarantee (code : module_func) : Prop :=
  forall hs s caller inst funcaddr memaddr m p len,
    lookup_N (inst_funcs (f_inst caller)) 67 = Some funcaddr ->
    lookup_N (s_funcs s) funcaddr =
      Some (FC_func_native (Tf [T_num T_i32; T_num T_i32] [T_num T_i64]) inst code) ->
    lookup_N (inst_mems inst) 0 = Some memaddr ->
    lookup_N (s_mems s) memaddr = Some m ->
    (Wasm_int.N_of_uint i32m p + Wasm_int.N_of_uint i32m len <= operations.mem_length m)%N ->
    (* The accessed range ends inside the 32-bit address space.  Any real wasm32
       memory has at most 2^32 bytes, so this follows from the previous
       hypothesis there; WasmCert's [meminst] does not bound its length. *)
    (Wasm_int.N_of_uint i32m p + Wasm_int.N_of_uint i32m len <= 2 ^ 32)%N ->
    exists r : i64,
      reduce_trans
        (hs, s, caller,
         [$VN (VAL_int32 p); $VN (VAL_int32 len); AI_basic (BI_call 67%N)])
        (hs, s, caller, [$VN (VAL_int64 r)]) /\
      Wasm_int.Int64.unsigned r =
        mem_sum m (Wasm_int.N_of_uint i32m p) (Wasm_int.N_of_uint i32m len).

Open Scope Z_scope.

Theorem byte_sum_function_guarantee : byte_sum_guarantee byte_sum_code.
Proof.
  intros hs s caller inst funcaddr memaddr m p len Hcall Hfunc Haddr Hmem Hb Hw.
  assert (Hsmem : smem s inst = Some m) by (unfold smem; rewrite Haddr; exact Hmem).
  change (Wasm_int.N_of_uint i32m p) with (Z.to_N (u32 p)) in *.
  change (Wasm_int.N_of_uint i32m len) with (Z.to_N (u32 len)) in *.
  pose proof (u32_range p); pose proof (u32_range len).
  assert (Hm : u32 p + u32 len <= ML m).
  { unfold ML. apply N2Z.inj_le in Hb. rewrite N2Z.inj_add, !Z2N.id in Hb; lia. }
  assert (Hw' : u32 p + u32 len <= 2 ^ 32).
  { apply N2Z.inj_le in Hw. rewrite N2Z.inj_add, !Z2N.id in Hw; [| lia | lia].
    exact Hw. }
  destruct (call_run hs s inst m Hsmem caller funcaddr p len Hcall Hfunc Hm Hw')
    as (r & Hrun & Hs).
  exists r. split; [exact Hrun |].
  change (Wasm_int.Int64.unsigned r) with (u64 r). rewrite Hs.
  unfold sumZ, mem_sum. rewrite Z_N_nat. reflexivity.
Qed.

(* The final theorem combines decoding with the complete function guarantee. *)
Theorem packaged_byte_sum :
  exists code, byte_sum_selected_code = Some code /\ byte_sum_guarantee code.
Proof.
  exists byte_sum_code; split.
  - apply byte_sum_decodes.
  - apply byte_sum_function_guarantee.
Qed.

End Guarantee.
