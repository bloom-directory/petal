From Stdlib Require Import List ZArith NArith Lia Relations.Relation_Operators.
From mathcomp Require Import seq.
From Wasm Require Import opsem.
Import ListNotations.

(* Reusable reduction library: lifting [reduce_trans] through evaluation
   contexts, labels and frames, and continuation-style lemmas for executing
   straight-line code on a stack [st] (top of stack first). *)

Section Lib.
Context {hfc : host_function_class} {bum : BlockUpdateMemory}
        {ho : @host hfc bum}.

Definition V (st : list value) := v_to_e_list (List.rev st).

Lemma V_nil : V [] = [].
Proof. reflexivity. Qed.

Lemma V_cons v st X : V (v :: st) ++ X = V st ++ v_to_e v :: X.
Proof.
  unfold V, v_to_e_list. simpl List.rev.
  rewrite List.map_app, <- List.app_assoc. reflexivity.
Qed.

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

Lemma run_trans c1 c2 c3 :
  reduce_trans c1 c2 -> reduce_trans c2 c3 -> reduce_trans c1 c3.
Proof. intros; eapply rt_trans; eauto. Qed.

(* Lifting through an arbitrary label context. *)
Definition lift_lh {k} (lh : lholed k)
  (c : host_state * store_record * frame * list administrative_instruction) :=
  let '(hs, s, f, es) := c in (hs, s, f, lfill lh es).

Lemma rt_lfill_gen {k} (lh : lholed k) c c' :
  reduce_trans c c' -> reduce_trans (lift_lh lh c) (lift_lh lh c').
Proof.
  intros Hrun; induction Hrun.
  - apply rt_step.
    destruct x as [[[hs s] f] es], y as [[[hs' s'] f'] es']; simpl in *.
    eapply r_label; [exact H | reflexivity | reflexivity].
  - apply rt_refl.
  - eapply rt_trans; eauto.
Qed.

Lemma rt_lfill {k} (lh : lholed k) hs s f es hs' s' f' es' :
  reduce_trans (hs, s, f, es) (hs', s', f', es') ->
  reduce_trans (hs, s, f, lfill lh es) (hs', s', f', lfill lh es').
Proof. intros H; exact (rt_lfill_gen lh _ _ H). Qed.

Lemma rt_ctx pre tail hs s f es hs' s' f' es' :
  reduce_trans (hs, s, f, es) (hs', s', f', es') ->
  reduce_trans (hs, s, f, v_to_e_list pre ++ es ++ tail)
               (hs', s', f', v_to_e_list pre ++ es' ++ tail).
Proof. intros H; exact (rt_lfill (LH_base pre tail) _ _ _ _ _ _ _ _ H). Qed.

Lemma rt_label n c hs s f es hs' s' f' es' :
  reduce_trans (hs, s, f, es) (hs', s', f', es') ->
  reduce_trans (hs, s, f, [AI_label n c es]) (hs', s', f', [AI_label n c es']).
Proof.
  intros H.
  pose proof (rt_lfill (LH_rec [] n c (LH_base [] []) []) _ _ _ _ _ _ _ _ H) as H'.
  simpl in H'. rewrite !List.app_nil_r in H'. exact H'.
Qed.

Definition lift_frame n (f0 : frame)
  (c : host_state * store_record * frame * list administrative_instruction) :=
  let '(hs, s, f, es) := c in (hs, s, f0, [AI_frame n f es]).

Lemma rt_frame_gen n f0 c c' :
  reduce_trans c c' -> reduce_trans (lift_frame n f0 c) (lift_frame n f0 c').
