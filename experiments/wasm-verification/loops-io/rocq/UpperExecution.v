(* End-to-end behaviour of the exported [_start] function (function index 2)
   of the upper-casing WASI command, under the concrete WASI host of
   WasiHost.v.  The proof builds an [exec]/[wp] derivation (UpperSem/UpperWp),
   which [exec_sound] turns into WasmCert [reduce_trans]. *)
From Stdlib Require Import List ZArith NArith Lia Relations.Relation_Operators Strings.String.
From Wasm Require Import opsem binary_format_parser.
Require Import UpperArtifact UpperSem UpperWp WasiMem WasiHost.
Import ListNotations.

Notation vi x := (VAL_num (VAL_int32 x)).
Notation I z := (Wasm_int.Int32.repr z).

Arguments Wasm_int.Int32.repr : simpl never.
Arguments Wasm_int.Int32.add : simpl never.
Arguments Wasm_int.Int32.sub : simpl never.
Arguments Wasm_int.Int32.and : simpl never.
Arguments Wasm_int.Int32.xor : simpl never.
Arguments Wasm_int.Int32.ltu : simpl never.
Arguments Wasm_int.Int32.eq : simpl never.
Arguments Wasm_int.Int32.unsigned : simpl never.
Arguments Wasm_int.Int32.zero : simpl never.
Arguments Wasm_int.Int32.one : simpl never.

(* ------------------------------------------------------------------ *)
(* i32 arithmetic                                                       *)

Definition rng (z : Z) : Prop := (0 <= z <= 4294967295)%Z.

Lemma uns_I z : rng z -> Wasm_int.Int32.unsigned (I z) = z.
Proof. intros H. apply Wasm_int.Int32.unsigned_repr. unfold rng in H. exact H. Qed.

Lemma add_I a b : Wasm_int.Int32.add (I a) (I b) = I (a + b).
Proof.
  unfold Wasm_int.Int32.add. apply Wasm_int.Int32.eqm_samerepr.
  apply Wasm_int.Int32.eqm_add; apply Wasm_int.Int32.eqm_sym, Wasm_int.Int32.eqm_unsigned_repr.
Qed.

Lemma sub_I a b : Wasm_int.Int32.sub (I a) (I b) = I (a - b).
Proof.
  unfold Wasm_int.Int32.sub. apply Wasm_int.Int32.eqm_samerepr.
  apply Wasm_int.Int32.eqm_sub; apply Wasm_int.Int32.eqm_sym, Wasm_int.Int32.eqm_unsigned_repr.
Qed.

Lemma eq_I a b : rng a -> rng b -> Wasm_int.Int32.eq (I a) (I b) = Z.eqb a b.
Proof.
  intros Ha Hb. unfold Wasm_int.Int32.eq. rewrite !uns_I by assumption.
  destruct (Coqlib.zeq a b) as [E|E]; symmetry; [apply Z.eqb_eq | apply Z.eqb_neq]; exact E.
Qed.

Lemma ltu_I a b : rng a -> rng b -> Wasm_int.Int32.ltu (I a) (I b) = Z.ltb a b.
Proof.
  intros Ha Hb. unfold Wasm_int.Int32.ltu. rewrite !uns_I by assumption.
  destruct (Coqlib.zlt a b) as [E|E]; symmetry; [apply Z.ltb_lt | apply Z.ltb_ge]; lia.
Qed.

Lemma zero_I : Wasm_int.Int32.zero = I 0.
Proof. reflexivity. Qed.

Lemma bool_true_nz : wasm_bool true <> Wasm_int.Int32.zero.
Proof. exact Wasm_int.Int32.one_not_zero. Qed.

Lemma bool_false_z : wasm_bool false = Wasm_int.Int32.zero.
Proof. reflexivity. Qed.

Lemma sel_bool (b : bool) :
  Wasm_int.Int32.eq (wasm_bool b) Wasm_int.Int32.zero = negb b.
Proof.
  destruct b; simpl.
  - apply Wasm_int.Int32.eq_false. exact Wasm_int.Int32.one_not_zero.
  - apply Wasm_int.Int32.eq_true.
Qed.

Lemma u32_I z : rng z -> Wasm_int.N_of_uint i32m (I z) = Z.to_N z.
Proof. intros H. simpl. rewrite uns_I by exact H. reflexivity. Qed.

(* The selects/ands on the read length, checked for every possible length. *)
Definition and_ok (n : nat) : bool :=
  Wasm_int.Int32.eq (Wasm_int.Int32.and (I (Z.of_nat n)) (I 1)) (I (Z.of_nat (n mod 2))) &&
  Wasm_int.Int32.eq (Wasm_int.Int32.and (I (Z.of_nat n)) (I 510))
                    (I (Z.of_nat (n - n mod 2))).

Lemma and_ok_all : forallb and_ok (seq 0 257) = true.
Proof. vm_compute. reflexivity. Qed.

Lemma and_facts n : (n <= 256)%nat ->
  Wasm_int.Int32.and (I (Z.of_nat n)) (I 1) = I (Z.of_nat (n mod 2)) /\
  Wasm_int.Int32.and (I (Z.of_nat n)) (I 510) = I (Z.of_nat (n - n mod 2)).
Proof.
  intros Hn. pose proof and_ok_all as H. rewrite forallb_forall in H.
  specialize (H n). assert (Hin : In n (seq 0 257)) by (apply in_seq; lia).
  specialize (H Hin). unfold and_ok in H. apply andb_prop in H. destruct H as [H1 H2].
  split; apply Wasm_int.Int32.same_if_eq; assumption.
Qed.

(* ------------------------------------------------------------------ *)
(* The byte transformation computed by the upcase snippet.              *)

