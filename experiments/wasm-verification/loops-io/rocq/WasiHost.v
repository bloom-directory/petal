(* A concrete WasmCert [host] modelling the two WASI preview-1 calls used by
   the upper-casing program: fd_read on fd 0 and fd_write on fd 1, each with
   a single iovec.  The host is deterministic: [host_application] is the graph
   of the function [wasi_apply].  Short reads/writes are driven by the
   schedules [rsched]/[wsched] stored in the host state. *)
From Stdlib Require Import List ZArith NArith Lia Bool.
From mathcomp Require Import ssrbool eqtype seq.
From Wasm Require Import datatypes_properties operations typing host.
Require Import WasiMem.
Import ListNotations.

Inductive wasi_fn : Type := WASI_fd_read | WASI_fd_write.

Definition wasi_fn_eq_dec (a b : wasi_fn) : {a = b} + {a <> b}.
Proof. decide equality. Defined.

#[export] Instance wasi_hfc : host_function_class :=
  @Build_host_function_class wasi_fn wasi_fn_eq_dec.

Record wasi_state : Type := {
  stdin : list byte;
  stdout : list byte;
  rsched : nat -> nat;
  wsched : nat -> nat;
  nreads : nat;
  nwrites : nat;
}.

Definition T_i32' : value_type := T_num T_i32.
Definition wasi_fd_type : function_type :=
  Tf [T_i32'; T_i32'; T_i32'; T_i32'] [T_i32'].

Definition vi32 (z : Z) : value := VAL_num (VAL_int32 (Wasm_int.Int32.repr z)).
Definition errno (z : Z) : result := result_values [vi32 z].
Definition u32 (x : i32) : N := Wasm_int.N_of_uint i32m x.

Definition read_count (hs : wasi_state) (len : N) : nat :=
  match stdin hs with
  | [] => 0
  | _ => if (len =? 0)%N then 0
         else Nat.min (Nat.max 1 (rsched hs (nreads hs)))
                      (Nat.min (N.to_nat len) (List.length (stdin hs)))
  end.

Definition write_count (hs : wasi_state) (len : N) : nat :=
  if (len =? 0)%N then 0
  else Nat.min (Nat.max 1 (wsched hs (nwrites hs))) (N.to_nat len).

Definition after_read (hs : wasi_state) (k : nat) : wasi_state :=
  {| stdin := List.skipn k (stdin hs); stdout := stdout hs;
     rsched := rsched hs; wsched := wsched hs;
     nreads := S (nreads hs); nwrites := nwrites hs |}.

Definition after_write (hs : wasi_state) (bs : list byte) : wasi_state :=
  {| stdin := stdin hs; stdout := stdout hs ++ bs;
     rsched := rsched hs; wsched := wsched hs;
     nreads := nreads hs; nwrites := S (nwrites hs) |}.

Section WasiHost.
Context {bum : BlockUpdateMemory}.
Context (wasi_mem : N).

Definition read_u32 (m : meminst) (a : N) : option N :=
  option_map (fun bs => Z.to_N (Memdata.decode_int bs)) (load m a 0%N 4%N).

Definition write_u32 (m : meminst) (a : N) (k : nat) : option meminst :=
  store m a 0%N (serialise_i32 (Wasm_int.Int32.repr (Z.of_nat k))) 4.

Definition set_mem (s : store_record) (m : meminst) : store_record :=
  upd_s_mem s (set_nth m (s_mems s) (N.to_nat wasi_mem) m).

Definition host_out := (wasi_state * option (store_record * result))%type.

Definition fd_read (hs : wasi_state) (s : store_record) (fd iovs iovs_len p : i32) : host_out :=
  if Wasm_int.Int32.eq fd (Wasm_int.Int32.repr 0) &&
     Wasm_int.Int32.eq iovs_len (Wasm_int.Int32.repr 1) then
    match lookup_N (s_mems s) wasi_mem with
    | None => (hs, Some (s, errno 21))
    | Some m =>
      match read_u32 m (u32 iovs), read_u32 m (u32 iovs + 4) with
      | Some buf, Some len =>
        let k := read_count hs len in
        match store m buf 0%N (List.firstn k (stdin hs)) k with
        | None => (hs, Some (s, errno 21))
        | Some m1 =>
          match write_u32 m1 (u32 p) k with
          | None => (hs, Some (s, errno 21))
          | Some m2 => (after_read hs k, Some (set_mem s m2, errno 0))
          end
        end
      | _, _ => (hs, Some (s, errno 21))
      end
    end
  else (hs, Some (s, errno 8)).

Definition fd_write (hs : wasi_state) (s : store_record) (fd iovs iovs_len p : i32) : host_out :=
  if Wasm_int.Int32.eq fd (Wasm_int.Int32.repr 1) &&
     Wasm_int.Int32.eq iovs_len (Wasm_int.Int32.repr 1) then
    match lookup_N (s_mems s) wasi_mem with
    | None => (hs, Some (s, errno 21))
    | Some m =>
      match read_u32 m (u32 iovs), read_u32 m (u32 iovs + 4) with
      | Some buf, Some len =>
        let k := write_count hs len in
        match load m buf 0%N (N.of_nat k) with
        | None => (hs, Some (s, errno 21))
        | Some bs =>
          match write_u32 m (u32 p) k with
          | None => (hs, Some (s, errno 21))
          | Some m1 => (after_write hs bs, Some (set_mem s m1, errno 0))
          end
        end
      | _, _ => (hs, Some (s, errno 21))
      end
    end
  else (hs, Some (s, errno 8)).

Definition args4 (vs : list value) : option (i32 * i32 * i32 * i32) :=
  match vs with
  | [VAL_num (VAL_int32 a); VAL_num (VAL_int32 b);
     VAL_num (VAL_int32 c); VAL_num (VAL_int32 d)] => Some (a, b, c, d)
  | _ => None
  end.

(* Calls whose declared type or argument values do not match the WASI
   signature (impossible for well-typed configurations) trap. *)
Definition wasi_apply (hs : wasi_state) (s : store_record) (t : function_type)
    (h : wasi_fn) (vs : list value) : host_out :=
  if function_type_eq_dec t wasi_fd_type then
    match args4 vs with
    | Some (a, b, c, d) =>
      match h with
      | WASI_fd_read => fd_read hs s a b c d
      | WASI_fd_write => fd_write hs s a b c d
      end
    | None => (hs, Some (s, result_trap))
    end
  else (hs, Some (s, result_trap)).

Definition wasi_app (hs : wasi_state) (s : store_record) (t : function_type)
    (h : wasi_fn) (vs : list value) (hs' : wasi_state)
    (res : option (store_record * result)) : Prop :=
  wasi_apply hs s t h vs = (hs', res).

(* ---------- shape of the results ---------- *)

Definition mem_change (s s' : store_record) : Prop :=
  s' = s \/
  exists m m', lookup_N (s_mems s) wasi_mem = Some m /\
    meminst_type m' = meminst_type m /\
    operations.mem_length m' = operations.mem_length m /\
    s' = set_mem s m'.

Ltac destruct_all :=
  repeat match goal with
  | |- context [match ?x with _ => _ end] => destruct x eqn:?
  | |- context [if ?x then _ else _] => destruct x eqn:?
  end.

Lemma fd_read_shape hs s a b c d hs' s' r :
  fd_read hs s a b c d = (hs', Some (s', r)) ->
  mem_change s s' /\ exists z, r = errno z.
Proof.
  unfold fd_read, write_u32.
  destruct_all; intros H; inversion H; subst; clear H;
    try (split; [left; reflexivity | eexists; reflexivity]).
  split; [| eexists; reflexivity]. right.
  match goal with
  | H1 : lookup_N _ _ = Some ?m, H2 : store ?m _ _ _ _ = Some ?m1,
    H3 : store ?m1 _ _ _ _ = Some ?m2 |- _ =>
      destruct (store_inv _ _ _ _ _ _ H2); destruct (store_inv _ _ _ _ _ _ H3);
      exists m, m2; split; [exact H1|]; split; [congruence|];
      split; [congruence|reflexivity]
  end.
Qed.

Lemma fd_write_shape hs s a b c d hs' s' r :
  fd_write hs s a b c d = (hs', Some (s', r)) ->
  mem_change s s' /\ exists z, r = errno z.
Proof.
  unfold fd_write, write_u32.
  destruct_all; intros H; inversion H; subst; clear H;
    try (split; [left; reflexivity | eexists; reflexivity]).
  split; [| eexists; reflexivity]. right.
  match goal with
  | H1 : lookup_N _ _ = Some ?m, H3 : store ?m _ _ _ _ = Some ?m2 |- _ =>
      destruct (store_inv _ _ _ _ _ _ H3);
      exists m, m2; split; [exact H1|]; split; [congruence|];
      split; [congruence|reflexivity]
  end.
Qed.

Lemma wasi_apply_shape hs s t h vs hs' s' r :
  wasi_apply hs s t h vs = (hs', Some (s', r)) ->
  mem_change s s' /\
  (r = result_trap \/ (t = wasi_fd_type /\ exists z, r = errno z)).
Proof.
  unfold wasi_apply. intros H.
  destruct (function_type_eq_dec t wasi_fd_type) as [Et|Et].
  - destruct (args4 vs) as [[[[a b] c] d]|].
    + destruct h.
      * apply fd_read_shape in H. destruct H as [H1 [z H2]].
        split; [exact H1 | right; split; [exact Et | exists z; exact H2]].
      * apply fd_write_shape in H. destruct H as [H1 [z H2]].
        split; [exact H1 | right; split; [exact Et | exists z; exact H2]].
    + inversion H; subst. split; [left; reflexivity | left; reflexivity].
  - inversion H; subst. split; [left; reflexivity | left; reflexivity].
Qed.

(* ---------- list lemmas ---------- *)

Lemma length_set_nth {T} (d : T) (l : list T) n y x :
  List.nth_error l n = Some x -> List.length (set_nth d l n y) = List.length l.
Proof.
  revert n; induction l as [|a l IH]; intros [|n] H; simpl in *; try discriminate.
  - reflexivity.
  - f_equal. eapply IH; eauto.
Qed.

Lemma all2_set_nth {T} (R : T -> T -> bool) (d : T) l n x y :
  (forall z, R z z) -> List.nth_error l n = Some x -> R x y ->
  all2 R l (set_nth d l n y).
Proof.
  intros Hr. revert n; induction l as [|a l IH]; intros [|n] H Hxy;
    simpl in *; try discriminate.
  - inversion H; subst. rewrite Hxy. simpl.
    clear -Hr. induction l as [|b l IH]; [reflexivity|]. simpl. rewrite Hr. exact IH.
  - rewrite Hr. simpl. eapply IH; eauto.
Qed.

Lemma all2_refl {T} (R : T -> T -> bool) l : (forall z, R z z) -> all2 R l l.
Proof.
  intros Hr. induction l as [|a l IH]; [reflexivity|]. simpl. rewrite Hr. exact IH.
Qed.

Lemma take_length {T} (l : list T) : take (List.length l) l = l.
Proof. induction l as [|a l IH]; [reflexivity|]. simpl. rewrite IH. reflexivity. Qed.

Lemma comp_ext_refl {T} (R : T -> T -> bool) l :
  (forall z, R z z) -> component_extension R l l.
Proof.
  intros Hr. unfold component_extension. rewrite take_length, ssrnat.leqnn.
  apply all2_refl; exact Hr.
Qed.

Lemma comp_ext_set_nth {T} (R : T -> T -> bool) l n x y :
  (forall z, R z z) -> List.nth_error l n = Some x -> R x y ->
  component_extension R l (set_nth y l n y).
Proof.
  intros Hr Hx Hxy. unfold component_extension.
  rewrite (length_set_nth y l n y x Hx).
  rewrite ssrnat.leqnn. simpl.
  assert (E := take_length (set_nth y l n y)).
  rewrite (length_set_nth y l n y x Hx) in E. rewrite E.
  eapply all2_set_nth; eauto.
Qed.

Lemma nth_error_set_nth {T} (d : T) l n x y k :
  List.nth_error l n = Some x ->
  List.nth_error (set_nth d l n y) k = if Nat.eqb k n then Some y else List.nth_error l k.
Proof.
  revert n k; induction l as [|a l IH]; intros n k H.
  - destruct n; discriminate.
  - destruct n, k; simpl in *; try reflexivity. apply IH; exact H.
Qed.

Lemma Forall_set_nth {T} (P : T -> Prop) l n y :
  List.Forall P l -> P y -> List.Forall P (set_nth y l n y).
Proof.
  intros Hl Hy. revert n; induction Hl as [|a l Ha Hl IH]; intros n.
  - simpl. induction n as [|n IHn]; simpl; constructor; auto.
    destruct n; simpl; [constructor|]; auto.
  - destruct n; simpl; constructor; auto.
Qed.

(* ---------- the three obligations ---------- *)

Lemma func_ext_refl f : func_extension f f.
Proof. apply eqxx. Qed.

Lemma limits_ext_refl l : limits_extension l l.
Proof. unfold limits_extension. rewrite N.leb_refl, eqxx. reflexivity. Qed.

Lemma table_ext_refl t : table_extension t t.
Proof.
  unfold table_extension, table_type_extension.
  rewrite eqxx, limits_ext_refl, ssrnat.leqnn. reflexivity.
Qed.

Lemma mem_ext_refl m : mem_extension m m.
Proof. unfold mem_extension. rewrite limits_ext_refl, N.leb_refl. reflexivity. Qed.

Lemma global_ext_refl g : global_extension g g.
Proof.
  unfold global_extension. rewrite eqxx. destruct (g_type g). simpl.
  rewrite eqxx, orbT. reflexivity.
Qed.

Lemma elem_ext_refl e : elem_extension e e.
Proof. unfold elem_extension. rewrite !eqxx. reflexivity. Qed.

Lemma data_ext_refl d : data_extension d d.
Proof. unfold data_extension. rewrite eqxx. reflexivity. Qed.

Lemma store_extension_refl s : store_extension s s.
Proof.
  unfold store_extension.
  rewrite (comp_ext_refl _ _ func_ext_refl), (comp_ext_refl _ _ table_ext_refl),
    (comp_ext_refl _ _ mem_ext_refl), (comp_ext_refl _ _ global_ext_refl),
    (comp_ext_refl _ _ elem_ext_refl), (comp_ext_refl _ _ data_ext_refl).
  reflexivity.
Qed.

Lemma store_extension_set_mem s m m' :
  lookup_N (s_mems s) wasi_mem = Some m ->
  meminst_type m' = meminst_type m ->
  operations.mem_length m' = operations.mem_length m ->
  store_extension s (set_mem s m').
Proof.
  intros Hm Ht Hl. unfold store_extension, set_mem, upd_s_mem. simpl.
  rewrite (comp_ext_refl _ _ func_ext_refl), (comp_ext_refl _ _ table_ext_refl),
    (comp_ext_refl _ _ global_ext_refl),
    (comp_ext_refl _ _ elem_ext_refl), (comp_ext_refl _ _ data_ext_refl).
  rewrite (comp_ext_set_nth _ _ _ m m' mem_ext_refl Hm).
  - reflexivity.
  - unfold mem_extension. rewrite Ht, Hl, limits_ext_refl, N.leb_refl. reflexivity.
Qed.

Lemma wasi_extension : forall hs t st h vs hs' st' r,
  wasi_app hs st t h vs hs' (Some (st', r)) -> store_extension st st'.
Proof.
  intros hs t st h vs hs' st' r H.
  apply wasi_apply_shape in H. destruct H as [[E|[m [m' [Hm [Ht [Hl E]]]]]] _]; subst.
  - apply store_extension_refl.
  - eapply store_extension_set_mem; eauto.
Qed.

Section Typing.
Variables (s : store_record) (m m' : meminst).
Hypothesis Hm : lookup_N (s_mems s) wasi_mem = Some m.
Hypothesis Ht : meminst_type m' = meminst_type m.
Hypothesis Hl : operations.mem_length m' = operations.mem_length m.

Lemma ext_mem_typing_set a : ext_mem_typing (set_mem s m') a = ext_mem_typing s a.
Proof.
  unfold ext_mem_typing, set_mem, upd_s_mem, lookup_N. simpl.
  rewrite (nth_error_set_nth m' _ _ m m' _ Hm).
  destruct (Nat.eqb (N.to_nat a) (N.to_nat wasi_mem)) eqn:E; [|reflexivity].
  apply Nat.eqb_eq in E. unfold lookup_N in Hm. rewrite E, Hm, Ht. reflexivity.
Qed.

Lemma inst_typing_set i : inst_typing (set_mem s m') i = inst_typing s i.
Proof.
  destruct i. unfold inst_typing.
  assert (Hmap : forall l, map (ext_mem_typing (set_mem s m')) l = map (ext_mem_typing s) l).
  { induction l as [|a l IH]; [reflexivity|]. simpl. rewrite ext_mem_typing_set, IH. reflexivity. }
  rewrite Hmap. reflexivity.
Qed.

Lemma funcinst_typing_set x t : funcinst_typing s x t -> funcinst_typing (set_mem s m') x t.
Proof.
  destruct x; unfold funcinst_typing; [rewrite inst_typing_set|]; exact (fun H => H).
Qed.

Lemma store_typing_set : store_typing s -> store_typing (set_mem s m').
Proof.
  pose proof funcinst_typing_set as HF. pose proof Hm as Hm0.
  revert HF Hm0. unfold set_mem, upd_s_mem.
  destruct s as [fs tabs ms gs es ds]. simpl.
  intros HF Hm0 [Hf [Htab [Hms [Hg [He Hd]]]]].
  split; [|split; [|split; [|split; [|split]]]].
  - eapply List.Forall_impl; [|exact Hf]. intros x [t Hx].
    exists t. exact (HF _ _ Hx).
  - exact Htab.
  - apply Forall_set_nth.
    + exact Hms.
    + unfold lookup_N in Hm0. apply List.nth_error_In in Hm0.
      rewrite List.Forall_forall in Hms. destruct (Hms m Hm0) as [t Hmt].
      exists t. rewrite <- Hmt. destruct m as [mt md], m' as [mt' md'].
      simpl in Ht, Hl |- *. unfold operations.mem_length in Hl. simpl in Hl.
      rewrite Ht. unfold operations.mem_length. simpl. rewrite Hl. reflexivity.
  - exact Hg.
  - exact He.
  - exact Hd.
Qed.
End Typing.

Lemma wasi_typing : forall hs t st h vs hs' st' r,
  wasi_app hs st t h vs hs' (Some (st', r)) -> store_typing st -> store_typing st'.
Proof.
  intros hs t st h vs hs' st' r H.
  apply wasi_apply_shape in H. destruct H as [[E|[m [m' [Hm [Ht [Hl E]]]]]] _]; subst.
  - exact (fun H => H).
  - eapply store_typing_set; eauto.
Qed.

Lemma wasi_respect : forall hs t1s t2s st h vs hs' st' r,
  values_typing st vs t1s ->
  wasi_app hs st (Tf t1s t2s) h vs hs' (Some (st', r)) ->
  result_types_agree st' t2s r.
Proof.
  intros hs t1s t2s st h vs hs' st' r _ H.
  apply wasi_apply_shape in H. destruct H as [_ [E|[Et [z E]]]]; subst.
  - reflexivity.
  - inversion Et; subst. reflexivity.
Qed.

#[export] Instance wasi_host : @host wasi_hfc bum :=
  @Build_host wasi_hfc bum wasi_state wasi_app wasi_extension wasi_typing wasi_respect.

End WasiHost.

Definition ascii_upper (b : byte) : byte :=
  if (97 <=? Integers.Byte.unsigned b)%Z && (Integers.Byte.unsigned b <=? 122)%Z
  then Integers.Byte.repr (Integers.Byte.unsigned b - 32)
  else b.
