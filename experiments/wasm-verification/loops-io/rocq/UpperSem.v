(* A small structured big-step judgement [exec] for the WebAssembly fragment
   used by [_start], proved sound with respect to WasmCert's own small-step
   [reduce_trans].  Nothing here is trusted: [exec_sound] derives every
   [exec] fact from WasmCert's reduction rules (r_label, rs_br, r_block,
   r_loop, r_invoke_host_success, ...).  The host is generic here. *)
From Stdlib Require Import List ZArith NArith Lia Relations.Relation_Operators.
From mathcomp Require Import ssrbool seq.
From Wasm Require Import opsem.
Import ListNotations.

Section Sem.
Context {hfc : host_function_class} {bum : BlockUpdateMemory}
        {ho : @host hfc bum}.

Notation vi x := (VAL_num (VAL_int32 x)).
Definition T4 : function_type :=
  Tf [T_num T_i32; T_num T_i32; T_num T_i32; T_num T_i32] [T_num T_i32].

Definition cfg := (host_state * store_record * frame * list administrative_instruction)%type.

Lemma step1 hs s f es hs' s' f' es' :
  reduce hs s f es hs' s' f' es' ->
  reduce_trans (hs, s, f, es) (hs', s', f', es').
Proof. intros H. apply rt_step. exact H. Qed.

Lemma run_step hs s f es hs' s' f' es' finish :
  reduce hs s f es hs' s' f' es' ->
  reduce_trans (hs', s', f', es') finish ->
  reduce_trans (hs, s, f, es) finish.
Proof.
  intros H1 H2. eapply rt_trans; [apply step1; exact H1 | exact H2].
Qed.

(* Lifting a reduction sequence through any context that lifts single steps. *)
Lemma rt_lift (F : list administrative_instruction -> list administrative_instruction) :
  (forall hs s f es hs' s' f' es',
      reduce hs s f es hs' s' f' es' -> reduce hs s f (F es) hs' s' f' (F es')) ->
  forall hs s f es hs' s' f' es',
    reduce_trans (hs, s, f, es) (hs', s', f', es') ->
    reduce_trans (hs, s, f, F es) (hs', s', f', F es').
Proof.
  intros HF hs s f es hs' s' f' es' H.
  remember (hs, s, f, es) as c eqn:Ec.
  remember (hs', s', f', es') as c' eqn:Ec'.
  revert hs s f es hs' s' f' es' Ec Ec'.
  induction H as [x y Hxy | x | x y z Hxy IH1 Hyz IH2];
    intros hs s f es hs' s' f' es' Ec Ec'; subst.
  - apply step1. apply HF. exact Hxy.
  - inversion Ec'; subst. apply rt_refl.
  - destruct y as [[[hy sy] fy] ey].
    eapply rt_trans; [eapply IH1 | eapply IH2]; reflexivity.
Qed.

Definition ctx (vs : list value) (tail : list administrative_instruction) es :=
  v_to_e_list vs ++ es ++ tail.

Lemma step_ctx vs tail hs s f es hs' s' f' es' :
  reduce hs s f es hs' s' f' es' ->
  reduce hs s f (ctx vs tail es) hs' s' f' (ctx vs tail es').
Proof.
  intros H. eapply r_label with (lh := LH_base vs tail); eauto.
Qed.

Lemma rt_ctx vs tail hs s f es hs' s' f' es' :
  reduce_trans (hs, s, f, es) (hs', s', f', es') ->
  reduce_trans (hs, s, f, ctx vs tail es) (hs', s', f', ctx vs tail es').
Proof. apply rt_lift. intros; apply step_ctx; auto. Qed.

Definition lctx vs n cont tail es :=
  v_to_e_list vs ++ [AI_label n cont es] ++ tail.

Lemma rt_lctx vs n cont tail hs s f es hs' s' f' es' :
  reduce_trans (hs, s, f, es) (hs', s', f', es') ->
  reduce_trans (hs, s, f, lctx vs n cont tail es) (hs', s', f', lctx vs n cont tail es').
