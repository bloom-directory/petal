From Stdlib Require Import List ZArith Lia Relations.Relation_Operators.
From mathcomp Require Import seq.
From Wasm Require Import opsem.
Require Import Arithmetic MemoryFacts Artifact.
Import ListNotations.
Local Open Scope N_scope.

Section Execution.
Context {hfc : host_function_class} {bum : BlockUpdateMemory}
        {ho : @host hfc bum}.

Lemma run_step hs s f es hs' s' f' es' finish :
  reduce hs s f es hs' s' f' es' ->
  reduce_trans (hs', s', f', es') finish ->
  reduce_trans (hs, s, f, es) finish.
Proof.
  intros Hstep Hrest.
  eapply rt_trans with (y := (hs', s', f', es')).
  - apply rt_step; exact Hstep.
  - exact Hrest.
Qed.

(* A convenience rule derived from WasmCert's own evaluation-context rule. *)
Lemma step_context hs s f es hs' s' f' es' vs tail :
  reduce hs s f es hs' s' f' es' ->
  reduce hs s f (v_to_e_list vs ++ es ++ tail)
    hs' s' f' (v_to_e_list vs ++ es' ++ tail).
Proof.
  intros Hstep.
  eapply r_label with (lh := LH_base vs tail); eauto; reflexivity.
Qed.

Arguments step_context {hs s f es hs' s' f' es' vs tail} _.

Lemma step_get hs s f prefix j v tail :
  lookup_N (f_locs f) j = Some v ->
  reduce hs s f (v_to_e_list prefix ++ AI_basic (BI_local_get j) :: tail)
    hs s f (v_to_e_list prefix ++ v_to_e v :: tail).
