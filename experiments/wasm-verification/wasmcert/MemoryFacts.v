From Stdlib Require Import List ZArith NArith Lia.
From Wasm Require Import operations.
Import ListNotations.

Section MemoryFacts.
Context `{bum : BlockUpdateMemory}.

Definition bytes_at (m : meminst) (p : N) (bs : list byte) : Prop :=
  forall i b, List.nth_error bs i = Some b ->
    mem_lookup (p + N.of_nat i) (meminst_data m) = Some b.

Lemma serialise_i64_length v : List.length (serialise_i64 v) = 8%nat.
Proof. apply Memdata.encode_int_length. Qed.

Lemma takefill_serialise v :
  bytes_takefill #00 8 (serialise_i64 v) = serialise_i64 v.
Proof.
  unfold bytes_takefill.
  rewrite serialise_i64_length.
  change (List.firstn 8 (serialise_i64 v) = serialise_i64 v).
  rewrite <- (serialise_i64_length v) at 1.
  apply List.firstn_all.
Qed.

Lemma store_i64_success m n off v :
  (n + off + 8 <= operations.mem_length m)%N ->
  exists m',
    store m n off (serialise_i64 v) 8 = Some m' /\
    operations.mem_length m' = operations.mem_length m /\
    bytes_at m' (n + off) (serialise_i64 v) /\
    (forall q, (q < n + off \/ n + off + 8 <= q)%N ->
      mem_lookup q (meminst_data m') = mem_lookup q (meminst_data m)).
Proof.
  intros Hbound.
  unfold store.
  change (N.of_nat 8) with 8%N.
  rewrite (proj2 (N.leb_le _ _) Hbound), takefill_serialise.
  unfold write_bytes_meminst, write_bytes, write_bytes_gen.
  rewrite serialise_i64_length.
  change (N.of_nat 8) with 8%N.
  remember (fun k => match lookup_N (serialise_i64 v) k with
                     | Some b => b | None => #00 end) as gen.
  destruct (mem_update_gen (n + off) 8 gen (meminst_data m))
    as [md |] eqn:Hwrite.
  - exists (Build_meminst (meminst_type m) md).
    split; [reflexivity |].
    split.
    + unfold operations.mem_length; simpl.
      symmetry; eapply mem_update_gen_length; eauto.
    + split.
      * intros i b Hi; simpl.
        assert (i < 8)%nat as Hidx.
        { rewrite <- (serialise_i64_length v).
          apply List.nth_error_Some; rewrite Hi; discriminate. }
        rewrite (mem_update_gen_lookup Hwrite).
        -- subst gen; unfold lookup_N; rewrite Nat2N.id, Hi; reflexivity.
        -- lia.
      * intros q [Hlt | Hge]; simpl.
        -- eapply mem_update_gen_lookup_lt; eauto.
        -- eapply mem_update_gen_lookup_ge; eauto; lia.
  - exfalso.
    eapply (mem_update_gen_ib (n := n + off) (len := 8)
      (gen := gen) (m := meminst_data m)); eauto.
Qed.

Lemma bytes_at_preserved m m' p bs lo hi :
  bytes_at m p bs ->
  (forall q, (q < lo \/ hi <= q)%N ->
     mem_lookup q (meminst_data m') = mem_lookup q (meminst_data m)) ->
  ((p + N.of_nat (List.length bs) <= lo)%N \/ (hi <= p)%N) ->
  bytes_at m' p bs.
Proof.
  intros Hbytes Hframe Hdisjoint i b Hi.
  assert (i < List.length bs)%nat as Hidx.
  { apply List.nth_error_Some; rewrite Hi; discriminate. }
  rewrite Hframe; [apply Hbytes; assumption |].
  destruct Hdisjoint; [left | right]; lia.
Qed.

End MemoryFacts.