Proof.
  apply rt_lift. intros.
  eapply r_label with (lh := LH_rec vs n cont (LH_base [] []) tail); eauto;
    simpl; rewrite List.app_nil_r; reflexivity.
Qed.

Lemma rt_frame n f0 hs s f es hs' s' f' es' :
  reduce_trans (hs, s, f, es) (hs', s', f', es') ->
  reduce_trans (hs, s, f0, [AI_frame n f es]) (hs', s', f0, [AI_frame n f' es']).
Proof.
  intros H.
  remember (hs, s, f, es) as c eqn:Ec.
  remember (hs', s', f', es') as c' eqn:Ec'.
  revert hs s f es hs' s' f' es' Ec Ec'.
  induction H as [x y Hxy | x | x y z Hxy IH1 Hyz IH2];
    intros hs s f es hs' s' f' es' Ec Ec'; subst.
  - apply step1. apply r_frame. exact Hxy.
  - inversion Ec'; subst. apply rt_refl.
  - destruct y as [[[hy sy] fy] ey].
    eapply rt_trans; [eapply IH1 | eapply IH2]; reflexivity.
Qed.

Lemma v_to_e_list_app a b : v_to_e_list (a ++ b) = v_to_e_list a ++ v_to_e_list b.
Proof. unfold v_to_e_list. apply List.map_app. Qed.

Lemma rev_cons_v (v : value) st :
  v_to_e_list (List.rev (v :: st)) = v_to_e_list (List.rev st) ++ [v_to_e v].
Proof. simpl. rewrite v_to_e_list_app. reflexivity. Qed.

(* ------------------------------------------------------------------ *)
(* Single straight-line instructions.  The stack is top-first.         *)

Definition set_local (f : frame) (j : N) (v : value) : frame :=
  Build_frame (set_nth v (f_locs f) (N.to_nat j) v) (f_inst f).

Inductive sstep : store_record -> frame -> list value -> basic_instruction ->
                  store_record -> frame -> list value -> Prop :=
| ss_const s f st v :
    sstep s f st (BI_const_num v) s f (VAL_num v :: st)
| ss_local_get s f st j v :
    lookup_N (f_locs f) j = Some v ->
    sstep s f st (BI_local_get j) s f (v :: st)
| ss_local_set s f st j v :
    N.to_nat j < length (f_locs f) ->
    sstep s f (v :: st) (BI_local_set j) s (set_local f j v) st
| ss_local_tee s f st j v :
    N.to_nat j < length (f_locs f) ->
    sstep s f (v :: st) (BI_local_tee j) s (set_local f j v) (v :: st)
| ss_global_get s f st j v :
    sglob_val s (f_inst f) j = Some v ->
    sstep s f st (BI_global_get j) s f (v :: st)
| ss_global_set s s' f st j v :
    supdate_glob s (f_inst f) j v = Some s' ->
    sstep s f (v :: st) (BI_global_set j) s' f st
| ss_binop s f st op x y v :
    binop_typecheck (VAL_int32 x) (VAL_int32 y) T_i32 op = true ->
    app_binop op (VAL_int32 x) (VAL_int32 y) = Some v ->
    sstep s f (vi y :: vi x :: st) (BI_binop T_i32 op) s f (VAL_num v :: st)
| ss_relop s f st op x y :
    relop_typecheck (VAL_int32 x) (VAL_int32 y) T_i32 op = true ->
    sstep s f (vi y :: vi x :: st) (BI_relop T_i32 op) s f
      (vi (wasm_bool (app_relop op (VAL_int32 x) (VAL_int32 y))) :: st)
| ss_testop s f st op x :
    sstep s f (vi x :: st) (BI_testop T_i32 op) s f
      (vi (wasm_bool (app_testop_i i32m op x)) :: st)