Definition upv (b : byte) : i32 :=
  let B := I (Memdata.decode_int (bytes_takefill #00 4 [b])) in
  Wasm_int.Int32.xor
    (if Wasm_int.Int32.eq
          (wasm_bool (Wasm_int.Int32.ltu
             (Wasm_int.Int32.and (Wasm_int.Int32.add B (I (-97))) (I 255)) (I 26)))
          Wasm_int.Int32.zero
     then I 0 else I 32) B.

Definition up_ok (z : Z) : bool :=
  let b := Integers.Byte.repr z in
  if list_eq_dec Integers.Byte.eq_dec
       (bytes_takefill #00 1 (serialise_i32 (upv b))) [ascii_upper b]
  then true else false.

Lemma up_ok_all : forallb up_ok (map Z.of_nat (seq 0 256)) = true.
Proof. vm_compute. reflexivity. Qed.

Lemma upv_spec b : bytes_takefill #00 1 (serialise_i32 (upv b)) = [ascii_upper b].
Proof.
  pose proof up_ok_all as H. rewrite forallb_forall in H.
  rewrite <- (Integers.Byte.repr_unsigned b).
  pose proof (Integers.Byte.unsigned_range b) as R.
  assert (Hin : In (Integers.Byte.unsigned b) (map Z.of_nat (seq 0 256))).
  { apply in_map_iff. exists (Z.to_nat (Integers.Byte.unsigned b)).
    split; [apply Z2Nat.id; lia|]. apply in_seq.
    change Integers.Byte.modulus with 256%Z in R. lia. }
  specialize (H _ Hin). unfold up_ok in H.
  destruct (list_eq_dec _ _ _); [exact e | discriminate].
Qed.

Lemma takefill_length l bs : List.length (bytes_takefill #00 l bs) = l.
Proof.
  unfold bytes_takefill. cbv zeta. destruct (Nat.ltb_spec (List.length bs) l).
  - rewrite List.length_app, List.repeat_length. unfold ssrnat.subn, ssrnat.subn_rec. lia.
  - rewrite List.firstn_length_le; lia.
Qed.

Lemma takefill_eq X l : List.length X = l -> bytes_takefill #00 l X = X.
Proof. intros <-. apply takefill_full. Qed.

Lemma store_takefill {bum : BlockUpdateMemory} m n off bs l :
  store m n off bs l = store m n off (bytes_takefill #00 l bs) l.
Proof.
  unfold store. rewrite (takefill_eq (bytes_takefill #00 l bs) l (takefill_length l bs)).
  reflexivity.
Qed.

Lemma toN_add i c : (0 <= c)%Z -> Z.to_N (Z.of_nat i + c) = (N.of_nat i + Z.to_N c)%N.
Proof.
  intros Hc. apply N2Z.inj. rewrite N2Z.inj_add, !Z2N.id by lia.
  rewrite nat_N_Z. reflexivity.
Qed.

Lemma ascii_upper_zero : ascii_upper #00 = #00.
Proof. reflexivity. Qed.

Ltac zc := repeat match goal with
  | |- context [(Zpos ?a + Zpos ?b)%Z] =>
      let c := eval vm_compute in (Zpos a + Zpos b)%Z in change (Zpos a + Zpos b)%Z with c
  | |- context [(Zpos ?a - Zpos ?b)%Z] =>
      let c := eval vm_compute in (Zpos a - Zpos b)%Z in change (Zpos a - Zpos b)%Z with c
  | |- context [(Zpos ?a + Zneg ?b)%Z] =>
      let c := eval vm_compute in (Zpos a + Zneg b)%Z in change (Zpos a + Zneg b)%Z with c
  end.

Section Exec.
Context {bum : BlockUpdateMemory} (wasi_mem : N).
#[local] Instance ho : @host wasi_hfc bum := wasi_host wasi_mem.

Variables (inst : moduleinst) (FS : list funcinst) (ga ar aw ML : N).
Hypothesis Hfr : lookup_N (inst_funcs inst) 0%N = Some ar.
Hypothesis Hfw : lookup_N (inst_funcs inst) 1%N = Some aw.
Hypothesis Har : lookup_N FS ar = Some (FC_func_host T4 WASI_fd_read).
Hypothesis Haw : lookup_N FS aw = Some (FC_func_host T4 WASI_fd_write).
Hypothesis Hgi : lookup_N (inst_globals inst) 0%N = Some ga.
Hypothesis Hmi : lookup_N (inst_mems inst) 0%N = Some wasi_mem.
Hypothesis HML : (1048832 <= ML)%N.

Definition GT : global_type := Build_global_type MUT_var (T_num T_i32).

Definition Inv (s : store_record) (m : meminst) (g : i32) : Prop :=
  s_funcs s = FS /\ lookup_N (s_mems s) wasi_mem = Some m /\
  lookup_N (s_globals s) ga = Some (Build_globalinst GT (vi g)) /\
  operations.mem_length m = ML.

Notation Fr a b c d e f :=
  (Build_frame [vi a; vi b; vi c; vi d; vi e; vi f] inst).

Definition agree_out (m m' : meminst) (a l : N) : Prop :=
  forall q, (q < a \/ a + l <= q)%N -> byte_at m' q = byte_at m q.

Lemma bytes_at_agree m m' p d a l :
  bytes_at m p d -> agree_out m m' a l ->
  (p + N.of_nat (List.length d) <= a \/ a + l <= p)%N -> bytes_at m' p d.
Proof.
  intros Hb Ha Hd i Hi. rewrite Ha; [apply Hb; exact Hi | lia].
Qed.

Lemma Inv_smem s m g : Inv s m g -> smem s inst = Some m.
Proof. intros [_ [Hm _]]. unfold smem. rewrite Hmi. exact Hm. Qed.

Lemma Inv_set_mem s m g m' :
  Inv s m g -> operations.mem_length m' = ML -> Inv (set_mem wasi_mem s m') m' g.
Proof.
  intros [Hf [Hm [Hg Hl]]] Hl'. unfold Inv, set_mem, upd_s_mem; simpl.
  split; [exact Hf|]. split; [|split; [exact Hg | exact Hl']].
  unfold lookup_N in *. rewrite (nth_error_set_nth m' _ _ m m' _ Hm).
  rewrite Nat.eqb_refl. reflexivity.
Qed.

Lemma Inv_glob s m g : Inv s m g -> sglob_val s inst 0%N = Some (vi g).
Proof.
  intros [_ [_ [Hg _]]]. unfold sglob_val, sglob, sglob_ind. rewrite Hgi, Hg. reflexivity.
Qed.

Lemma Inv_update_glob s m g g' :
  Inv s m g -> exists s', supdate_glob s inst 0%N (vi g') = Some s' /\ Inv s' m g'.
Proof.
  intros [Hf [Hm [Hg Hl]]].
  unfold supdate_glob, supdate_glob_s, sglob_ind. rewrite Hgi. simpl. rewrite Hg. simpl.
  eexists. split; [reflexivity|].
  unfold Inv; simpl. split; [exact Hf|]. split; [exact Hm|]. split; [|exact Hl].
  unfold lookup_N in *. rewrite (nth_error_set_nth _ _ _ _ _ _ Hg).
  rewrite Nat.eqb_refl. reflexivity.
Qed.

Lemma Inv_store32 s m g k off v :
  Inv s m g ->
  (Wasm_int.N_of_uint i32m k + off + 4 <= ML)%N ->
  exists s' m',
    smem_store s inst (Wasm_int.N_of_uint i32m k) off (VAL_int32 v) T_i32 = Some s' /\
    Inv s' m' g /\
    bytes_at m' (Wasm_int.N_of_uint i32m k + off) (serialise_i32 v) /\
    agree_out m m' (Wasm_int.N_of_uint i32m k + off) 4.
Proof.
  intros HI Hb. pose proof HI as [Hf [Hm [Hg Hl]]].
  destruct (store_spec m (Wasm_int.N_of_uint i32m k) off (serialise_i32 v)) as
    [m' [E [Ht [Hl' [Hbs Hu]]]]].
  { rewrite serialise_i32_length. simpl N.of_nat. lia. }
  rewrite serialise_i32_length in E, Hu.
  exists (set_mem wasi_mem s m'), m'.
  split; [|split; [|split]].
  - unfold smem_store. rewrite Hmi, Hm. simpl serialise_num.
    change (ssrnat.nat_of_bin (tnum_length T_i32)) with 4%nat. rewrite E. reflexivity.
  - apply (Inv_set_mem _ _ _ _ HI). lia.
  - exact Hbs.
  - intros q Hq. apply Hu. simpl N.of_nat. lia.
Qed.

Lemma Inv_store8 s m g k off v :
  Inv s m g ->
  (Wasm_int.N_of_uint i32m k + off + 1 <= ML)%N ->
  exists s' m',
    smem_store_packed s inst (Wasm_int.N_of_uint i32m k) off (VAL_int32 v) Tp_i8 = Some s' /\
    Inv s' m' g /\
    bytes_at m' (Wasm_int.N_of_uint i32m k + off) (bytes_takefill #00 1 (serialise_i32 v)) /\
    agree_out m m' (Wasm_int.N_of_uint i32m k + off) 1.
Proof.
  intros HI Hb. pose proof HI as [Hf [Hm [Hg Hl]]].
  destruct (store_spec m (Wasm_int.N_of_uint i32m k) off
              (bytes_takefill #00 1 (serialise_i32 v))) as [m' [E [Ht [Hl' [Hbs Hu]]]]].
  { rewrite takefill_length. simpl N.of_nat. lia. }
  rewrite takefill_length in E, Hu.
  exists (set_mem wasi_mem s m'), m'.
  split; [|split; [|split]].
  - unfold smem_store_packed, store_packed. rewrite Hmi, Hm. simpl serialise_num.
    change (ssrnat.nat_of_bin (tp_length Tp_i8)) with 1%nat.
    rewrite store_takefill, E. reflexivity.
  - apply (Inv_set_mem _ _ _ _ HI). lia.
  - exact Hbs.
  - intros q Hq. apply Hu. simpl N.of_nat. lia.
Qed.

Lemma ss_load32 s m g f st k off al v :
  Inv s m g -> f_inst f = inst ->
  (Wasm_int.N_of_uint i32m k + off + 4 <= ML)%N ->
  bytes_at m (Wasm_int.N_of_uint i32m k + off) (serialise_i32 v) ->
  sstep s f (vi k :: st) (BI_load T_i32 None (Build_memarg off al)) s f (vi v :: st).
Proof.
  intros HI Hf Hb Hbs.
  replace (vi v) with (VAL_num (wasm_deserialise (serialise_i32 v) T_i32))
    by (rewrite deserialise_serialise_i32; reflexivity).
  eapply ss_load; [rewrite Hf; exact (Inv_smem _ _ _ HI)|].
  simpl memarg_offset. change (tnum_length T_i32) with (N.of_nat (List.length (serialise_i32 v))).
  apply load_spec; [|exact Hbs].
  rewrite serialise_i32_length. destruct HI as [_ [_ [_ Hl]]]. simpl N.of_nat. lia.
Qed.

Definition byteval (b : byte) : i32 := I (Memdata.decode_int (bytes_takefill #00 4 [b])).

Lemma ss_load8 s m g f st k off al :
  Inv s m g -> f_inst f = inst ->
  (Wasm_int.N_of_uint i32m k + off + 1 <= ML)%N ->
  sstep s f (vi k :: st) (BI_load T_i32 (Some (Tp_i8, SX_U)) (Build_memarg off al)) s f
    (vi (byteval (byte_at m (Wasm_int.N_of_uint i32m k + off))) :: st).
Proof.
  intros HI Hf Hb.
  change (vi (byteval (byte_at m (Wasm_int.N_of_uint i32m k + off))))
    with (VAL_num (wasm_deserialise
           (bytes_takefill #00 4 [byte_at m (Wasm_int.N_of_uint i32m k + off)]) T_i32)).
  eapply ss_load8u; [rewrite Hf; exact (Inv_smem _ _ _ HI)|].
  unfold load_packed. simpl memarg_offset.
  change 1%N with (N.of_nat (List.length [byte_at m (Wasm_int.N_of_uint i32m k + off)])) at 1.
  rewrite load_spec; [reflexivity | |].
  - destruct HI as [_ [_ [_ Hl]]]. rewrite Hl. simpl List.length. simpl N.of_nat. lia.
  - intros i Hi. simpl in Hi. assert (i = 0)%nat by lia. subst. simpl. rewrite N.add_0_r. reflexivity.
Qed.

Lemma ss_sel s f st c v1 v2 ot :
  sstep s f (vi c :: vi v2 :: vi v1 :: st) (BI_select ot) s f
    (vi (if Wasm_int.Int32.eq c Wasm_int.Int32.zero then v2 else v1) :: st).
Proof.
  pose proof (ss_select s f st c (vi v1) (vi v2) ot) as H.
  destruct (Wasm_int.Int32.eq c Wasm_int.Int32.zero); exact H.
Qed.


(* ---------- wp-level single steps ---------- *)

Lemma wp_lset hs s f f' st v j rest Q :
  (N.to_nat j < List.length (f_locs f))%nat -> set_local f j v = f' -> wp hs s f' st rest Q ->
  wp hs s f (v :: st) (BI_local_set j :: rest) Q.
Proof. intros H <- W. eapply wp_step; [apply ss_local_set; exact H | exact W]. Qed.

Lemma wp_ltee hs s f f' st v j rest Q :
  (N.to_nat j < List.length (f_locs f))%nat -> set_local f j v = f' -> wp hs s f' (v :: st) rest Q ->
  wp hs s f (v :: st) (BI_local_tee j :: rest) Q.
Proof. intros H <- W. eapply wp_step; [apply ss_local_tee; exact H | exact W]. Qed.

Lemma wp_st32 hs s m g f st z off al v rest Q :
  Inv s m g -> f_inst f = inst -> rng z -> (Z.to_N z + off + 4 <= ML)%N ->
  (forall s' m', Inv s' m' g -> bytes_at m' (Z.to_N z + off) (serialise_i32 v) ->
     agree_out m m' (Z.to_N z + off) 4 -> wp hs s' f st rest Q) ->
  wp hs s f (vi v :: vi (I z) :: st) (BI_store T_i32 None (Build_memarg off al) :: rest) Q.
Proof.
  intros HI Hf Hz Hb K. rewrite <- (u32_I z Hz) in Hb, K.
  destruct (Inv_store32 s m g (I z) off v HI Hb) as [s' [m' [E [HI' [Hbs Ha]]]]].
  eapply wp_step; [apply ss_store; rewrite Hf; exact E | exact (K s' m' HI' Hbs Ha)].
Qed.

Lemma wp_st8 hs s m g f st z off al v rest Q :
  Inv s m g -> f_inst f = inst -> rng z -> (Z.to_N z + off + 1 <= ML)%N ->
  (forall s' m', Inv s' m' g ->
     bytes_at m' (Z.to_N z + off) (bytes_takefill #00 1 (serialise_i32 v)) ->
     agree_out m m' (Z.to_N z + off) 1 -> wp hs s' f st rest Q) ->
  wp hs s f (vi v :: vi (I z) :: st) (BI_store T_i32 (Some Tp_i8) (Build_memarg off al) :: rest) Q.
Proof.
  intros HI Hf Hz Hb K. rewrite <- (u32_I z Hz) in Hb, K.
  destruct (Inv_store8 s m g (I z) off v HI Hb) as [s' [m' [E [HI' [Hbs Ha]]]]].
  eapply wp_step; [apply ss_store8; rewrite Hf; exact E | exact (K s' m' HI' Hbs Ha)].
Qed.

Lemma wp_ld32 hs s m g f st z off al v rest Q :
  Inv s m g -> f_inst f = inst -> rng z -> (Z.to_N z + off + 4 <= ML)%N ->
  bytes_at m (Z.to_N z + off) (serialise_i32 v) ->
  wp hs s f (vi v :: st) rest Q ->
  wp hs s f (vi (I z) :: st) (BI_load T_i32 None (Build_memarg off al) :: rest) Q.
Proof.
  intros HI Hf Hz Hb Hbs W. rewrite <- (u32_I z Hz) in Hb, Hbs.
  eapply wp_step; [eapply ss_load32; eauto | exact W].
Qed.

Lemma wp_ld8 hs s m g f st z off al rest Q :
  Inv s m g -> f_inst f = inst -> rng z -> (Z.to_N z + off + 1 <= ML)%N ->
  wp hs s f (vi (byteval (byte_at m (Z.to_N z + off))) :: st) rest Q ->
  wp hs s f (vi (I z) :: st) (BI_load T_i32 (Some (Tp_i8, SX_U)) (Build_memarg off al) :: rest) Q.
Proof.
  intros HI Hf Hz Hb W. rewrite <- (u32_I z Hz) in Hb, W.
  eapply wp_step; [eapply ss_load8; eauto | exact W].
Qed.

Lemma wp_gget hs s m g f st rest Q :
  Inv s m g -> f_inst f = inst -> wp hs s f (vi g :: st) rest Q ->
  wp hs s f st (BI_global_get 0%N :: rest) Q.
Proof.
  intros HI Hf W. eapply wp_step; [apply ss_global_get; rewrite Hf; exact (Inv_glob _ _ _ HI) | exact W].
Qed.

Lemma wp_gset hs s m g g' f st rest Q :
  Inv s m g -> f_inst f = inst -> (forall s', Inv s' m g' -> wp hs s' f st rest Q) ->
  wp hs s f (vi g' :: st) (BI_global_set 0%N :: rest) Q.
Proof.
  intros HI Hf K. destruct (Inv_update_glob s m g g' HI) as [s' [E HI']].
  eapply wp_step; [apply ss_global_set; rewrite Hf; exact E | apply K; exact HI'].
Qed.

Ltac unf := unfold i32c, i32add, i32sub, i32and, i32xor, i32ltu, i32eq, i32ne, i32eqz,
  sel, lget, lset, ltee, st32, ld32, st8, ld8u, marg in *.

Ltac fr := cbv [set_local]; cbn [f_locs f_inst];
  match goal with |- context [N.to_nat ?j] =>
    let k := eval compute in (N.to_nat j) in change (N.to_nat j) with k end;
  cbn [seq.set_nth]; reflexivity.

Ltac norm := rewrite ?add_I, ?sub_I; zc.

Ltac st1 :=
  match goal with
  | |- wp _ _ _ _ (BI_const_num _ :: _) _ => eapply wp_step; [apply ss_const|]
  | |- wp _ _ _ _ (BI_local_get _ :: _) _ => eapply wp_step; [apply ss_local_get; reflexivity|]
  | |- wp _ _ _ _ (BI_local_set _ :: _) _ => eapply wp_lset; [simpl; lia | fr |]
  | |- wp _ _ _ _ (BI_local_tee _ :: _) _ => eapply wp_ltee; [simpl; lia | fr |]
  | |- wp _ _ _ _ (BI_binop _ (Binop_i BOI_add) :: _) _ => eapply wp_step; [apply ss_add|]; norm
  | |- wp _ _ _ _ (BI_binop _ (Binop_i BOI_sub) :: _) _ => eapply wp_step; [apply ss_sub|]; norm
  | |- wp _ _ _ _ (BI_binop _ (Binop_i BOI_and) :: _) _ => eapply wp_step; [apply ss_and|]
  | |- wp _ _ _ _ (BI_binop _ (Binop_i BOI_xor) :: _) _ => eapply wp_step; [apply ss_xor|]
  | |- wp _ _ _ _ (BI_relop _ (Relop_i (ROI_lt SX_U)) :: _) _ => eapply wp_step; [apply ss_ltu|]
  | |- wp _ _ _ _ (BI_relop _ (Relop_i ROI_eq) :: _) _ => eapply wp_step; [apply ss_eq|]
  | |- wp _ _ _ _ (BI_relop _ (Relop_i ROI_ne) :: _) _ => eapply wp_step; [apply ss_ne|]
  | |- wp _ _ _ _ (BI_testop _ TO_eqz :: _) _ => eapply wp_step; [apply ss_eqz|]
  | |- wp _ _ _ _ (BI_select _ :: _) _ => eapply wp_step; [apply ss_sel|]
  end.

Ltac sts := repeat st1.

(* ---------- the shared upcase core ---------- *)

Definition core (off : N) : list basic_instruction :=
  [ld8u off; ltee 1%N; i32c (-97); i32add; i32c 255; i32and; i32c 26; i32ltu; sel;
   lget 1%N; i32xor; st8 off].

Lemma core_ok hs s m g st z off rest Q l0 l1 l2 l3 l4 l5 :
  Inv s m g -> rng z -> (Z.to_N z + off + 1 <= ML)%N ->
  (forall s' m', Inv s' m' g ->
     bytes_at m' (Z.to_N z + off) [ascii_upper (byte_at m (Z.to_N z + off))] ->
     agree_out m m' (Z.to_N z + off) 1 ->
     wp hs s' (Fr l0 (byteval (byte_at m (Z.to_N z + off))) l2 l3 l4 l5) st rest Q) ->
  wp hs s (Fr l0 l1 l2 l3 l4 l5) (vi (I z) :: vi (I 0) :: vi (I 32) :: vi (I z) :: st)
    (core off ++ rest) Q.
Proof.
  intros HI Hz Hb K. unfold core. unf. cbn [List.app].
  eapply wp_ld8; [exact HI | reflexivity | exact Hz | exact Hb |].
  set (b := byte_at m (Z.to_N z + off)) in *.
  eapply wp_ltee; [simpl; lia | fr |].
  eapply wp_step; [apply ss_const|].
  eapply wp_step; [apply ss_add|].
  eapply wp_step; [apply ss_const|].
  eapply wp_step; [apply ss_and|].
  eapply wp_step; [apply ss_const|].
  eapply wp_step; [apply ss_ltu|].
  eapply wp_step; [apply ss_sel|].
  eapply wp_step; [apply ss_local_get; reflexivity|].
  eapply wp_step; [apply ss_xor|].
  change (Wasm_int.Int32.xor _ (byteval b)) with (upv b).
  eapply wp_st8; [exact HI | reflexivity | exact Hz | exact Hb |].
  intros s' m' HI' Hbs Ha. rewrite upv_spec in Hbs. exact (K s' m' HI' Hbs Ha).
Qed.

Lemma tail_ok hs s m g st z off rest Q l0 l1 l2 l3 l4 l5 :
  Inv s m g -> rng z -> (Z.to_N z + off + 1 <= ML)%N ->
  (forall s' m', Inv s' m' g ->
     bytes_at m' (Z.to_N z + off) [ascii_upper (byte_at m (Z.to_N z + off))] ->
     agree_out m m' (Z.to_N z + off) 1 ->
     forall a1, wp hs s' (Fr l0 a1 l2 l3 l4 l5) st rest Q) ->
  wp hs s (Fr l0 l1 l2 l3 l4 l5) (vi (I z) :: st) (upcase_tail off ++ rest) Q.
Proof.
  intros HI Hz Hb K.
  change (upcase_tail off ++ rest) with ([ltee 1%N; i32c 32; i32c 0; lget 1%N] ++ core off ++ rest).
  unfold ltee, i32c, lget. cbn [List.app].
  eapply wp_ltee; [simpl; lia | fr |].
  eapply wp_step; [apply ss_const|].
  eapply wp_step; [apply ss_const|].
  eapply wp_step; [apply ss_local_get; reflexivity|].
  eapply core_ok; [exact HI | exact Hz | exact Hb |].
  intros s' m' HI' Hbs Ha. exact (K s' m' HI' Hbs Ha _).
Qed.

Lemma rem_ok hs s m g st z rest Q l0 l1 l2 l4 l5 :
  Inv s m g -> rng z -> (Z.to_N z + 1048576 + 1 <= ML)%N ->
  (forall s' m', Inv s' m' g ->
     bytes_at m' (Z.to_N z + 1048576) [ascii_upper (byte_at m (Z.to_N z + 1048576))] ->
     agree_out m m' (Z.to_N z + 1048576) 1 ->
     forall a1, wp hs s' (Fr l0 a1 l2 (I z) l4 l5) st rest Q) ->
  wp hs s (Fr l0 l1 l2 (I z) l4 l5) st (rem_code ++ rest) Q.
Proof.
  intros HI Hz Hb K.
  change (rem_code ++ rest) with ([lget 3%N; i32c 32; i32c 0; lget 3%N] ++ core 1048576 ++ rest).
  unfold i32c, lget. cbn [List.app].
  eapply wp_step; [apply ss_local_get; reflexivity|].
  eapply wp_step; [apply ss_const|].
  eapply wp_step; [apply ss_const|].
  eapply wp_step; [apply ss_local_get; reflexivity|].
  eapply core_ok; [exact HI | exact Hz | exact Hb |].
  intros s' m' HI' Hbs Ha. exact (K s' m' HI' Hbs Ha _).
Qed.


(* ---------- the buffer ---------- *)

Definition BUF : N := 1048576.

Definition bufp (m : meminst) (d : list byte) (i : nat) : Prop :=
  forall j, (j < List.length d)%nat ->
    byte_at m (BUF + N.of_nat j) =
      if (j <? i)%nat then ascii_upper (List.nth j d #00) else List.nth j d #00.

Lemma bufp_init m d : bytes_at m BUF d -> bufp m d 0.
Proof.
  intros H j Hj. replace (j <? 0)%nat with false by (destruct j; reflexivity). apply H. exact Hj.
Qed.

Lemma bufp_done m d i : i = List.length d -> bufp m d i -> bytes_at m BUF (List.map ascii_upper d).
Proof.
  intros -> H j Hj. rewrite List.length_map in Hj. rewrite H by exact Hj.
  destruct (Nat.ltb_spec j (List.length d)); [|lia].
  change #00 with (ascii_upper #00) at 2. rewrite List.map_nth. reflexivity.
Qed.

Lemma bufp_step m m' d i A :
  (i < List.length d)%nat -> bufp m d i -> A = (BUF + N.of_nat i)%N ->
  bytes_at m' A [ascii_upper (byte_at m A)] ->
  agree_out m m' A 1 -> bufp m' d (S i).
Proof.
  intros Hi Hb -> Hn Ha j Hj.
  destruct (Nat.eq_dec j i) as [->|Hne].
  - specialize (Hn 0%nat ltac:(simpl; lia)). rewrite N.add_0_r in Hn. rewrite Hn. cbn [List.nth].
    rewrite Hb by exact Hi. rewrite Nat.ltb_irrefl.
    replace (i <? S i)%nat with true by (symmetry; apply Nat.ltb_lt; lia). reflexivity.
  - rewrite Ha by lia. rewrite Hb by exact Hj.
    destruct (Nat.ltb_spec j i), (Nat.ltb_spec j (S i)); try reflexivity; lia.
Qed.

(* ---------- the pair loop ---------- *)

Lemma pair_body_ok hs s m g l0 l1 l2 l4 l5 d i Q :
  (i + 2 <= List.length d)%nat -> (List.length d <= 256)%nat -> Inv s m g -> bufp m d i ->
  (forall s' m' a1, Inv s' m' g -> bufp m' d (i + 2) ->
     wp hs s' (Fr l0 a1 l2 (I (Z.of_nat (i + 2))) l4 l5)
       [vi (wasm_bool (negb (Wasm_int.Int32.eq l5 (I (Z.of_nat (i + 2))))))] [BI_br_if 0%N] Q) ->
  wp hs s (Fr l0 l1 l2 (I (Z.of_nat i)) l4 l5) [] pair_body Q.
Proof.
  intros Hi Hd HI Hb K. unfold pair_body. unfold lget, i32c, i32add. cbn [List.app].
  eapply wp_step; [apply ss_local_get; reflexivity|].
  eapply wp_step; [apply ss_const|].
  eapply wp_step; [apply ss_add|]. rewrite add_I.
  eapply tail_ok; [exact HI | unfold rng; lia | lia |].
  intros s1 m1 HI1 Hbs1 Ha1 a1.
  assert (Hb1 : bufp m1 d (S i)).
  { eapply bufp_step; [lia | exact Hb | | exact Hbs1 | exact Ha1]. unfold BUF. lia. }
  cbn [List.app].
  eapply wp_step; [apply ss_local_get; reflexivity|].
  eapply wp_step; [apply ss_const|].
  eapply wp_step; [apply ss_add|]. rewrite add_I.
  eapply tail_ok; [exact HI1 | unfold rng; lia | lia |].
  intros s2 m2 HI2 Hbs2 Ha2 a2.
  assert (Hb2 : bufp m2 d (i + 2)).
  { replace (i + 2)%nat with (S (S i)) by lia.
    eapply bufp_step; [lia | exact Hb1 | | exact Hbs2 | exact Ha2]. unfold BUF. lia. }
  unfold ltee, i32ne. cbn [List.app].
  eapply wp_step; [apply ss_local_get; reflexivity|].
  eapply wp_step; [apply ss_local_get; reflexivity|].
  eapply wp_step; [apply ss_const|].
  eapply wp_step; [apply ss_add|]. rewrite add_I.
  replace (Z.of_nat i + 2)%Z with (Z.of_nat (i + 2)) by lia.
  eapply wp_ltee; [simpl; lia | fr |].
  eapply wp_step; [apply ss_ne|].
  exact (K s2 m2 a2 HI2 Hb2).
Qed.

Lemma pair_ok d e l0 l2 l4 hs rest Q g :
  (e <= List.length d)%nat -> (List.length d <= 256)%nat ->
  (forall s' m' a1, Inv s' m' g -> bufp m' d e ->
     wp hs s' (Fr l0 a1 l2 (I (Z.of_nat e)) l4 (I (Z.of_nat e))) [] rest Q) ->
  forall k i s m l1, e = (i + 2 * S k)%nat -> Inv s m g -> bufp m d i ->
  wp hs s (Fr l0 l1 l2 (I (Z.of_nat i)) l4 (I (Z.of_nat e))) [] (lp pair_body :: rest) Q.
Proof.
  intros He Hd K. induction k as [|k IH]; intros i s m l1 Ei HI Hb; apply wp_loop;
    (eapply pair_body_ok with (d := d); [lia | exact Hd | exact HI | exact Hb |]);
    intros s' m' a1 HI' Hb'; rewrite eq_I by (unfold rng; lia).
  - replace (Z.of_nat e =? Z.of_nat (i + 2))%Z with true by (symmetry; apply Z.eqb_eq; lia).
    apply wp_br_if_false; [reflexivity|]. apply wp_nil. cbn [after_loop].
    replace (i + 2)%nat with e by lia. apply (K _ m'); [exact HI' | replace e with (i + 2)%nat by lia; exact Hb'].
  - replace (Z.of_nat e =? Z.of_nat (i + 2))%Z with false by (symmetry; apply Z.eqb_neq; lia).
    apply wp_br_if_true; [apply bool_true_nz|]. simpl N.to_nat. cbn [after_loop].
    apply (IH (i + 2)%nat s' m' a1); [lia | exact HI' | exact Hb'].
Qed.

(* ---------- block 3: upper-case the whole buffer ---------- *)

Lemma sel256 n : (n <= 256)%nat ->
  (if Wasm_int.Int32.eq (wasm_bool (Wasm_int.Int32.ltu (I (Z.of_nat n)) (I 256))) Wasm_int.Int32.zero
   then I 256 else I (Z.of_nat n)) = I (Z.of_nat n).
Proof.
  intros Hn. rewrite sel_bool, ltu_I by (unfold rng; lia).
  destruct (Z.ltb_spec (Z.of_nat n) 256); simpl; [reflexivity|].
  f_equal. lia.
Qed.

Lemma block3_ok hs s m g l0 l4 l5 d n st rest Q :
  List.length d = n -> (1 <= n <= 256)%nat -> Inv s m g -> bytes_at m BUF d ->
  (forall s' m' a1 a3 a4 a5, Inv s' m' g -> bytes_at m' BUF (List.map ascii_upper d) ->
     wp hs s' (Fr l0 a1 (I (Z.of_nat n)) a3 a4 a5) st rest Q) ->
  wp hs s (Fr l0 (I (Z.of_nat n)) (I (Z.of_nat n)) (I 0) l4 l5) st (blk block3_body :: rest) Q.
Proof.
  intros Hl Hn HI Hb K. apply wp_block. unfold block3_body. apply wp_block.
  unfold block4_body. unf.
  eapply wp_step; [apply ss_local_get; reflexivity|].
  eapply wp_step; [apply ss_const|].
  eapply wp_step; [apply ss_eq|].
  rewrite eq_I by (unfold rng; lia).
  destruct (Nat.eq_dec n 1) as [E1|N1].
  - replace (Z.of_nat n =? 1)%Z with true by (symmetry; apply Z.eqb_eq; lia).
    apply wp_br_if_true; [apply bool_true_nz|]. simpl N.to_nat. cbn [after_block].
    rewrite <- (List.app_nil_r rem_code).
    eapply rem_ok; [exact HI | unfold rng; lia | simpl; lia |].
    intros s' m' HI' Hbs Ha a1. apply wp_nil. cbn [after_block].
    apply (K _ m'); [exact HI'|]. apply (bufp_done _ _ 1%nat); [lia|].
    eapply bufp_step; [lia | apply bufp_init; exact Hb | | exact Hbs | exact Ha].
    reflexivity.
  - replace (Z.of_nat n =? 1)%Z with false by (symmetry; apply Z.eqb_neq; lia).
    apply wp_br_if_false; [reflexivity|].
    destruct (and_facts n ltac:(lia)) as [A1 A2].
    eapply wp_step; [apply ss_local_get; reflexivity|].
    eapply wp_step; [apply ss_const|].
    eapply wp_step; [apply ss_and|]. rewrite A1.
    eapply wp_lset; [simpl; lia | fr |].
    eapply wp_step; [apply ss_local_get; reflexivity|].
    eapply wp_step; [apply ss_const|].
    eapply wp_step; [apply ss_and|]. rewrite A2.
    eapply wp_lset; [simpl; lia | fr |].
    eapply wp_step; [apply ss_const|].
    eapply wp_lset; [simpl; lia | fr |].
    change (I 0) with (I (Z.of_nat 0)).
    eapply (pair_ok d (n - n mod 2)); [lia | lia | | | exact HI | apply bufp_init; exact Hb].
    2: { instantiate (1 := (n / 2 - 1)%nat).
         pose proof (Nat.div_mod n 2 ltac:(lia)). pose proof (Nat.mod_upper_bound n 2 ltac:(lia)).
         assert (1 <= n / 2)%nat by (apply Nat.div_le_lower_bound; lia). lia. }
    intros s' m' a1 HI' Hb'.
    eapply wp_step; [apply ss_local_get; reflexivity|].
    eapply wp_step; [apply ss_eqz|].
    pose proof (Nat.div_mod n 2 ltac:(lia)) as DM. pose proof (Nat.mod_upper_bound n 2 ltac:(lia)) as MU.
    rewrite zero_I, eq_I by (unfold rng; lia).
    destruct (Nat.eq_dec (n mod 2) 0) as [Ev|Od].
    + replace (0 =? Z.of_nat (n mod 2))%Z with true by (symmetry; apply Z.eqb_eq; lia).
      apply wp_br_if_true; [apply bool_true_nz|]. simpl N.to_nat. cbn [after_block].
      apply (K _ m'); [exact HI'|]. eapply bufp_done; [|exact Hb']. lia.
    + replace (0 =? Z.of_nat (n mod 2))%Z with false by (symmetry; apply Z.eqb_neq; lia).
      apply wp_br_if_false; [reflexivity|]. apply wp_nil. cbn [after_block].
      rewrite <- (List.app_nil_r rem_code).
      eapply rem_ok; [exact HI' | unfold rng; lia | lia |].
      intros s'' m'' HI'' Hbs Ha a1'. apply wp_nil. cbn [after_block].
      apply (K _ m''); [exact HI''|]. apply (bufp_done _ _ (S (n - n mod 2))); [lia|].
      eapply bufp_step; [lia | exact Hb' | | exact Hbs | exact Ha]. unfold BUF. lia.
Qed.


(* ---------- the two host calls ---------- *)

Lemma read_u32_ok m a v :
  (a + 4 <= operations.mem_length m)%N -> bytes_at m a (serialise_i32 v) ->
  read_u32 m a = Some (Z.to_N (Wasm_int.Int32.unsigned v)).
Proof.
  intros Hb Hbs. unfold read_u32.
  assert (L : load m a 0%N (N.of_nat (List.length (serialise_i32 v))) = Some (serialise_i32 v)).
  { apply load_spec; rewrite ?N.add_0_r; [rewrite serialise_i32_length; exact Hb | exact Hbs]. }
  rewrite serialise_i32_length in L. change (N.of_nat 4) with 4%N in L. rewrite L. simpl.
  rewrite decode_serialise_i32. reflexivity.
Qed.

Lemma host_app_eq hs s h vs hs' res :
  wasi_apply wasi_mem hs s T4 h vs = (hs', res) -> host_application hs s T4 h vs hs' res.
Proof. intros H. exact H. Qed.

Lemma wp_fd_read hs s m g f st rest Q :
  Inv s m g -> f_inst f = inst ->
  bytes_at m 1048552 (serialise_i32 (I 1048576)) -> bytes_at m 1048556 (serialise_i32 (I 256)) ->
  (forall m2, Inv (set_mem wasi_mem s m2) m2 g ->
     bytes_at m2 BUF (List.firstn (read_count hs 256) (stdin hs)) ->
     bytes_at m2 1048560 (serialise_i32 (I (Z.of_nat (read_count hs 256)))) ->
     wp (after_read hs (read_count hs 256)) (set_mem wasi_mem s m2) f (vi (I 0) :: st) rest Q) ->
  wp hs s f (vi (I 1048560) :: vi (I 1) :: vi (I 1048552) :: vi (I 0) :: st) (BI_call 0%N :: rest) Q.
Proof.
  intros HI Hf Hb1 Hb2 K. pose proof HI as [HF [Hm [Hg Hl]]].
  set (k := read_count hs 256).
  assert (Hk : (k <= 256)%nat /\ (k <= List.length (stdin hs))%nat).
  { unfold k, read_count. destruct (stdin hs) as [|b l]; [simpl; lia|].
    change (256 =? 0)%N with false. cbv iota. change (N.to_nat 256) with 256%nat. lia. }
  destruct (store_spec m 1048576 0 (List.firstn k (stdin hs))) as [m1 [E1 [_ [Hl1 [Hbs1 Hu1]]]]].
  { rewrite List.firstn_length_le by lia. rewrite Hl. lia. }
  rewrite List.firstn_length_le in E1, Hu1 by lia. rewrite N.add_0_r in Hbs1, Hu1.
  destruct (store_spec m1 1048560 0 (serialise_i32 (I (Z.of_nat k)))) as [m2 [E2 [_ [Hl2 [Hbs2 Hu2]]]]].
  { rewrite serialise_i32_length, Hl1, Hl. simpl. lia. }
  rewrite serialise_i32_length in E2, Hu2. rewrite N.add_0_r in Hbs2, Hu2.
  assert (R1 : read_u32 m 1048552 = Some 1048576%N).
  { rewrite (read_u32_ok _ _ (I 1048576)); [rewrite uns_I by (unfold rng; lia); reflexivity | rewrite Hl; lia | exact Hb1]. }
  assert (R2 : read_u32 m 1048556 = Some 256%N).
  { rewrite (read_u32_ok _ _ (I 256)); [rewrite uns_I by (unfold rng; lia); reflexivity | rewrite Hl; lia | exact Hb2]. }
  eapply wp_call_host; [rewrite Hf; exact Hfr | rewrite HF; exact Har | |].
  - apply host_app_eq. unfold wasi_apply.
    destruct (function_type_eq_dec T4 wasi_fd_type) as [_|C]; [|exfalso; apply C; reflexivity].
    cbn [args4]. unfold fd_read.
    rewrite !Wasm_int.Int32.eq_true. cbn [andb]. rewrite Hm.
    unfold u32. rewrite !u32_I by (unfold rng; lia).
    change (Z.to_N 1048552) with 1048552%N. change (1048552 + 4)%N with 1048556%N.
    rewrite R1, R2. cbv zeta. fold k. rewrite E1.
    unfold write_u32. change (Z.to_N 1048560) with 1048560%N. rewrite E2. reflexivity.
  - apply (K m2).
    + apply (Inv_set_mem _ _ _ _ HI). rewrite Hl2, Hl1. exact Hl.
    + intros i Hi. fold k in Hi |- *. rewrite List.firstn_length_le in Hi by lia.
      change (N.of_nat 4) with 4%N in Hu2.
      rewrite Hu2 by (unfold BUF; lia). apply Hbs1. rewrite List.firstn_length_le by lia. exact Hi.
    + exact Hbs2.
Qed.

Lemma wp_fd_write hs s m g f st rest Q ptr len bs :
  Inv s m g -> f_inst f = inst -> rng ptr -> rng len ->
  bytes_at m 1048564 (serialise_i32 (I ptr)) -> bytes_at m 1048568 (serialise_i32 (I len)) ->
  List.length bs = write_count hs (Z.to_N len) -> bytes_at m (Z.to_N ptr) bs ->
  (Z.to_N ptr + N.of_nat (List.length bs) <= ML)%N ->
  (forall m2, Inv (set_mem wasi_mem s m2) m2 g ->
     bytes_at m2 1048572 (serialise_i32 (I (Z.of_nat (List.length bs)))) ->
     agree_out m m2 1048572 4 ->
     wp (after_write hs bs) (set_mem wasi_mem s m2) f (vi (I 0) :: st) rest Q) ->
  wp hs s f (vi (I 1048572) :: vi (I 1) :: vi (I 1048564) :: vi (I 1) :: st) (BI_call 1%N :: rest) Q.
Proof.
  intros HI Hf Hp Hn Hb1 Hb2 Hlen Hbs Hbd K. pose proof HI as [HF [Hm [Hg Hl]]].
  destruct (store_spec m 1048572 0 (serialise_i32 (I (Z.of_nat (List.length bs)))))
    as [m1 [E1 [_ [Hl1 [Hbs1 Hu1]]]]].
  { rewrite serialise_i32_length, Hl. simpl. lia. }
  rewrite serialise_i32_length in E1. rewrite N.add_0_r in Hbs1.
  assert (L : load m (Z.to_N ptr) 0%N (N.of_nat (List.length bs)) = Some bs).
  { apply load_spec; rewrite N.add_0_r; [rewrite Hl; exact Hbd | exact Hbs]. }
  assert (R1 : read_u32 m 1048564 = Some (Z.to_N ptr)).
  { rewrite (read_u32_ok _ _ (I ptr)); [rewrite uns_I by exact Hp; reflexivity | rewrite Hl; lia | exact Hb1]. }
  assert (R2 : read_u32 m 1048568 = Some (Z.to_N len)).
  { rewrite (read_u32_ok _ _ (I len)); [rewrite uns_I by exact Hn; reflexivity | rewrite Hl; lia | exact Hb2]. }
  eapply wp_call_host; [rewrite Hf; exact Hfw | rewrite HF; exact Haw | |].
  - apply host_app_eq. unfold wasi_apply.
    destruct (function_type_eq_dec T4 wasi_fd_type) as [_|C]; [|exfalso; apply C; reflexivity].
    cbn [args4]. unfold fd_write.
    rewrite !Wasm_int.Int32.eq_true. cbn [andb]. rewrite Hm.
    unfold u32. rewrite !u32_I by (unfold rng; lia).
    change (Z.to_N 1048564) with 1048564%N. change (1048564 + 4)%N with 1048568%N.
    rewrite R1, R2. cbv zeta. rewrite <- Hlen, L.
    unfold write_u32. change (Z.to_N 1048572) with 1048572%N. rewrite E1. reflexivity.
  - apply (K m1).
    + apply (Inv_set_mem _ _ _ _ HI). rewrite Hl1. exact Hl.
    + exact Hbs1.
    + intros q Hq. apply Hu1. rewrite serialise_i32_length. simpl. lia.
Qed.

(* ---------- list helpers ---------- *)

Lemma firstn_add {A} a b (l : list A) :
  (a <= List.length l)%nat -> List.firstn (a + b) l = List.firstn a l ++ List.firstn b (List.skipn a l).
Proof.
  revert l; induction a as [|a IH]; intros [|x l] H; simpl in *; try reflexivity; try lia.
  f_equal. apply IH. lia.
Qed.

Lemma bytes_at_sub m p d off w :
  (off + w <= List.length d)%nat -> bytes_at m p d ->
  bytes_at m (p + N.of_nat off) (List.firstn w (List.skipn off d)).
Proof.
  intros Hl Hb i Hi. rewrite List.firstn_length_le in Hi by (rewrite List.length_skipn; lia).
  rewrite List.nth_firstn. destruct (Nat.ltb_spec i w); [|lia].
  rewrite List.nth_skipn. rewrite <- Hb by lia. f_equal. lia.
Qed.

Lemma wc_bounds hs r : (1 <= r)%nat -> (1 <= write_count hs (N.of_nat r) <= r)%nat.
Proof.
  intros H. unfold write_count. rewrite Nat2N.id. destruct r as [|r]; [lia|].
  replace (N.of_nat (S r) =? 0)%N with false by reflexivity. lia.
Qed.

Lemma below_buf m m' p d a l :
  bytes_at m p d -> agree_out m m' a l -> (a + l <= p)%N -> bytes_at m' p d.
Proof. intros Hb Ha H. eapply bytes_at_agree; [exact Hb | exact Ha | lia]. Qed.

(* ---------- the write loop ---------- *)

Lemma write_ok d out0 si g (Hd : (1 <= List.length d <= 256)%nat) :
  forall r off hs s m l1 l2 l3 l4 l5 Q,
  (off + r)%nat = List.length d -> (1 <= r)%nat ->
  l2 = I (Z.of_nat r) -> l3 = I (1048576 + Z.of_nat off) ->
  Inv s m g -> bytes_at m BUF (List.map ascii_upper d) ->
  stdin hs = si -> stdout hs = out0 ++ List.firstn off (List.map ascii_upper d) ->
  (forall hs' s' m' a1 a2 a3 a4 a5, Inv s' m' g -> stdin hs' = si ->
     stdout hs' = out0 ++ List.map ascii_upper d ->
     Q hs' s' (Fr (I 1048544) a1 a2 a3 a4 a5) (Brk 0)) ->
  wp hs s (Fr (I 1048544) l1 l2 l3 l4 l5) [] [lp write_body] Q.
Proof.
  intros r. induction r as [r IH] using (well_founded_induction lt_wf).
  intros off hs s m l1 l2 l3 l4 l5 Q Hr Hr1 E2 E3 HI Hb Hsi Hso K. subst l2 l3.
  set (X := List.map ascii_upper d) in *.
  assert (HX : List.length X = List.length d) by apply List.length_map.
  apply wp_loop. unfold write_body. unf.
  st1. st1.
  eapply wp_st32; [exact HI | reflexivity | unfold rng; lia | lia |].
  intros s1 m1 HI1 Hbs1 Ha1.
  st1. st1.
  eapply wp_st32; [exact HI1 | reflexivity | unfold rng; lia | lia |].
  intros s2 m2 HI2 Hbs2 Ha2.
  st1. st1.
  eapply wp_st32; [exact HI2 | reflexivity | unfold rng; lia | lia |].
  intros s3 m3 HI3 Hbs3 Ha3.
  change (Z.to_N 1048544 + 24)%N with 1048568%N in *.
  change (Z.to_N 1048544 + 20)%N with 1048564%N in *.
  change (Z.to_N 1048544 + 28)%N with 1048572%N in *.
  sts.
  set (w := write_count hs (N.of_nat r)).
  assert (Hw : (1 <= w <= r)%nat) by (apply wc_bounds; lia).
  assert (Hbw : List.length (List.firstn w (List.skipn off X)) = w).
  { rewrite List.firstn_length_le; [reflexivity|]. rewrite List.length_skipn. lia. }
  eapply (wp_fd_write _ _ m3 g _ _ _ _ (1048576 + Z.of_nat off) (Z.of_nat r)
            (List.firstn w (List.skipn off X)));
    [exact HI3 | reflexivity | unfold rng; lia | unfold rng; lia | | | | | | ].
  - eapply bytes_at_agree; [exact Hbs2 | exact Ha3 | rewrite serialise_i32_length; simpl; lia].
  - eapply bytes_at_agree; [| exact Ha3 | rewrite serialise_i32_length; simpl; lia].
    eapply bytes_at_agree; [exact Hbs1 | exact Ha2 | rewrite serialise_i32_length; simpl; lia].
  - rewrite Hbw. replace (Z.to_N (Z.of_nat r)) with (N.of_nat r) by lia. reflexivity.
  - replace (Z.to_N (1048576 + Z.of_nat off)) with (BUF + N.of_nat off)%N by (unfold BUF; lia).
    apply bytes_at_sub; [lia|].
    eapply below_buf; [| exact Ha3 | unfold BUF; lia].
    eapply below_buf; [| exact Ha2 | unfold BUF; lia].
    eapply below_buf; [exact Hb | exact Ha1 | unfold BUF; lia].
  - rewrite Hbw. lia.
  - intros m4 HI4 Hbs4 Ha4. rewrite Hbw in Hbs4.
    apply wp_br_if_false; [reflexivity|].
    st1.
    eapply wp_ld32; [exact HI4 | reflexivity | unfold rng; lia | lia | exact Hbs4 |].
    st1. st1.
    rewrite zero_I, eq_I by (unfold rng; lia).
    replace (0 =? Z.of_nat w)%Z with false by (symmetry; apply Z.eqb_neq; lia).
    apply wp_br_if_false; [reflexivity|].
    do 6 st1.
    rewrite ltu_I by (unfold rng; lia).
    replace (Z.of_nat r <? Z.of_nat w)%Z with false by (symmetry; apply Z.ltb_ge; lia).
    st1. rewrite sel_bool. cbv beta iota delta [negb].
    sts.
    rewrite zero_I, eq_I by (unfold rng; lia).
    destruct (Nat.eq_dec w r) as [Ew|Nw].
    + replace (0 =? Z.of_nat r - Z.of_nat w)%Z with true by (symmetry; apply Z.eqb_eq; lia).
      apply wp_br_if_true; [apply bool_true_nz|]. simpl N.to_nat. cbn [after_loop].
      apply (K _ _ m4); [exact HI4 | exact Hsi |].
      cbn [stdout after_write]. rewrite Hso, <- List.app_assoc, <- firstn_add by lia.
      f_equal. apply List.firstn_all2. lia.
    + replace (0 =? Z.of_nat r - Z.of_nat w)%Z with false by (symmetry; apply Z.eqb_neq; lia).
      apply wp_br_if_false; [reflexivity|].
      apply wp_br. simpl N.to_nat. cbn [after_loop].
      apply (IH (r - w)%nat ltac:(lia) (off + w)%nat _ _ m4); try assumption.
      * lia.
      * lia.
      * f_equal. lia.
      * f_equal. lia.
      * eapply below_buf; [| exact Ha4 | unfold BUF; lia].
        eapply below_buf; [| exact Ha3 | unfold BUF; lia].
        eapply below_buf; [| exact Ha2 | unfold BUF; lia].
        eapply below_buf; [exact Hb | exact Ha1 | unfold BUF; lia].
      * cbn [stdout after_write]. rewrite Hso, <- List.app_assoc, <- firstn_add by lia. reflexivity.
Qed.


(* ---------- the read loop ---------- *)

Lemma rc_spec hs :
  (read_count hs 256 = 0%nat /\ stdin hs = []) \/
  ((1 <= read_count hs 256 <= 256)%nat /\ (read_count hs 256 <= List.length (stdin hs))%nat).
Proof.
  unfold read_count. destruct (stdin hs) as [|b l]; [left; split; reflexivity|right].
  change (256 =? 0)%N with false. cbv iota. change (N.to_nat 256) with 256%nat.
  simpl List.length. lia.
Qed.

Lemma read_ok :
  forall n hs s m l1 l2 l3 l4 l5 Q,
  List.length (stdin hs) = n -> Inv s m (I 1048544) ->
  (forall hs' s' m' a1 a2 a3 a4 a5, Inv s' m' (I 1048544) -> stdin hs' = [] ->
     stdout hs' = stdout hs ++ List.map ascii_upper (stdin hs) ->
     Q hs' s' (Fr (I 1048544) a1 a2 a3 a4 a5) (Brk 0)) ->
  wp hs s (Fr (I 1048544) l1 l2 l3 l4 l5) [] [lp read_body] Q.
Proof.
  intros n. induction n as [n IH] using (well_founded_induction lt_wf).
  intros hs s m l1 l2 l3 l4 l5 Q Hn HI K.
  apply wp_loop. unfold read_body. unf.
  st1. st1.
  eapply wp_st32; [exact HI | reflexivity | unfold rng; lia | lia |].
  intros s1 m1 HI1 Hbs1 Ha1.
  st1. st1.
  eapply wp_st32; [exact HI1 | reflexivity | unfold rng; lia | lia |].
  intros s2 m2 HI2 Hbs2 Ha2.
  st1. st1.
  eapply wp_st32; [exact HI2 | reflexivity | unfold rng; lia | lia |].
  intros s3 m3 HI3 Hbs3 Ha3.
  change (Z.to_N 1048544 + 12)%N with 1048556%N in *.
  change (Z.to_N 1048544 + 8)%N with 1048552%N in *.
  change (Z.to_N 1048544 + 16)%N with 1048560%N in *.
  sts.
  eapply (wp_fd_read _ _ m3); [exact HI3 | reflexivity | | | ].
  - eapply bytes_at_agree; [exact Hbs2 | exact Ha3 | rewrite serialise_i32_length; simpl; lia].
  - eapply bytes_at_agree; [| exact Ha3 | rewrite serialise_i32_length; simpl; lia].
    eapply bytes_at_agree; [exact Hbs1 | exact Ha2 | rewrite serialise_i32_length; simpl; lia].
  - intros m4 HI4 Hbuf Hcnt.
    set (k := read_count hs 256) in *.
    assert (Hk := rc_spec hs). fold k in Hk.
    assert (Hk256 : (k <= 256)%nat) by (destruct Hk as [[-> _]|[? _]]; lia).
    apply wp_br_if_false; [reflexivity|].
    st1.
    eapply wp_ld32; [exact HI4 | reflexivity | unfold rng; lia | lia | exact Hcnt |].
    st1. st1.
    rewrite zero_I, eq_I by (unfold rng; lia).
    destruct Hk as [[Ek Hnil]|[Hk1 Hkl]].
    + rewrite Ek. change (0 =? Z.of_nat 0)%Z with true.
      apply wp_br_if_true; [apply bool_true_nz|]. simpl N.to_nat. cbn [after_loop].
      apply (K _ _ m4); [exact HI4 | |].
      * cbn [stdin after_read]. rewrite ?Ek, Hnil. reflexivity.
      * cbn [stdout after_read]. rewrite Hnil. simpl. symmetry. apply List.app_nil_r.
    + replace (0 =? Z.of_nat k)%Z with false by (symmetry; apply Z.eqb_neq; lia).
      apply wp_br_if_false; [reflexivity|].
      do 6 st1. rewrite sel256 by lia.
      st1. st1. st1.
      eapply (block3_ok _ _ m4 _ _ _ _ (List.firstn k (stdin hs)) k);
        [apply List.firstn_length_le; lia | lia | exact HI4 | exact Hbuf |].
      intros s5 m5 a1 a3 a4 a5 HI5 Hb5.
      st1. st1.
      apply (write_ok (List.firstn k (stdin hs)) (stdout hs) (List.skipn k (stdin hs)) (I 1048544)
               ltac:(rewrite List.firstn_length_le; lia) k 0%nat _ _ m5);
        [rewrite List.firstn_length_le; lia | lia | reflexivity | reflexivity | exact HI5 | exact Hb5
        | reflexivity | cbn [stdout after_read]; simpl; symmetry; apply List.app_nil_r |].
      intros hs' s' m' b1 b2 b3 b4 b5 HI' Hsi Hso. cbn [after_loop].
      apply (IH (List.length (stdin hs')) ltac:(rewrite Hsi, List.length_skipn; lia) hs' s' m');
        [reflexivity | exact HI' |].
      intros hs'' s'' m'' c1 c2 c3 c4 c5 HI'' Hnil Hso'.
      apply (K _ _ m''); [exact HI'' | exact Hnil |].
      rewrite Hso', Hso, Hsi, <- List.app_assoc, <- List.map_app, List.firstn_skipn. reflexivity.
Qed.

(* ---------- the whole function body ---------- *)

Lemma start_ok hs s m l0 l1 l2 l3 l4 l5 :
  Inv s m (I 1048576) ->
  wp hs s (Fr l0 l1 l2 l3 l4 l5) [] start_body
    (fun hs' s' f' o => o = Norm [] /\ f_inst f' = inst /\
       (exists m', Inv s' m' (I 1048576)) /\
       stdin hs' = [] /\ stdout hs' = stdout hs ++ List.map ascii_upper (stdin hs)).
Proof.
  intros HI. unfold start_body. unf.
  eapply wp_gget; [exact HI | reflexivity |].
  st1. st1. st1.
  eapply wp_gset; [exact HI | reflexivity |]. intros s1 HI1.
  apply wp_block.
  eapply (read_ok _ hs s1 m); [reflexivity | exact HI1 |].
  intros hs' s' m' a1 a2 a3 a4 a5 HI' Hnil Hso. cbn [after_block].
  st1. st1. st1.
  eapply wp_gset; [exact HI' | reflexivity |]. intros s2 HI2.
  apply wp_nil. split; [reflexivity|]. split; [reflexivity|].
  split; [exists m'; exact HI2|]. split; [exact Hnil | exact Hso].
Qed.


(* ---------- calling [_start] (function index 2) ---------- *)

Lemma start_correct hs s m f0 a2 :
  f_inst f0 = inst -> Inv s m (I 1048576) ->
  lookup_N (inst_funcs inst) 2%N = Some a2 ->
  lookup_N FS a2 = Some (FC_func_native (Tf [] []) inst start_code) ->
  exists (hs' : wasi_state) s' m',
    reduce_trans (hs, s, f0, [AI_basic (BI_call 2%N)]) (hs', s', f0, []) /\
    stdin hs' = [] /\ stdout hs' = stdout hs ++ List.map ascii_upper (stdin hs) /\
    Inv s' m' (I 1048576).
Proof.
  intros Hf0 HI Hf2 Ha2.
  destruct (start_ok hs s m (I 0) (I 0) (I 0) (I 0) (I 0) (I 0) HI)
    as [hs' [s' [f' [o [E [-> [_ [[m' HI'] [Hn Hso]]]]]]]]].
  apply exec_sound_norm in E.
  exists hs', s', m'. split; [|split; [exact Hn | split; [exact Hso | exact HI']]].
  eapply run_step; [apply r_call; rewrite Hf0; exact Hf2 |].
  eapply run_step.
  { eapply r_invoke_native with (addr := a2) (vs := []) (ves := []);
      [destruct HI as [HF _]; rewrite HF; exact Ha2 | reflexivity .. ]. }
  eapply rt_trans.
  { apply rt_frame. exact (@rt_lctx _ _ ho [] 0 [] [] _ _ _ _ _ _ _ _ E). }
  eapply run_step; [apply r_frame; apply r_simple; apply rs_label_const; reflexivity |].
  apply (@step1 _ _ ho). apply r_simple. apply rs_local_const; reflexivity.
Qed.

Theorem upper_start (inp : list byte) rs ws s m f0 a2 :
  f_inst f0 = inst ->
  s_funcs s = FS ->
  lookup_N (s_mems s) wasi_mem = Some m ->
  operations.mem_length m = ML ->
  lookup_N (s_globals s) ga =
    Some (Build_globalinst (Build_global_type MUT_var (T_num T_i32))
            (VAL_num (VAL_int32 (Wasm_int.Int32.repr 1048576)))) ->
  lookup_N (inst_funcs inst) 2%N = Some a2 ->
  lookup_N FS a2 = Some (FC_func_native (Tf [] []) inst start_code) ->
  exists (hs' : wasi_state) s',
    reduce_trans
      (Build_wasi_state inp [] rs ws 0 0, s, f0, [AI_basic (BI_call 2%N)])
      (hs', s', f0, []) /\
    stdout hs' = List.map ascii_upper inp /\
    stdin hs' = [] /\
    lookup_N (s_globals s') ga =
      Some (Build_globalinst (Build_global_type MUT_var (T_num T_i32))
              (VAL_num (VAL_int32 (Wasm_int.Int32.repr 1048576)))).
Proof.
  intros Hf0 HF Hm Hl Hg Hf2 Ha2.
  destruct (start_correct (Build_wasi_state inp [] rs ws 0 0) s m f0 a2 Hf0
              (conj HF (conj Hm (conj Hg Hl))) Hf2 Ha2)
    as [hs' [s' [m' [R [Hn [Hso [_ [_ [Hg' _]]]]]]]]].
  exists hs', s'. split; [exact R|]. split; [exact Hso | split; [exact Hn | exact Hg']].
Qed.

End Exec.

(* ---------- the packaged statement ---------- *)

Theorem packaged_upper_start :
  exists md, run_parse_module upper_module = Some md /\
    List.In (Build_module_export (str "_start"%string) (MED_func 2%N)) (mod_exports md) /\
    List.length (mod_imports md) = 2%nat /\
    mod_funcs md = [start_code] /\
    forall (bum : BlockUpdateMemory) (wasi_mem : N) (inst : moduleinst) (FS : list funcinst)
      (ga ar aw ML a2 : N) (inp : list byte) rs ws s m f0,
    lookup_N (inst_funcs inst) 0%N = Some ar ->
    lookup_N (inst_funcs inst) 1%N = Some aw ->
    lookup_N FS ar = Some (FC_func_host T4 WASI_fd_read) ->
    lookup_N FS aw = Some (FC_func_host T4 WASI_fd_write) ->
    lookup_N (inst_globals inst) 0%N = Some ga ->
    lookup_N (inst_mems inst) 0%N = Some wasi_mem ->
    (1048832 <= ML)%N ->
    f_inst f0 = inst ->
    s_funcs s = FS ->
    lookup_N (s_mems s) wasi_mem = Some m ->
    operations.mem_length m = ML ->
    lookup_N (s_globals s) ga =
      Some (Build_globalinst (Build_global_type MUT_var (T_num T_i32))
              (VAL_num (VAL_int32 (Wasm_int.Int32.repr 1048576)))) ->
    lookup_N (inst_funcs inst) 2%N = Some a2 ->
    lookup_N FS a2 = Some (FC_func_native (Tf [] []) inst start_code) ->
    exists (hs' : wasi_state) s',
      @reduce_trans _ _ (wasi_host wasi_mem)
        (Build_wasi_state inp [] rs ws 0 0, s, f0, [AI_basic (BI_call 2%N)])
        (hs', s', f0, []) /\
      stdout hs' = List.map ascii_upper inp /\
      stdin hs' = [] /\
      lookup_N (s_globals s') ga =
        Some (Build_globalinst (Build_global_type MUT_var (T_num T_i32))
                (VAL_num (VAL_int32 (Wasm_int.Int32.repr 1048576)))).
Proof.
  exists upper_decoded. split; [exact upper_decodes|].
  destruct decoded_export_start as [H1 [H2 [H3 _]]].
  split; [exact H1|]. split; [exact H2|]. split; [exact H3|].
  intros. eapply upper_start; eassumption.
Qed.
