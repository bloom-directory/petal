(* Facts about WasmCert's generic linear-memory operations ([load], [store])
   over an arbitrary [BlockUpdateMemory] implementation. *)
From Stdlib Require Import List ZArith NArith Lia.
From mathcomp Require Import ssrbool seq.
From Wasm Require Import list_extra operations.
Import ListNotations.

Section WasiMem.
Context {bum : BlockUpdateMemory}.

Definition byte_at (m : meminst) (a : N) : byte :=
  match mem_lookup a (meminst_data m) with Some b => b | None => #00 end.

Definition bytes_at (m : meminst) (p : N) (bs : list byte) : Prop :=
  forall i, (i < List.length bs)%nat -> byte_at m (p + N.of_nat i) = List.nth i bs #00.

Lemma takefill_full bs : bytes_takefill #00 (List.length bs) bs = bs.
Proof.
  unfold bytes_takefill.
  rewrite (proj2 (PeanoNat.Nat.ltb_ge _ _) (le_n _)).
  apply List.firstn_all.
Qed.

Lemma lookup_N_length {A} (l : list A) n x :
  lookup_N l (N.of_nat n) = Some x -> List.nth n l x = x.
Proof.
  unfold lookup_N. rewrite Nat2N.id. intros H.
  apply List.nth_error_nth. exact H.
Qed.

Lemma store_spec m n off bs :
  (n + off + N.of_nat (List.length bs) <= operations.mem_length m)%N ->
  exists m',
    store m n off bs (List.length bs) = Some m' /\
    meminst_type m' = meminst_type m /\
    operations.mem_length m' = operations.mem_length m /\
    bytes_at m' (n + off) bs /\
    (forall q, (q < n + off \/ n + off + N.of_nat (List.length bs) <= q)%N ->
       byte_at m' q = byte_at m q).
Proof.
  intros Hb. unfold store.
  rewrite (proj2 (N.leb_le _ _) Hb), takefill_full.
  unfold write_bytes_meminst, write_bytes, write_bytes_gen.
  remember (fun k => match lookup_N bs k with Some b => b | None => #00 end) as gen.
  destruct (mem_update_gen (n + off) (N.of_nat (List.length bs)) gen (meminst_data m))
    as [md|] eqn:Hw.
  - exists (Build_meminst (meminst_type m) md).
    split; [reflexivity|]. split; [reflexivity|].
    split; [unfold operations.mem_length; simpl; symmetry; eapply mem_update_gen_length; eauto|].
    split.
    + intros i Hi. unfold byte_at. simpl.
      rewrite (mem_update_gen_lookup Hw); [| lia].
      subst gen. unfold lookup_N. rewrite Nat2N.id.
      destruct (List.nth_error bs i) eqn:E.
      * symmetry. apply List.nth_error_nth. exact E.
      * apply List.nth_error_None in E. lia.
    + intros q [Hq|Hq]; unfold byte_at; simpl.
      * erewrite mem_update_gen_lookup_lt; eauto.
      * erewrite mem_update_gen_lookup_ge; eauto. lia.
  - exfalso. eapply mem_update_gen_ib; [| exact Hw]. exact Hb.
Qed.

Lemma store_inv m n off bs l m' :
  store m n off bs l = Some m' ->
  meminst_type m' = meminst_type m /\
  operations.mem_length m' = operations.mem_length m.
Proof.
  unfold store. destruct (N.leb _ _); [|discriminate].
  unfold write_bytes_meminst, write_bytes, write_bytes_gen.
  destruct (mem_update_gen _ _ _ _) as [md|] eqn:Hw; [|discriminate].
  intros H; inversion H; subst; clear H. split; [reflexivity|].
  unfold operations.mem_length; simpl. symmetry; eapply mem_update_gen_length; eauto.
Qed.

Lemma those_map_some (g : nat -> byte) (f : nat -> option byte) len k :
  (forall i, (i < len)%nat -> f (k + i)%nat = Some (g (k + i)%nat)) ->
  those (List.map f (iota k len)) = Some (List.map g (iota k len)).
Proof.
  rewrite <- !those_those0.
  revert k; induction len as [|len IH]; intros k H; [reflexivity|].
  simpl. pose proof (H 0%nat ltac:(lia)) as H0. rewrite Nat.add_0_r in H0.
  rewrite H0. rewrite IH; [reflexivity|].
  intros i Hi. replace (S k + i)%nat with (k + S i)%nat by lia. apply H. lia.
Qed.

Lemma In_iota_ge i k n : List.In i (iota k n) -> (k <= i)%nat.
Proof.
  revert k; induction n as [|n IH]; intros k H; simpl in H; [contradiction|].
  destruct H as [H|H]; [lia|]. apply IH in H. lia.
Qed.

Lemma map_nth_iota (bs : list byte) k :
  List.map (fun i => List.nth (i - k) bs #00) (iota k (List.length bs)) = bs.
Proof.
  revert k; induction bs as [|b bs IH]; intros k; [reflexivity|].
  simpl. rewrite Nat.sub_diag. f_equal.
  rewrite <- (IH (S k)) at 2. apply List.map_ext_in.
  intros i Hi. apply In_iota_ge in Hi.
  destruct (i - k)%nat eqn:E; [lia|]. f_equal. lia.
Qed.

Lemma nat_of_bin_of_nat n : ssrnat.nat_of_bin (N.of_nat n) = n.
Proof.
  destruct n as [|n]; [reflexivity|]. simpl.
  induction n as [|n IH]; [reflexivity|].
  simpl. rewrite ssrnat.nat_of_succ_pos, IH. reflexivity.
Qed.

Lemma load_spec m n off bs :
  (n + off + N.of_nat (List.length bs) <= operations.mem_length m)%N ->
  bytes_at m (n + off) bs ->
  load m n off (N.of_nat (List.length bs)) = Some bs.
Proof.
  intros Hb Hbs. unfold load.
  rewrite (proj2 (N.leb_le _ _)) by lia.
  unfold read_bytes.
  replace (ssrnat.nat_of_bin (N.of_nat (List.length bs))) with (List.length bs)
    by (symmetry; apply nat_of_bin_of_nat).
  rewrite (those_map_some (fun i => List.nth (i - 0) bs #00)).
  - f_equal. apply map_nth_iota.
  - intros i Hi. simpl. rewrite Nat.sub_0_r.
    rewrite <- (Hbs i Hi). unfold byte_at.
    destruct (mem_lookup _ _) eqn:E; [reflexivity|].
    exfalso. revert E. apply mem_lookup_ib.
    unfold operations.mem_length in Hb. lia.
Qed.

(* Four-byte little-endian integers. *)
Lemma serialise_i32_length v : List.length (serialise_i32 v) = 4%nat.
Proof. apply Memdata.encode_int_length. Qed.

Lemma decode_serialise_i32 v :
  Memdata.decode_int (serialise_i32 v) = Wasm_int.Int32.unsigned v.
Proof.
  unfold serialise_i32. rewrite Memdata.decode_encode_int.
  rewrite Z.mod_small; [reflexivity|].
  pose proof (Wasm_int.Int32.unsigned_range v) as R.
  change (Zpower.two_p (Z.of_nat 4 * 8)) with Wasm_int.Int32.modulus. lia.
Qed.

Lemma deserialise_serialise_i32 v :
  wasm_deserialise (serialise_i32 v) T_i32 = VAL_int32 v.
Proof.
  simpl. rewrite decode_serialise_i32, Wasm_int.Int32.repr_unsigned. reflexivity.
Qed.

End WasiMem.
