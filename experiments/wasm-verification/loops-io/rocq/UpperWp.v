(* A weakest-precondition style layer over the [exec] judgement of UpperSem,
   plus canonical single-instruction steps for the i32 operations used by
   [_start].  Everything here is derived from [exec]'s constructors, hence
   ultimately from WasmCert's [reduce] (via [exec_sound]). *)
From Stdlib Require Import List ZArith NArith Lia.
From Wasm Require Import opsem.
Require Import UpperSem.
Import ListNotations.

Section Wp.
Context {hfc : host_function_class} {bum : BlockUpdateMemory}
        {ho : @host hfc bum}.
Notation vi x := (VAL_num (VAL_int32 x)).

Lemma exec_app hs s f st a hs1 s1 f1 o :
  exec hs s f st a hs1 s1 f1 o ->
  forall b,
  match o with
  | Norm st1 => forall hs2 s2 f2 o2,
      exec hs1 s1 f1 st1 b hs2 s2 f2 o2 -> exec hs s f st (a ++ b) hs2 s2 f2 o2
  | Brk j => exec hs s f st (a ++ b) hs1 s1 f1 (Brk j)
  end.
Proof.
  induction 1; intros b0; simpl;
    repeat match goal with
           | IH : forall b : list basic_instruction, _ |- _ => specialize (IH b0)
           end;
    try (destruct o; simpl in *; intros; eauto using exec; fail);
    try (intros; assumption); try (constructor; assumption).
Qed.

Definition wp hs s f st bes
  (Q : host_state -> store_record -> frame -> outcome -> Prop) : Prop :=
  exists hs' s' f' o, exec hs s f st bes hs' s' f' o /\ Q hs' s' f' o.

Lemma wp_mono hs s f st bes (Q Q' : host_state -> store_record -> frame -> outcome -> Prop) :
  (forall a b c d, Q a b c d -> Q' a b c d) -> wp hs s f st bes Q -> wp hs s f st bes Q'.
Proof. intros H [a [b [c [d [E HQ]]]]]. exists a, b, c, d. split; auto. Qed.

Lemma wp_nil hs s f st Q : Q hs s f (Norm st) -> wp hs s f st [] Q.
Proof. intros H. exists hs, s, f, (Norm st). split; [constructor | exact H]. Qed.

Lemma wp_step hs s f st i rest s1 f1 st1 Q :
  sstep s f st i s1 f1 st1 -> wp hs s1 f1 st1 rest Q -> wp hs s f st (i :: rest) Q.
Proof.
  intros H [a [b [c [d [E HQ]]]]]. exists a, b, c, d. split; [econstructor; eauto | exact HQ].
Qed.

Lemma wp_br hs s f st j rest Q :
  Q hs s f (Brk (N.to_nat j)) -> wp hs s f st (BI_br j :: rest) Q.
Proof. intros H. do 4 eexists. split; [apply ex_br | exact H]. Qed.

Lemma wp_br_if_true hs s f st c j rest Q :
  c <> Wasm_int.Int32.zero -> Q hs s f (Brk (N.to_nat j)) ->
  wp hs s f (vi c :: st) (BI_br_if j :: rest) Q.
Proof. intros Hc H. do 4 eexists. split; [apply ex_br_if_true; exact Hc | exact H]. Qed.

Lemma wp_br_if_false hs s f st c j rest Q :
  c = Wasm_int.Int32.zero -> wp hs s f st rest Q ->
  wp hs s f (vi c :: st) (BI_br_if j :: rest) Q.
Proof.
  intros Hc [a [b [c' [d [E HQ]]]]]. exists a, b, c', d.
  split; [apply ex_br_if_false; assumption | exact HQ].
Qed.

Definition after_block st rest (Q : host_state -> store_record -> frame -> outcome -> Prop)
  hs1 s1 f1 o : Prop :=
  match o with
  | Norm [] => wp hs1 s1 f1 st rest Q
  | Norm _ => False
  | Brk 0 => wp hs1 s1 f1 st rest Q
  | Brk (S j) => Q hs1 s1 f1 (Brk j)
  end.

Lemma wp_block hs s f st body rest Q :
  wp hs s f [] body (after_block st rest Q) ->
  wp hs s f st (BI_block BTnone body :: rest) Q.
Proof.
  intros [hs1 [s1 [f1 [o [E H]]]]].
  destruct o as [[|v l]|[|j]]; simpl in H; try contradiction.
  - destruct H as [a [b [c [d [E2 HQ]]]]]. exists a, b, c, d.
    split; [eapply ex_block_norm; eauto | exact HQ].
  - destruct H as [a [b [c [d [E2 HQ]]]]]. exists a, b, c, d.
    split; [eapply ex_block_br0; eauto | exact HQ].
  - exists hs1, s1, f1, (Brk j). split; [eapply ex_block_brS; eauto | exact H].
Qed.

Definition after_loop st body rest (Q : host_state -> store_record -> frame -> outcome -> Prop)
  hs1 s1 f1 o : Prop :=
  match o with
  | Norm [] => wp hs1 s1 f1 st rest Q
  | Norm _ => False
  | Brk 0 => wp hs1 s1 f1 st (BI_loop BTnone body :: rest) Q
  | Brk (S j) => Q hs1 s1 f1 (Brk j)
  end.

Lemma wp_loop hs s f st body rest Q :
  wp hs s f [] body (after_loop st body rest Q) ->
  wp hs s f st (BI_loop BTnone body :: rest) Q.
Proof.
  intros [hs1 [s1 [f1 [o [E H]]]]].
  destruct o as [[|v l]|[|j]]; simpl in H; try contradiction.
  - destruct H as [a [b [c [d [E2 HQ]]]]]. exists a, b, c, d.
    split; [eapply ex_loop_norm; eauto | exact HQ].
  - destruct H as [a [b [c [d [E2 HQ]]]]]. exists a, b, c, d.
    split; [eapply ex_loop_br0; eauto | exact HQ].
  - exists hs1, s1, f1, (Brk j). split; [eapply ex_loop_brS; eauto | exact H].
Qed.

Definition after_app (st : list value) b (Q : host_state -> store_record -> frame -> outcome -> Prop)
  hs1 s1 f1 o : Prop :=
  match o with
  | Norm st1 => wp hs1 s1 f1 st1 b Q
  | Brk j => Q hs1 s1 f1 (Brk j)
  end.

Lemma wp_app hs s f st a b Q :
  wp hs s f st a (after_app st b Q) -> wp hs s f st (a ++ b) Q.
Proof.
  intros [hs1 [s1 [f1 [o [E H]]]]].
  pose proof (exec_app _ _ _ _ _ _ _ _ _ E b) as X.
  destruct o as [st1|j]; simpl in H.
  - destruct H as [a' [b' [c [d [E2 HQ]]]]]. exists a', b', c, d. split; eauto.
  - exists hs1, s1, f1, (Brk j). split; eauto.
Qed.

Lemma wp_call_host hs s f st i a h a1 a2 a3 a4 hs1 s1 r rest Q :
  lookup_N (inst_funcs (f_inst f)) i = Some a ->
  lookup_N (s_funcs s) a = Some (FC_func_host T4 h) ->
  host_application hs s T4 h [a1; a2; a3; a4] hs1 (Some (s1, result_values [r])) ->
  wp hs1 s1 f (r :: st) rest Q ->
  wp hs s f (a4 :: a3 :: a2 :: a1 :: st) (BI_call i :: rest) Q.
Proof.
  intros H1 H2 H3 [x [y [z [w [E HQ]]]]]. exists x, y, z, w.
  split; [eapply ex_call_host; eauto | exact HQ].
Qed.

Corollary wp_sound hs s f bes Q :
  wp hs s f [] bes Q ->
  exists hs' s' f', Q hs' s' f' (Norm []) /\ reduce_trans (hs, s, f, to_e_list bes) (hs', s', f', [])
  \/ (exists o, o <> Norm [] /\ Q hs' s' f' o).
Proof.
  intros [hs' [s' [f' [o [E HQ]]]]]. exists hs', s', f'.
  destruct o as [[|v l]|j].
  - left. split; [exact HQ | apply exec_sound_norm; exact E].
  - right. exists (Norm (v :: l)). split; [discriminate | exact HQ].
  - right. exists (Brk j). split; [discriminate | exact HQ].
Qed.

(* ---------- canonical single steps ---------- *)

Lemma ss_add s f st x y :
  sstep s f (vi y :: vi x :: st) (BI_binop T_i32 (Binop_i BOI_add)) s f
    (vi (Wasm_int.Int32.add x y) :: st).
Proof. eapply ss_binop; reflexivity. Qed.

Lemma ss_sub s f st x y :
  sstep s f (vi y :: vi x :: st) (BI_binop T_i32 (Binop_i BOI_sub)) s f
    (vi (Wasm_int.Int32.sub x y) :: st).
Proof. eapply ss_binop; reflexivity. Qed.

Lemma ss_and s f st x y :
  sstep s f (vi y :: vi x :: st) (BI_binop T_i32 (Binop_i BOI_and)) s f
    (vi (Wasm_int.Int32.and x y) :: st).
Proof. eapply ss_binop; reflexivity. Qed.

Lemma ss_xor s f st x y :
  sstep s f (vi y :: vi x :: st) (BI_binop T_i32 (Binop_i BOI_xor)) s f
    (vi (Wasm_int.Int32.xor x y) :: st).
Proof. eapply ss_binop; reflexivity. Qed.

Lemma ss_ltu s f st x y :
  sstep s f (vi y :: vi x :: st) (BI_relop T_i32 (Relop_i (ROI_lt SX_U))) s f
    (vi (wasm_bool (Wasm_int.Int32.ltu x y)) :: st).
Proof. apply ss_relop; reflexivity. Qed.

Lemma ss_eq s f st x y :
  sstep s f (vi y :: vi x :: st) (BI_relop T_i32 (Relop_i ROI_eq)) s f
    (vi (wasm_bool (Wasm_int.Int32.eq x y)) :: st).
Proof. apply ss_relop; reflexivity. Qed.

Lemma ss_ne s f st x y :
  sstep s f (vi y :: vi x :: st) (BI_relop T_i32 (Relop_i ROI_ne)) s f
    (vi (wasm_bool (negb (Wasm_int.Int32.eq x y))) :: st).
Proof. apply ss_relop; reflexivity. Qed.

Lemma ss_eqz s f st x :
  sstep s f (vi x :: st) (BI_testop T_i32 TO_eqz) s f
    (vi (wasm_bool (Wasm_int.Int32.eq Wasm_int.Int32.zero x)) :: st).
Proof. apply ss_testop. Qed.

End Wp.