Proof.
  intros Hrun; induction Hrun.
  - apply rt_step.
    destruct x as [[[hs s] f] es], y as [[[hs' s'] f'] es']; simpl in *.
    apply r_frame; exact H.
  - apply rt_refl.
  - eapply rt_trans; eauto.
Qed.

Lemma rt_frame n f0 hs s f es hs' s' f' es' :
  reduce_trans (hs, s, f, es) (hs', s', f', es') ->
  reduce_trans (hs, s, f0, [AI_frame n f es]) (hs', s', f0, [AI_frame n f' es']).
Proof. intros H; exact (rt_frame_gen n f0 _ _ H). Qed.

(* ---------- continuation-style straight-line steps ---------- *)

Lemma step_ctx_cps hs s f f' pre es es' tail fin :
  reduce hs s f es hs s f' es' ->
  reduce_trans (hs, s, f', v_to_e_list pre ++ es' ++ tail) fin ->
  reduce_trans (hs, s, f, v_to_e_list pre ++ es ++ tail) fin.
Proof.
  intros Hstep Hrest. eapply run_step; [| exact Hrest].
  eapply r_label with (lh := LH_base pre tail); eauto.
Qed.

Lemma to_e_cons i is : to_e_list (i :: is) = AI_basic i :: to_e_list is.
Proof. reflexivity. Qed.

Lemma cps_get hs s f st j v is fin :
  lookup_N (f_locs f) j = Some v ->
  reduce_trans (hs, s, f, V (v :: st) ++ to_e_list is) fin ->
  reduce_trans (hs, s, f, V st ++ to_e_list (BI_local_get j :: is)) fin.
Proof.
  intros Hj H. rewrite V_cons in H. rewrite to_e_cons.
  apply (step_ctx_cps hs s f f (List.rev st) [AI_basic (BI_local_get j)]
           [v_to_e v] (to_e_list is) fin); [| exact H].
  apply r_local_get; exact Hj.
Qed.

Lemma cps_const hs s f st c is fin :
  reduce_trans (hs, s, f, V (VAL_num c :: st) ++ to_e_list is) fin ->
  reduce_trans (hs, s, f, V st ++ to_e_list (BI_const_num c :: is)) fin.
Proof. intros H. rewrite V_cons in H. exact H. Qed.

Lemma cps_binop hs s f st v1 v2 v t op is fin :
  binop_typecheck v1 v2 t op = true ->
  app_binop op v1 v2 = Some v ->
  reduce_trans (hs, s, f, V (VAL_num v :: st) ++ to_e_list is) fin ->
  reduce_trans (hs, s, f, V (VAL_num v2 :: VAL_num v1 :: st) ++
                          to_e_list (BI_binop t op :: is)) fin.
Proof.
  intros Ht Hv H. rewrite V_cons in H. rewrite !V_cons, to_e_cons.
  apply (step_ctx_cps hs s f f (List.rev st)
    [$VN v1; $VN v2; AI_basic (BI_binop t op)] [$VN v] (to_e_list is) fin);
    [| exact H].
  apply r_simple; eapply rs_binop_success; eauto.
Qed.

Lemma cps_relop hs s f st v1 v2 t op is fin :
  relop_typecheck v1 v2 t op = true ->
  reduce_trans (hs, s, f, V (VAL_num (VAL_int32 (wasm_bool (app_relop op v1 v2)))
                             :: st) ++ to_e_list is) fin ->
  reduce_trans (hs, s, f, V (VAL_num v2 :: VAL_num v1 :: st) ++
                          to_e_list (BI_relop t op :: is)) fin.
Proof.
  intros Ht H. rewrite V_cons in H. rewrite !V_cons, to_e_cons.
  apply (step_ctx_cps hs s f f (List.rev st)
    [$VN v1; $VN v2; AI_basic (BI_relop t op)]
    [v_to_e (VAL_num (VAL_int32 (wasm_bool (app_relop op v1 v2))))] (to_e_list is) fin);
    [| exact H].
  apply r_simple; eapply rs_relop; eauto.
Qed.

Lemma cps_eqz32 hs s f st c is fin :
  reduce_trans (hs, s, f, V (VAL_num (VAL_int32 (wasm_bool
                  (app_testop_i i32m TO_eqz c))) :: st) ++ to_e_list is) fin ->
  reduce_trans (hs, s, f, V (VAL_num (VAL_int32 c) :: st) ++
                          to_e_list (BI_testop T_i32 TO_eqz :: is)) fin.
Proof.
  intros H. rewrite V_cons in H. rewrite !V_cons, to_e_cons.
  apply (step_ctx_cps hs s f f (List.rev st)
    [$VN (VAL_int32 c); AI_basic (BI_testop T_i32 TO_eqz)]
    [v_to_e (VAL_num (VAL_int32 (wasm_bool (app_testop_i i32m TO_eqz c))))]
    (to_e_list is) fin);
    [| exact H].
  apply r_simple; apply rs_testop_i32.
Qed.

Lemma cps_tee hs s f st v j is fin :
  reduce_trans (hs, s, f, V (v :: v :: st) ++ to_e_list (BI_local_set j :: is)) fin ->
  reduce_trans (hs, s, f, V (v :: st) ++ to_e_list (BI_local_tee j :: is)) fin.
Proof.
  intros H. rewrite !V_cons, to_e_cons in H. rewrite V_cons, to_e_cons.
  apply (step_ctx_cps hs s f f (List.rev st)
    [v_to_e v; AI_basic (BI_local_tee j)]
    [v_to_e v; v_to_e v; AI_basic (BI_local_set j)] (to_e_list is) fin);
    [| exact H].
  apply r_simple; apply rs_local_tee.
Qed.

Lemma cps_set hs s locs inst st v j is fin :
  N.to_nat j < List.length locs ->
  reduce_trans (hs, s, Build_frame (set_nth (VAL_num (VAL_int32 Wasm_int.Int32.zero))
                                     locs (N.to_nat j) v) inst,
                V st ++ to_e_list is) fin ->
  reduce_trans (hs, s, Build_frame locs inst,
                V (v :: st) ++ to_e_list (BI_local_set j :: is)) fin.
Proof.
  intros Hj H. rewrite V_cons, to_e_cons.
  apply (step_ctx_cps hs s (Build_frame locs inst)
    (Build_frame (set_nth (VAL_num (VAL_int32 Wasm_int.Int32.zero))
                                     locs (N.to_nat j) v) inst) (List.rev st)
    [v_to_e v; AI_basic (BI_local_set j)] [] (to_e_list is) fin);
    [| exact H].
  eapply r_local_set; simpl; eauto.
  exact (ssrbool.introT ssrnat.ltP Hj).
Qed.

Lemma cps_br_if_true hs s f st c j is fin :
  c <> Wasm_int.int_zero i32m ->
  reduce_trans (hs, s, f, V st ++ to_e_list (BI_br j :: is)) fin ->
  reduce_trans (hs, s, f, V (VAL_num (VAL_int32 c) :: st) ++
                          to_e_list (BI_br_if j :: is)) fin.
Proof.
  intros Hc H. rewrite to_e_cons in H. rewrite V_cons, to_e_cons.
  apply (step_ctx_cps hs s f f (List.rev st)
    [$VN (VAL_int32 c); AI_basic (BI_br_if j)] [AI_basic (BI_br j)]
    (to_e_list is) fin); [| exact H].
  apply r_simple; apply rs_br_if_true; exact Hc.
Qed.

Lemma cps_br_if_false hs s f st c j is fin :
  c = Wasm_int.int_zero i32m ->
  reduce_trans (hs, s, f, V st ++ to_e_list is) fin ->
  reduce_trans (hs, s, f, V (VAL_num (VAL_int32 c) :: st) ++
                          to_e_list (BI_br_if j :: is)) fin.
Proof.
  intros Hc H. rewrite V_cons, to_e_cons.
  apply (step_ctx_cps hs s f f (List.rev st)
    [$VN (VAL_int32 c); AI_basic (BI_br_if j)] [] (to_e_list is) fin);
    [| exact H].
  apply r_simple; apply rs_br_if_false; exact Hc.
Qed.

(* Run a single instruction to completion, then continue. *)
Lemma cps_sub hs s f f' st i is fin :
  reduce_trans (hs, s, f, [AI_basic i]) (hs, s, f', []) ->
  reduce_trans (hs, s, f', V st ++ to_e_list is) fin ->
  reduce_trans (hs, s, f, V st ++ to_e_list (i :: is)) fin.
Proof.
  intros Hsub H. rewrite to_e_cons.
  eapply run_trans; [| exact H].
  exact (rt_ctx (List.rev st) (to_e_list is) _ _ _ _ _ _ _ _ Hsub).
Qed.

(* ---------- blocks, loops and branches (all labels here have arity 0) ---------- *)

Lemma block_enter hs s f body fin :
  reduce_trans (hs, s, f, [AI_label 0 [] (to_e_list body)]) fin ->
  reduce_trans (hs, s, f, [AI_basic (BI_block (BT_valtype None) body)]) fin.
Proof.
  intros H. eapply run_step; [| exact H].
  eapply r_block with (vs := []) (t1s := []) (t2s := []); reflexivity.
Qed.

Lemma loop_enter hs s f body fin :
  reduce_trans (hs, s, f, [AI_label 0 [AI_basic (BI_loop (BT_valtype None) body)]
                              (to_e_list body)]) fin ->
  reduce_trans (hs, s, f, [AI_basic (BI_loop (BT_valtype None) body)]) fin.
Proof.
  intros H. eapply run_step; [| exact H].
  eapply r_loop with (vs := []) (t1s := []) (t2s := []); reflexivity.
Qed.

(* Content finishes normally: the label disappears. *)
Lemma label_done hs s f f' n c es :
  reduce_trans (hs, s, f, es) (hs, s, f', []) ->
  reduce_trans (hs, s, f, [AI_label n c es]) (hs, s, f', []).
Proof.
  intros H. eapply run_trans; [apply rt_label; exact H |].
  apply rt_step, r_simple, rs_label_const; reflexivity.
Qed.

(* Content branches to this label: continue with its continuation. *)
Lemma label_br0 hs s f f' c es rest fin :
  reduce_trans (hs, s, f, es) (hs, s, f', AI_basic (BI_br 0%N) :: rest) ->
  reduce_trans (hs, s, f', c) fin ->
  reduce_trans (hs, s, f, [AI_label 0 c es]) fin.
Proof.
  intros H Hc. eapply run_trans; [apply rt_label; exact H |].
  eapply run_step; [| exact Hc].
  apply r_simple.
  eapply rs_br with (vs := []) (lh := LH_base [] rest); reflexivity.
Qed.

(* An inner block (entered from a stack [st]) branches out past the label of
   the enclosing block, which continues with its continuation [c]. *)
Lemma label_block_br1 hs s f f' c st body rest is fin :
  reduce_trans (hs, s, f, [AI_label 0 [] (to_e_list body)])
               (hs, s, f', [AI_label 0 [] (AI_basic (BI_br 1%N) :: rest)]) ->
  reduce_trans (hs, s, f', c) fin ->
  reduce_trans (hs, s, f, [AI_label 0 c (V st ++ to_e_list
                   (BI_block (BT_valtype None) body :: is))]) fin.
Proof.
  intros H Hc. rewrite to_e_cons.
  apply run_trans with (c2 := (hs, s, f', [AI_label 0 c (v_to_e_list (List.rev st) ++
      [AI_label 0 [] (AI_basic (BI_br 1%N) :: rest)] ++ to_e_list is)])).
  { apply rt_label.
    change (V st ++ AI_basic (BI_block (BT_valtype None) body) :: to_e_list is) with
      (v_to_e_list (List.rev st) ++ [AI_basic (BI_block (BT_valtype None) body)]
        ++ to_e_list is).
    apply (rt_ctx (List.rev st) (to_e_list is)).
    eapply run_step; [| exact H].
    eapply r_block with (vs := []) (t1s := []) (t2s := []); reflexivity. }
  eapply run_step; [| exact Hc].
  apply r_simple.
  eapply rs_br with (vs := []) (lh := LH_rec (List.rev st) 0 [] (LH_base [] rest)
                                  (to_e_list is)); reflexivity.
Qed.

(* One loop iteration that ends by branching back to the loop head. *)
Lemma loop_again hs s f f' body rest fin :
  reduce_trans (hs, s, f, to_e_list body) (hs, s, f', AI_basic (BI_br 0%N) :: rest) ->
  reduce_trans (hs, s, f', [AI_basic (BI_loop (BT_valtype None) body)]) fin ->
  reduce_trans (hs, s, f, [AI_basic (BI_loop (BT_valtype None) body)]) fin.
Proof.
  intros H Hc. apply loop_enter. eapply label_br0; eauto.
Qed.

(* The last loop iteration, falling off the end of the body. *)
Lemma loop_exit hs s f f' body :
  reduce_trans (hs, s, f, to_e_list body) (hs, s, f', []) ->
  reduce_trans (hs, s, f, [AI_basic (BI_loop (BT_valtype None) body)]) (hs, s, f', []).
Proof.
  intros H. apply loop_enter. apply label_done; exact H.
Qed.

End Lib.