Proof.
  intros Hget.
  apply (step_context (vs := prefix) (tail := tail)
    (es := [AI_basic (BI_local_get j)]) (es' := [v_to_e v])).
  apply r_local_get; assumption.
Qed.

Lemma step_bin hs s f prefix x y z op tail :
  binop_typecheck x y T_i64 op = true ->
  app_binop op x y = Some z ->
  reduce hs s f
    (v_to_e_list prefix ++
      [$VN x; $VN y; AI_basic (BI_binop T_i64 op)] ++ tail)
    hs s f (v_to_e_list prefix ++ [$VN z] ++ tail).
Proof.
  intros Htype Hvalue.
  apply step_context, r_simple.
  eapply rs_binop_success; eauto.
Qed.

Lemma step_store hs s f s' prefix p v off tail :
  smem_store s (f_inst f) (Wasm_int.N_of_uint i32m p) off
    (VAL_int64 v) T_i64 = Some s' ->
  reduce hs s f
    (v_to_e_list prefix ++
      [$VN (VAL_int32 p); $VN (VAL_int64 v);
       AI_basic (BI_store T_i64 None (Build_memarg off 3%N))] ++ tail)
    hs s' f (v_to_e_list prefix ++ tail).
Proof.
  intros Hstore.
  apply (step_context (vs := prefix) (es' := [])).
  apply r_store_success; exact Hstore.
Qed.

Definition target_frame inst p t :=
  Build_frame [VAL_num (VAL_int32 p); VAL_num (VAL_int64 t);
               VAL_num (VAL_int64 I.zero)] inst.

Definition result_frame inst p t :=
  Build_frame [VAL_num (VAL_int32 p); VAL_num (VAL_int64 t);
               VAL_num (VAL_int64 (half t))] inst.

(* The store equalities are discharged from the memory precondition below;
   they are not assumptions of the final theorem. *)
Lemma correct_body_execution hs s s1 s2 inst p t :
  smem_store s inst (Wasm_int.N_of_uint i32m p) 0
    (VAL_int64 (half t)) T_i64 = Some s1 ->
  smem_store s1 inst (Wasm_int.N_of_uint i32m p) 8
    (VAL_int64 (remainder t)) T_i64 = Some s2 ->
  reduce_trans (hs, s, target_frame inst p t, to_e_list correct_body)
    (hs, s2, result_frame inst p t, []).
Proof.
  intros Hstore1 Hstore2.
  cbn [correct_body to_e_list].
  eapply run_step.
  { apply step_get with (prefix := []). reflexivity. }
  eapply run_step.
  { apply step_get with (prefix := [VAL_num (VAL_int32 p)]).
    reflexivity. }
  eapply run_step.
  {
    apply step_bin with (prefix := [VAL_num (VAL_int32 p)])
      (z := VAL_int64 (half t)); reflexivity. }
  eapply run_step.
  {
    apply (step_context (vs := [VAL_num (VAL_int32 p)])
      (es := [$VN (VAL_int64 (half t)); AI_basic (BI_local_tee 2%N)])
      (es' := [$VN (VAL_int64 (half t)); $VN (VAL_int64 (half t));
               AI_basic (BI_local_set 2%N)])).
    apply r_simple.
    apply rs_local_tee with (v := VAL_num (VAL_int64 (half t))). }
  eapply run_step.
  {
    apply (step_context
      (vs := [VAL_num (VAL_int32 p); VAL_num (VAL_int64 (half t))])
      (es := [$VN (VAL_int64 (half t)); AI_basic (BI_local_set 2%N)])
      (es' := [])).
    apply r_local_set with (v := VAL_num (VAL_int64 (half t)))
      (vd := VAL_num (VAL_int64 I.zero))
      (f' := result_frame inst p t); simpl; auto. }
  eapply run_step.
  { apply step_store with (prefix := []). exact Hstore1. }
  eapply run_step.
  { apply step_get with (prefix := []). reflexivity. }
  eapply run_step.
  { apply step_get with (prefix := [VAL_num (VAL_int32 p)]).
    reflexivity. }
  eapply run_step.
  {
    apply step_get with (prefix := [VAL_num (VAL_int32 p); VAL_num (VAL_int64 t)]).
    reflexivity. }
  eapply run_step.
  {
    apply step_bin with (prefix := [VAL_num (VAL_int32 p)])
      (z := VAL_int64 (remainder t)); reflexivity. }
  apply rt_step. apply step_store with (prefix := []). exact Hstore2.
Qed.

Lemma nth_error_set_same {A} (xs : list A) n (d v : A) :
  Nat.lt n (List.length xs) ->
  List.nth_error (set_nth d xs n v) n = Some v.
Proof.
  revert xs; induction n as [|n IH]; intros [|x xs] Hlen; simpl in *;
    try lia.
  - reflexivity.
  - apply IH; lia.
Qed.

Lemma memory_store_effect s inst addr m n off v :
  lookup_N (inst_mems inst) 0 = Some addr ->
  lookup_N (s_mems s) addr = Some m ->
  (n + off + 8 <= operations.mem_length m)%N ->
  exists s' m',
    smem_store s inst n off (VAL_int64 v) T_i64 = Some s' /\
    lookup_N (s_mems s') addr = Some m' /\
    operations.mem_length m' = operations.mem_length m /\
    bytes_at m' (n + off) (serialise_i64 v) /\
    (forall q, (q < n + off \/ n + off + 8 <= q)%N ->
       mem_lookup q (meminst_data m') = mem_lookup q (meminst_data m)).
Proof.
  intros Haddr Hmem Hbound.
  destruct (store_i64_success m n off v Hbound)
    as [m' [Hstore [Hlen [Hbytes Hframe]]]].
  exists (upd_s_mem s (set_nth m' (s_mems s) (N.to_nat addr) m')), m'.
  split.
  - unfold smem_store; rewrite Haddr, Hmem; simpl.
    rewrite Hstore; reflexivity.
  - split.
    + unfold lookup_N; simpl.
      apply nth_error_set_same.
      apply List.nth_error_Some; unfold lookup_N in Hmem.
      rewrite Hmem; discriminate.
    + auto.
Qed.

Theorem split_even_correct hs s inst addr m p t :
  lookup_N (inst_mems inst) 0 = Some addr ->
  lookup_N (s_mems s) addr = Some m ->
  (Wasm_int.N_of_uint i32m p + 16 <= operations.mem_length m)%N ->
  exists s' m',
    reduce_trans (hs, s, target_frame inst p t, to_e_list correct_body)
      (hs, s', result_frame inst p t, []) /\
    lookup_N (s_mems s') addr = Some m' /\
    operations.mem_length m' = operations.mem_length m /\
    bytes_at m' (Wasm_int.N_of_uint i32m p) (serialise_i64 (half t)) /\
    bytes_at m' (Wasm_int.N_of_uint i32m p + 8) (serialise_i64 (remainder t)) /\
    shares_correct (I.unsigned t) (I.unsigned (half t))
      (I.unsigned (remainder t)) /\
    (forall q, (q < Wasm_int.N_of_uint i32m p \/
                Wasm_int.N_of_uint i32m p + 16 <= q)%N ->
       mem_lookup q (meminst_data m') = mem_lookup q (meminst_data m)).
Proof.
  intros Haddr Hmem Hbound.
  destruct (memory_store_effect s inst addr m
    (Wasm_int.N_of_uint i32m p) 0 (half t) Haddr Hmem ltac:(lia))
    as [s1 [m1 [Hs1 [Hm1 [Hlen1 [Hb1 Hframe1]]]]]].
  destruct (memory_store_effect s1 inst addr m1
    (Wasm_int.N_of_uint i32m p) 8 (remainder t) Haddr Hm1 ltac:(lia))
    as [s2 [m2 [Hs2 [Hm2 [Hlen2 [Hb2 Hframe2]]]]]].
  exists s2, m2.
  split; [eapply correct_body_execution; eauto |].
  split; [assumption |].
  split; [lia |].
  split.
  - replace (Wasm_int.N_of_uint i32m p) with
      (Wasm_int.N_of_uint i32m p + 0)%N by lia.
    eapply bytes_at_preserved; [exact Hb1 | exact Hframe2 |].
    rewrite serialise_i64_length; left; lia.
  - split; [exact Hb2 |].
    split; [apply split_arithmetic |].
    intros q Houtside.
    rewrite Hframe2 by (destruct Houtside; [left | right]; lia).
    apply Hframe1; destruct Houtside; [left | right]; lia.
Qed.

Definition wrapped (caller : frame) (cfg : host_state * store_record * frame *
                          list administrative_instruction) :=
  let '(hs, s, f, es) := cfg in
  (hs, s, caller, [AI_frame 0%nat f [AI_label 0%nat [] es]]).

Lemma wrap_execution caller c c' :
  reduce_trans c c' -> reduce_trans (wrapped caller c) (wrapped caller c').
Proof.
  intros Hrun; induction Hrun.
  - apply rt_step.
    destruct x as [[[hs s] f] es], y as [[[hs' s'] f'] es']; simpl in *.
    apply r_frame.
    eapply r_label with (lh := LH_rec [] 0%nat [] (LH_base [] []) []).
    + exact H.
    + simpl; rewrite List.app_nil_r; reflexivity.
    + simpl; rewrite List.app_nil_r; reflexivity.
  - apply rt_refl.
  - eapply rt_trans; eauto.
Qed.

Lemma invoke_correct hs s s' caller inst addr p t :
  lookup_N (s_funcs s) addr =
    Some (FC_func_native (Tf [T_num T_i32; T_num T_i64] []) inst correct_code) ->
  reduce_trans (hs, s, target_frame inst p t, to_e_list correct_body)
    (hs, s', result_frame inst p t, []) ->
  reduce_trans
    (hs, s, caller, [$VN (VAL_int32 p); $VN (VAL_int64 t); AI_invoke addr])
    (hs, s', caller, []).
Proof.
  intros Hfunc Hrun.
  eapply run_step.
  {
    eapply r_invoke_native with
      (cl := FC_func_native (Tf [T_num T_i32; T_num T_i64] []) inst correct_code)
      (ts1 := [T_num T_i32; T_num T_i64]) (ts2 := [])
      (code := correct_code) (x := 16%N) (ts := [T_num T_i64])
      (es := correct_body) (inst := inst)
      (ves := [$VN (VAL_int32 p); $VN (VAL_int64 t)])
      (vs := [VAL_num (VAL_int32 p); VAL_num (VAL_int64 t)])
      (n := 2%nat) (m := 0%nat) (k := 1%nat)
      (defaults := [VAL_num (VAL_int64 I.zero)]);
      try reflexivity; exact Hfunc. }
  eapply rt_trans.
  { exact (wrap_execution caller _ _ Hrun). }
  cbn [wrapped].
  eapply run_step.
  { apply r_frame, r_simple, rs_label_const; reflexivity. }
  apply rt_step, r_simple, rs_local_const; reflexivity.
Qed.

Definition function_guarantee (code : module_func) : Prop :=
  forall hs s caller inst funcaddr memaddr m p t,
    lookup_N (inst_funcs (f_inst caller)) 67 = Some funcaddr ->
    lookup_N (s_funcs s) funcaddr =
      Some (FC_func_native (Tf [T_num T_i32; T_num T_i64] []) inst code) ->
    lookup_N (inst_mems inst) 0 = Some memaddr ->
    lookup_N (s_mems s) memaddr = Some m ->
    (Wasm_int.N_of_uint i32m p + 16 <= operations.mem_length m)%N ->
    exists s' m',
      reduce_trans
        (hs, s, caller,
         [$VN (VAL_int32 p); $VN (VAL_int64 t); AI_basic (BI_call 67%N)])
        (hs, s', caller, []) /\
      lookup_N (s_mems s') memaddr = Some m' /\
      operations.mem_length m' = operations.mem_length m /\
      bytes_at m' (Wasm_int.N_of_uint i32m p) (serialise_i64 (half t)) /\
      bytes_at m' (Wasm_int.N_of_uint i32m p + 8) (serialise_i64 (remainder t)) /\
      shares_correct (I.unsigned t) (I.unsigned (half t))
        (I.unsigned (remainder t)) /\
      (forall q, (q < Wasm_int.N_of_uint i32m p \/
                  Wasm_int.N_of_uint i32m p + 16 <= q)%N ->
         mem_lookup q (meminst_data m') = mem_lookup q (meminst_data m)).

Theorem correct_function_guarantee : function_guarantee correct_code.
Proof.
  intros hs s caller inst funcaddr memaddr m p t Hcall Hfunc Haddr Hmem Hbound.
  destruct (split_even_correct hs s inst memaddr m p t Haddr Hmem Hbound)
    as [s' [m' [Hrun Hpost]]].
  exists s', m'; split; [| exact Hpost].
  eapply run_step.
  - apply (step_context
      (vs := [VAL_num (VAL_int32 p); VAL_num (VAL_int64 t)])
      (es := [AI_basic (BI_call 67%N)]) (es' := [AI_invoke funcaddr])
      (tail := [])).
    apply r_call; exact Hcall.
  - eapply invoke_correct; eauto.
Qed.

(* The final theorem combines decoding with the complete function guarantee. *)
Theorem packaged_correct_function :
  exists code, correct_selected_code = Some code /\ function_guarantee code.
Proof.
  exists correct_code; split.
  - apply correct_decodes.
  - apply correct_function_guarantee.
Qed.

Print Assumptions split_even_correct.
Print Assumptions packaged_correct_function.

End Execution.

Arguments step_context {hfc bum ho hs s f es hs' s' f' es' vs tail} _.
