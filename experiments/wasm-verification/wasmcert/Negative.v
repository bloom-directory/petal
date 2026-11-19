From Stdlib Require Import List ZArith Lia Relations.Relation_Operators.
From Wasm Require Import opsem.
Require Import Artifact Arithmetic MemoryFacts Execution.
Import ListNotations.
Local Open Scope N_scope.

Section Negative.
Context {hfc : host_function_class} {bum : BlockUpdateMemory}
        {ho : @host hfc bum}.

Definition faulty_frame inst p t :=
  Build_frame [VAL_num (VAL_int32 p); VAL_num (VAL_int64 t)] inst.

Lemma faulty_body_execution hs s s1 s2 inst p t :
  smem_store s inst (Wasm_int.N_of_uint i32m p) 8
    (VAL_int64 (half t)) T_i64 = Some s1 ->
  smem_store s1 inst (Wasm_int.N_of_uint i32m p) 0
    (VAL_int64 (half t)) T_i64 = Some s2 ->
  reduce_trans (hs, s, faulty_frame inst p t, to_e_list faulty_body)
    (hs, s2, faulty_frame inst p (half t), []).
Proof.
  intros Hstore1 Hstore2.
  cbn [faulty_body to_e_list].
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
      (es := [$VN (VAL_int64 (half t)); AI_basic (BI_local_tee 1%N)])
      (es' := [$VN (VAL_int64 (half t)); $VN (VAL_int64 (half t));
               AI_basic (BI_local_set 1%N)])).
    apply r_simple.
    apply rs_local_tee with (v := VAL_num (VAL_int64 (half t))). }
  eapply run_step.
  {
    apply (step_context
      (vs := [VAL_num (VAL_int32 p); VAL_num (VAL_int64 (half t))])
      (es := [$VN (VAL_int64 (half t)); AI_basic (BI_local_set 1%N)])
      (es' := [])).
    apply r_local_set with (v := VAL_num (VAL_int64 (half t)))
      (vd := VAL_num (VAL_int64 I.zero))
      (f' := faulty_frame inst p (half t)); simpl; auto. }
  eapply run_step.
  { apply step_store with (prefix := []). exact Hstore1. }
  eapply run_step.
  { apply step_get with (prefix := []). reflexivity. }
  eapply run_step.
  { apply step_get with (prefix := [VAL_num (VAL_int32 p)]).
    reflexivity. }
  apply rt_step. apply step_store with (prefix := []). exact Hstore2.
Qed.

(* A semantic counterexample for the faulty artifact, not a hash rejection.
   For any valid output slot it returns the two values 0,0 at total=1. *)
Theorem faulty_violates_SPLIT_1 hs s inst addr m p :
  lookup_N (inst_mems inst) 0 = Some addr ->
  lookup_N (s_mems s) addr = Some m ->
  (Wasm_int.N_of_uint i32m p + 16 <= operations.mem_length m)%N ->
  exists code s' m',
    faulty_selected_code = Some code /\
    reduce_trans (hs, s, faulty_frame inst p (I.repr 1%Z),
                   to_e_list (modfunc_body code))
      (hs, s', faulty_frame inst p (half (I.repr 1%Z)), []) /\
    lookup_N (s_mems s') addr = Some m' /\
    bytes_at m' (Wasm_int.N_of_uint i32m p)
      (serialise_i64 (half (I.repr 1%Z))) /\
    bytes_at m' (Wasm_int.N_of_uint i32m p + 8)
      (serialise_i64 (half (I.repr 1%Z))) /\
    (I.unsigned (half (I.repr 1%Z)) + I.unsigned (half (I.repr 1%Z)) <> 1)%Z.
Proof.
  intros Haddr Hmem Hbound.
  destruct (memory_store_effect s inst addr m
    (Wasm_int.N_of_uint i32m p) 8 (half (I.repr 1%Z)) Haddr Hmem ltac:(lia))
    as [s1 [m1 [Hs1 [Hm1 [Hlen1 [Hb1 Hframe1]]]]]].
  destruct (memory_store_effect s1 inst addr m1
    (Wasm_int.N_of_uint i32m p) 0 (half (I.repr 1%Z)) Haddr Hm1 ltac:(lia))
    as [s2 [m2 [Hs2 [Hm2 [Hlen2 [Hb2 Hframe2]]]]]].
  exists faulty_code, s2, m2.
  split; [apply faulty_decodes |].
  split; [eapply faulty_body_execution; eauto |].
  split; [exact Hm2 |].
  split.
  - replace (Wasm_int.N_of_uint i32m p) with
      (Wasm_int.N_of_uint i32m p + 0)%N by lia; exact Hb2.
  - split.
    + eapply bytes_at_preserved; [exact Hb1 | exact Hframe2 |].
      right; lia.
    + exact faulty_conservation_counterexample.
Qed.

Print Assumptions faulty_violates_SPLIT_1.
End Negative.
