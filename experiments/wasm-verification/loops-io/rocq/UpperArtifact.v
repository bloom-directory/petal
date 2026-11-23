(* Whole-module decoding of the 517-byte upper-wasi-petal module.
   [upper_decodes] runs WasmCert's own [run_parse_module] on the complete
   embedded module bytes; nothing is sliced or selected outside Rocq. *)
From Stdlib Require Import List ZArith NArith Strings.Byte Strings.String.
From Wasm Require Import datatypes operations binary_format_parser.
Require Export UpperBytes.
Import ListNotations.
Local Open Scope N_scope.

Definition i32c (z : Z) : basic_instruction :=
  BI_const_num (VAL_int32 (Wasm_int.Int32.repr z)).
Definition marg (off al : N) : memarg := Build_memarg off al.

Definition I32 : value_type := T_num T_i32.
Definition i32add := BI_binop T_i32 (Binop_i BOI_add).
Definition i32sub := BI_binop T_i32 (Binop_i BOI_sub).
Definition i32and := BI_binop T_i32 (Binop_i BOI_and).
Definition i32xor := BI_binop T_i32 (Binop_i BOI_xor).
Definition i32ltu := BI_relop T_i32 (Relop_i (ROI_lt SX_U)).
Definition i32eq := BI_relop T_i32 (Relop_i ROI_eq).
Definition i32ne := BI_relop T_i32 (Relop_i ROI_ne).
Definition i32eqz := BI_testop T_i32 TO_eqz.
Definition sel := BI_select None.
Definition lget := BI_local_get.
Definition lset := BI_local_set.
Definition ltee := BI_local_tee.
Definition st32 off := BI_store T_i32 None (marg off 2).
Definition ld32 off := BI_load T_i32 None (marg off 2).
Definition st8 off := BI_store T_i32 (Some Tp_i8) (marg off 0).
Definition ld8u off := BI_load T_i32 (Some (Tp_i8, SX_U)) (marg off 0).
Definition blk := BI_block (BT_valtype None).
Definition lp := BI_loop (BT_valtype None).

(* One upper-casing step for address local $a (already computed on the stack
   by [addr]):  mem[a] := b xor (if ((b - 97) & 255) <u 26 then 32 else 0). *)
Definition upcase_tail (off : N) : list basic_instruction :=
  [ltee 1; i32c 32; i32c 0; lget 1; ld8u off; ltee 1; i32c (-97); i32add;
   i32c 255; i32and; i32c 26; i32ltu; sel; lget 1; i32xor; st8 off].

(* Pair loop body (label @5). *)
Definition pair_body : list basic_instruction :=
  [lget 3; i32c 1048576; i32add] ++ upcase_tail 0 ++
  [lget 3; i32c 1048577; i32add] ++ upcase_tail 0 ++
  [lget 5; lget 3; i32c 2; i32add; ltee 3; i32ne; BI_br_if 0].

(* Block @4: skip the pair loop when n = 1. *)
Definition block4_body : list basic_instruction :=
  [lget 1; i32c 1; i32eq; BI_br_if 0;
   lget 2; i32c 1; i32and; lset 4;
   lget 2; i32c 510; i32and; lset 5;
   i32c 0; lset 3;
   lp pair_body;
   lget 4; i32eqz; BI_br_if 1].

(* Remainder byte at index local 3. *)
Definition rem_code : list basic_instruction :=
  [lget 3; i32c 32; i32c 0; lget 3; ld8u 1048576; ltee 1; i32c (-97); i32add;
   i32c 255; i32and; i32c 26; i32ltu; sel; lget 1; i32xor; st8 1048576].

Definition block3_body : list basic_instruction :=
  blk block4_body :: rem_code.