| ss_select s f st c v1 v2 ot :
    sstep s f (vi c :: v2 :: v1 :: st) (BI_select ot) s f
      ((if Wasm_int.Int32.eq c Wasm_int.Int32.zero then v2 else v1) :: st)
| ss_load s f st k marg m bs :
    smem s (f_inst f) = Some m ->
    load m (Wasm_int.N_of_uint i32m k) (memarg_offset marg) (tnum_length T_i32) = Some bs ->
    sstep s f (vi k :: st) (BI_load T_i32 None marg) s f
      (VAL_num (wasm_deserialise bs T_i32) :: st)
| ss_load8u s f st k marg m bs :
    smem s (f_inst f) = Some m ->
    load_packed SX_U m (Wasm_int.N_of_uint i32m k) (memarg_offset marg) 1 4 = Some bs ->
    sstep s f (vi k :: st) (BI_load T_i32 (Some (Tp_i8, SX_U)) marg) s f
      (VAL_num (wasm_deserialise bs T_i32) :: st)
| ss_store s s' f st k v marg :
    smem_store s (f_inst f) (Wasm_int.N_of_uint i32m k) (memarg_offset marg)
      (VAL_int32 v) T_i32 = Some s' ->
    sstep s f (vi v :: vi k :: st) (BI_store T_i32 None marg) s' f st
| ss_store8 s s' f st k v marg :
    smem_store_packed s (f_inst f) (Wasm_int.N_of_uint i32m k) (memarg_offset marg)
      (VAL_int32 v) Tp_i8 = Some s' ->
    sstep s f (vi v :: vi k :: st) (BI_store T_i32 (Some Tp_i8) marg) s' f st.

Ltac lsimpl := simpl; rewrite ?v_to_e_list_app; simpl;
  rewrite <- ?List.app_assoc; simpl; reflexivity.

Lemma lt_ltn (a b : nat) : (a < b)%nat -> is_true (ssrnat.leq (S a) b).
Proof. intros H. destruct (@ssrnat.ltP a b); [reflexivity | contradiction]. Qed.

Lemma base_lift hs s f st0 args i s' f' res tail :
  reduce hs s f (v_to_e_list args ++ [AI_basic i]) hs s' f' (v_to_e_list res) ->
  reduce_trans
    (hs, s, f, v_to_e_list (List.rev (List.rev args ++ st0)) ++ AI_basic i :: tail)
    (hs, s', f', v_to_e_list (List.rev (List.rev res ++ st0)) ++ tail).
Proof.
  intros H. apply step1.
  rewrite !List.rev_app_distr, !List.rev_involutive, !v_to_e_list_app.
  eapply r_label with (lh := LH_base (List.rev st0) tail); [exact H | |];
    simpl; rewrite <- !List.app_assoc; reflexivity.
Qed.

Lemma sstep_sound hs s f st i s' f' st' tail :
  sstep s f st i s' f' st' ->
  reduce_trans (hs, s, f, v_to_e_list (List.rev st) ++ AI_basic i :: tail)
               (hs, s', f', v_to_e_list (List.rev st') ++ tail).
Proof.
  intros H; destruct H.
  - rewrite rev_cons_v, <- List.app_assoc. apply rt_refl.
  - apply (base_lift hs s f st [] _ s f [v]). apply r_local_get; auto.
  - apply (base_lift hs s f st [v] _ s _ []).
    apply r_local_set with (vd := v); auto. apply lt_ltn; auto.
  - rewrite rev_cons_v, <- List.app_assoc. simpl.
    eapply rt_trans with (y := (hs, s, f, v_to_e_list (List.rev st) ++
                                 [v_to_e v; v_to_e v; AI_basic (BI_local_set j)] ++ tail)).
    + apply step1.
      eapply r_label with (lh := LH_base (List.rev st) tail)
        (es := [v_to_e v; AI_basic (BI_local_tee j)]); [| reflexivity | reflexivity].
      apply r_simple. apply rs_local_tee.
    + apply step1.
      eapply r_label with (lh := LH_base (List.rev st ++ [v]) tail)
        (es := [v_to_e v; AI_basic (BI_local_set j)]) (es' := []).
      * apply r_local_set with (vd := v); auto. apply lt_ltn; auto.
      * lsimpl.
      * lsimpl.
  - apply (base_lift hs s f st [] _ s f [v]). apply r_global_get; auto.
  - apply (base_lift hs s f st [v] _ s' f []). apply r_global_set; auto.
  - apply (base_lift hs s f st [vi x; vi y] _ s f [VAL_num v]).
    apply r_simple. apply rs_binop_success; auto.
  - apply (base_lift hs s f st [vi x; vi y] _ s f [vi _]).
    apply r_simple. apply rs_relop; auto.
  - apply (base_lift hs s f st [vi x] _ s f [vi _]).
    apply r_simple. apply rs_testop_i32.
  - apply (base_lift hs s f st [v1; v2; vi c] _ s f [_]).
    apply r_simple.
    destruct (Wasm_int.Int32.eq c Wasm_int.Int32.zero) eqn:E.
    + apply rs_select_false.
      pose proof (Wasm_int.Int32.eq_spec c Wasm_int.Int32.zero) as Hs.
      rewrite E in Hs. exact Hs.
    + apply rs_select_true.
      pose proof (Wasm_int.Int32.eq_spec c Wasm_int.Int32.zero) as Hs.
      rewrite E in Hs. exact Hs.
  - apply (base_lift hs s f st [vi k] _ s f [_]).
    eapply r_load_success; eauto.
  - apply (base_lift hs s f st [vi k] _ s f [_]).
    eapply r_load_packed_success; eauto.
  - apply (base_lift hs s f st [vi k; vi v] _ s' f []).
    apply r_store_success; auto.
  - apply (base_lift hs s f st [vi k; vi v] _ s' f []).
    eapply r_store_packed_success; eauto.
Qed.

(* ------------------------------------------------------------------ *)
(* Structured execution.  [Brk j] is a pending [br] to the j-th enclosing
   label of the instruction sequence; all labels here have arity 0.     *)

Inductive outcome := Norm (st : list value) | Brk (j : nat).

Definition BTnone := BT_valtype None.

Inductive exec : host_state -> store_record -> frame -> list value ->
                 list basic_instruction ->
                 host_state -> store_record -> frame -> outcome -> Prop :=
| ex_nil hs s f st : exec hs s f st [] hs s f (Norm st)
| ex_step hs s f st i rest s1 f1 st1 hs2 s2 f2 o :
    sstep s f st i s1 f1 st1 ->
    exec hs s1 f1 st1 rest hs2 s2 f2 o ->
    exec hs s f st (i :: rest) hs2 s2 f2 o
| ex_br hs s f st j rest :
    exec hs s f st (BI_br j :: rest) hs s f (Brk (N.to_nat j))
| ex_br_if_true hs s f st c j rest :
    c <> Wasm_int.Int32.zero ->
    exec hs s f (vi c :: st) (BI_br_if j :: rest) hs s f (Brk (N.to_nat j))
| ex_br_if_false hs s f st c j rest hs2 s2 f2 o :
    c = Wasm_int.Int32.zero ->
    exec hs s f st rest hs2 s2 f2 o ->
    exec hs s f (vi c :: st) (BI_br_if j :: rest) hs2 s2 f2 o
| ex_block_norm hs s f st body rest hs1 s1 f1 hs2 s2 f2 o :
    exec hs s f [] body hs1 s1 f1 (Norm []) ->
    exec hs1 s1 f1 st rest hs2 s2 f2 o ->
    exec hs s f st (BI_block BTnone body :: rest) hs2 s2 f2 o
| ex_block_br0 hs s f st body rest hs1 s1 f1 hs2 s2 f2 o :
    exec hs s f [] body hs1 s1 f1 (Brk 0) ->
    exec hs1 s1 f1 st rest hs2 s2 f2 o ->
    exec hs s f st (BI_block BTnone body :: rest) hs2 s2 f2 o
| ex_block_brS hs s f st body rest hs1 s1 f1 j :
    exec hs s f [] body hs1 s1 f1 (Brk (S j)) ->
    exec hs s f st (BI_block BTnone body :: rest) hs1 s1 f1 (Brk j)
| ex_loop_norm hs s f st body rest hs1 s1 f1 hs2 s2 f2 o :
    exec hs s f [] body hs1 s1 f1 (Norm []) ->
    exec hs1 s1 f1 st rest hs2 s2 f2 o ->
    exec hs s f st (BI_loop BTnone body :: rest) hs2 s2 f2 o
| ex_loop_br0 hs s f st body rest hs1 s1 f1 hs2 s2 f2 o :
    exec hs s f [] body hs1 s1 f1 (Brk 0) ->
    exec hs1 s1 f1 st (BI_loop BTnone body :: rest) hs2 s2 f2 o ->
    exec hs s f st (BI_loop BTnone body :: rest) hs2 s2 f2 o
| ex_loop_brS hs s f st body rest hs1 s1 f1 j :
    exec hs s f [] body hs1 s1 f1 (Brk (S j)) ->
    exec hs s f st (BI_loop BTnone body :: rest) hs1 s1 f1 (Brk j)
| ex_call_host hs s f st i a h a1 a2 a3 a4 hs1 s1 r rest hs2 s2 f2 o :
    lookup_N (inst_funcs (f_inst f)) i = Some a ->
    lookup_N (s_funcs s) a = Some (FC_func_host T4 h) ->
    host_application hs s T4 h [a1; a2; a3; a4] hs1 (Some (s1, result_values [r])) ->
    exec hs1 s1 f (r :: st) rest hs2 s2 f2 o ->
    exec hs s f (a4 :: a3 :: a2 :: a1 :: st) (BI_call i :: rest) hs2 s2 f2 o.

Definition post (c : cfg) hs' s' f' (o : outcome) tail : Prop :=
  match o with
  | Norm st' => reduce_trans c (hs', s', f', v_to_e_list (List.rev st') ++ tail)
  | Brk j => exists i (lh : lholed i),
      reduce_trans c (hs', s', f', lfill lh [AI_basic (BI_br (N.of_nat (i + j)))])
  end.

Lemma post_pre c c' hs' s' f' o tail :
  reduce_trans c c' -> post c' hs' s' f' o tail -> post c hs' s' f' o tail.
Proof.
  intros H1 H2; destruct o as [st'|j]; simpl in *.
  - eapply rt_trans; eauto.
  - destruct H2 as [i [lh H2]]. exists i, lh. eapply rt_trans; eauto.
Qed.

Lemma block_entry hs s f st body T :
  reduce hs s f (v_to_e_list (List.rev st) ++ AI_basic (BI_block BTnone body) :: T)
    hs s f (lctx (List.rev st) 0 [] T (v_to_e_list (List.rev []) ++ to_e_list body ++ [])).
Proof.
  eapply r_label with (lh := LH_base (List.rev st) T)
    (es := [AI_basic (BI_block BTnone body)]); [| reflexivity |].
  - eapply r_block with (vs := []) (t1s := []) (t2s := []); reflexivity.
  - unfold lctx. simpl. rewrite List.app_nil_r. reflexivity.
Qed.

Lemma loop_entry hs s f st body T :
  reduce hs s f (v_to_e_list (List.rev st) ++ AI_basic (BI_loop BTnone body) :: T)
    hs s f (lctx (List.rev st) 0 [AI_basic (BI_loop BTnone body)] T
              (v_to_e_list (List.rev []) ++ to_e_list body ++ [])).
Proof.
  eapply r_label with (lh := LH_base (List.rev st) T)
    (es := [AI_basic (BI_loop BTnone body)]); [| reflexivity |].
  - eapply r_loop with (vs := []) (t1s := []) (t2s := []); reflexivity.
  - unfold lctx. simpl. rewrite List.app_nil_r. reflexivity.
Qed.

Lemma label_exit_norm hs s f vs cont T :
  reduce hs s f (lctx vs 0 cont T (v_to_e_list (List.rev []) ++ []))
    hs s f (v_to_e_list vs ++ T).
Proof.
  eapply r_label with (lh := LH_base vs T) (es := [AI_label 0 cont []]) (es' := []).
  - apply r_simple. apply rs_label_const. reflexivity.
  - reflexivity.
  - reflexivity.
Qed.

Lemma label_exit_br hs s f vs cont T i (lh : lholed i) :
  reduce hs s f (lctx vs 0 cont T (lfill lh [AI_basic (BI_br (N.of_nat (i + 0)))]))
    hs s f (v_to_e_list vs ++ cont ++ T).
Proof.
  eapply r_label with (lh := LH_base vs T)
    (es := [AI_label 0 cont (lfill lh [AI_basic (BI_br (N.of_nat (i + 0)))])])
    (es' := cont).
  - apply r_simple. eapply rs_br with (vs := []) (lh := lh); try reflexivity.
    rewrite Nat.add_0_r. reflexivity.
  - reflexivity.
  - reflexivity.
Qed.

Lemma label_exit_br0 hs s f vs T i (lh : lholed i) :
  reduce hs s f (lctx vs 0 [] T (lfill lh [AI_basic (BI_br (N.of_nat (i + 0)))]))
    hs s f (v_to_e_list vs ++ T).
Proof. exact (label_exit_br hs s f vs [] T i lh). Qed.

Lemma label_exit_brL hs s f vs T body i (lh : lholed i) :
  reduce hs s f (lctx vs 0 [AI_basic (BI_loop BTnone body)] T
                   (lfill lh [AI_basic (BI_br (N.of_nat (i + 0)))]))
    hs s f (v_to_e_list vs ++ to_e_list (BI_loop BTnone body :: []) ++ T).
Proof. exact (label_exit_br hs s f vs _ T i lh). Qed.

Theorem exec_sound hs s f st bes hs' s' f' o :
  exec hs s f st bes hs' s' f' o ->
  forall tail,
    post (hs, s, f, v_to_e_list (List.rev st) ++ to_e_list bes ++ tail) hs' s' f' o tail.
Proof.
  induction 1; intros tail; simpl to_e_list; simpl app.
  - apply rt_refl.
  - eapply post_pre; [apply sstep_sound; eauto | apply IHexec].
  - simpl. exists 0%nat, (LH_base (List.rev st) (to_e_list rest ++ tail)).
    simpl. rewrite N2Nat.id. apply rt_refl.
  - simpl. exists 0%nat, (LH_base (List.rev st) (to_e_list rest ++ tail)).
    simpl. rewrite N2Nat.id.
    simpl List.rev; rewrite ?v_to_e_list_app, <- ?List.app_assoc; simpl.
    apply step1.
    eapply r_label with (lh := LH_base (List.rev st) (to_e_list rest ++ tail))
      (es := [v_to_e (vi c); AI_basic (BI_br_if j)]) (es' := [AI_basic (BI_br j)]);
      [| reflexivity | reflexivity].
    apply r_simple. apply rs_br_if_true. exact H.
  - eapply post_pre; [| apply IHexec].
    simpl List.rev; rewrite ?v_to_e_list_app, <- ?List.app_assoc; simpl.
    apply step1.
    eapply r_label with (lh := LH_base (List.rev st) (to_e_list rest ++ tail))
      (es := [v_to_e (vi c); AI_basic (BI_br_if j)]) (es' := []);
      [| reflexivity | reflexivity].
    apply r_simple. apply rs_br_if_false. exact H.
  - (* block, normal *)
    eapply post_pre; [| apply IHexec2].
    eapply run_step; [apply block_entry |].
    eapply rt_trans; [apply rt_lctx; apply (IHexec1 []) |].
    apply step1. apply label_exit_norm.
  - (* block, br 0 *)
    eapply post_pre; [| apply IHexec2].
    eapply run_step; [apply block_entry |].
    destruct (IHexec1 []) as [i [lh Hb]].
    eapply rt_trans; [apply rt_lctx; exact Hb |].
    apply step1. apply label_exit_br0.
  - (* block, outer break *)
    destruct (IHexec []) as [i [lh Hb]].
    exists (S i), (LH_rec (List.rev st) 0 [] lh (to_e_list rest ++ tail)).
    eapply run_step; [apply block_entry |].
    rewrite <- plus_n_Sm in Hb.
    exact (rt_lctx _ _ _ _ _ _ _ _ _ _ _ _ Hb).
  - (* loop, normal *)
    eapply post_pre; [| apply IHexec2].
    eapply run_step; [apply loop_entry |].
    eapply rt_trans; [apply rt_lctx; apply (IHexec1 []) |].
    apply step1. apply label_exit_norm.
  - (* loop, br 0: run the loop again *)
    eapply post_pre; [| apply IHexec2].
    eapply run_step; [apply loop_entry |].
    destruct (IHexec1 []) as [i [lh Hb]].
    eapply rt_trans; [apply rt_lctx; exact Hb |].
    apply step1. apply label_exit_brL.
  - (* loop, outer break *)
    destruct (IHexec []) as [i [lh Hb]].
    exists (S i), (LH_rec (List.rev st) 0 [AI_basic (BI_loop BTnone body)] lh
                    (to_e_list rest ++ tail)).
    eapply run_step; [apply loop_entry |].
    rewrite <- plus_n_Sm in Hb.
    exact (rt_lctx _ _ _ _ _ _ _ _ _ _ _ _ Hb).
  - (* host call *)
    eapply post_pre; [| apply IHexec].
    assert (Hr : List.rev (a4 :: a3 :: a2 :: a1 :: st) = List.rev st ++ [a1; a2; a3; a4])
      by (simpl; rewrite <- !List.app_assoc; reflexivity).
    rewrite Hr, v_to_e_list_app, <- List.app_assoc.
    eapply run_step.
    { eapply r_label with (lh := LH_base (List.rev st ++ [a1; a2; a3; a4])
                                   (to_e_list rest ++ tail))
        (es := [AI_basic (BI_call i)]) (es' := [AI_invoke a]).
      - apply r_call. exact H.
      - simpl. rewrite v_to_e_list_app, <- List.app_assoc. reflexivity.
      - reflexivity. }
    apply step1.
    rewrite rev_cons_v, <- List.app_assoc.
    eapply r_label with (lh := LH_base (List.rev st) (to_e_list rest ++ tail))
      (es := v_to_e_list [a1; a2; a3; a4] ++ [AI_invoke a])
      (es' := [v_to_e r]).
    + exact (@r_invoke_host_success _ _ _ a _ h _ _ _ [a1; a2; a3; a4] _ _ s s1
               (result_values [r]) f hs hs1 H0 eq_refl eq_refl eq_refl eq_refl eq_refl H1).
    + lsimpl.
    + lsimpl.
Qed.

Corollary exec_sound_norm hs s f bes hs' s' f' :
  exec hs s f [] bes hs' s' f' (Norm []) ->
  reduce_trans (hs, s, f, to_e_list bes) (hs', s', f', []).
Proof.
  intros H. pose proof (exec_sound _ _ _ _ _ _ _ _ _ H []) as P.
  simpl in P. rewrite List.app_nil_r in P. exact P.
Qed.

End Sem.