(* Write-retry loop (label @3 of the outer loop). *)
Definition write_body : list basic_instruction :=
  [lget 0; lget 2; st32 24;
   lget 0; lget 3; st32 20;
   lget 0; i32c 0; st32 28;
   i32c 1; lget 0; i32c 20; i32add; i32c 1; lget 0; i32c 28; i32add;
   BI_call 1; BI_br_if 2;
   lget 0; ld32 28; ltee 1; i32eqz; BI_br_if 2;
   lget 3; lget 2; lget 1; lget 2; lget 1; i32ltu; sel; ltee 1; i32add; lset 3;
   lget 2; lget 1; i32sub; ltee 2; i32eqz; BI_br_if 1;
   BI_br 0].

(* Main read loop (label @2). *)
Definition read_body : list basic_instruction :=
  [lget 0; i32c 256; st32 12;
   lget 0; i32c 1048576; st32 8;
   lget 0; i32c 0; st32 16;
   i32c 0; lget 0; i32c 8; i32add; i32c 1; lget 0; i32c 16; i32add;
   BI_call 0; BI_br_if 1;
   lget 0; ld32 16; ltee 1; i32eqz; BI_br_if 1;
   lget 1; i32c 256; lget 1; i32c 256; i32ltu; sel; lset 2;
   i32c 0; lset 3;
   blk block3_body;
   i32c 1048576; lset 3;
   lp write_body].

Definition start_body : list basic_instruction :=
  [BI_global_get 0; i32c 32; i32sub; ltee 0; BI_global_set 0;
   blk [lp read_body];
   lget 0; i32c 32; i32add; BI_global_set 0].

Definition start_code : module_func :=
  Build_module_func 1 [I32; I32; I32; I32; I32; I32] start_body.

Definition str (s : string) : list Byte.byte := list_byte_of_string s.

Definition fd_type : function_type := Tf [I32; I32; I32; I32] [I32].

Definition upper_decoded : module := {|
  mod_types := [fd_type; Tf [] []];
  mod_funcs := [start_code];
  mod_tables := [];
  mod_mems := [Build_module_mem (Build_limits 17 None)];
  mod_globals := [Build_module_global (Build_global_type MUT_var I32) [i32c 1048576]];
  mod_elems := [];
  mod_datas := [];
  mod_start := None;
  mod_imports :=
    [Build_module_import (str "wasi_snapshot_preview1") (str "fd_read") (MID_func 0);
     Build_module_import (str "wasi_snapshot_preview1") (str "fd_write") (MID_func 0)];
  mod_exports :=
    [Build_module_export (str "memory") (MED_mem 0);
     Build_module_export (str "_start") (MED_func 2)]
|}.

Theorem upper_decodes : run_parse_module upper_module = Some upper_decoded.
Proof. vm_compute. reflexivity. Qed.

(* Facts the execution proof relies on, read off the decoded module. *)
Lemma decoded_imports :
  mod_imports upper_decoded =
    [Build_module_import (str "wasi_snapshot_preview1") (str "fd_read") (MID_func 0);
     Build_module_import (str "wasi_snapshot_preview1") (str "fd_write") (MID_func 0)] /\
  lookup_N (mod_types upper_decoded) 0 = Some fd_type.
Proof. split; reflexivity. Qed.

(* Imports occupy function indices 0 and 1, so the single defined function
   is function index 2, which is what [_start] exports. *)
Lemma decoded_export_start :
  List.In (Build_module_export (str "_start") (MED_func 2)) (mod_exports upper_decoded) /\
  List.length (mod_imports upper_decoded) = 2%nat /\
  mod_funcs upper_decoded = [start_code] /\
  lookup_N (mod_types upper_decoded) (modfunc_type start_code) = Some (Tf [] []).
Proof. repeat split; simpl; auto. Qed.

Lemma decoded_global :
  mod_globals upper_decoded =
    [Build_module_global (Build_global_type MUT_var I32) [i32c 1048576]].
Proof. reflexivity. Qed.

Lemma decoded_memory :
  mod_mems upper_decoded = [Build_module_mem (Build_limits 17 None)].
Proof. reflexivity. Qed.
