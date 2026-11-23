From Stdlib Require Import List ZArith Strings.Byte.
From Wasm Require Import datatypes binary_format_parser.
Require Export ByteSumBytes.
Import ListNotations.

(* The decoded body of function 67 (bloom_verify_byte_sum), split into named
   blocks so that the execution proof can refer to them.  The equality with
   the bytes embedded in the packaged module is checked by [byte_sum_decodes]. *)

Definition bt0 := BT_valtype None.
Definition i32c (z : Z) := BI_const_num (VAL_int32 (Wasm_int.Int32.repr z)).
Definition i64c (z : Z) := BI_const_num (VAL_int64 (Wasm_int.Int64.repr z)).
Definition ld8u := BI_load T_i64 (Some (Tp_i8, SX_U)) (Build_memarg 0%N 0%N).
Definition add32 := BI_binop T_i32 (Binop_i BOI_add).
Definition add64 := BI_binop T_i64 (Binop_i BOI_add).
Definition and32 := BI_binop T_i32 (Binop_i BOI_and).

(* block @0: early return for len = 0 *)
Definition b0_body : list basic_instruction :=
  [BI_local_get 1%N; BI_br_if 0%N; i64c 0; BI_return].

(* innermost block: len >= 4 continues to the main loop, else skip it *)
Definition b3_body : list basic_instruction :=
  [BI_local_get 1%N; i32c 4; BI_relop T_i32 (Relop_i (ROI_ge SX_U));
   BI_br_if 0%N; i32c 0; BI_local_set 3%N; i64c 0; BI_local_set 4%N;
   BI_br 1%N].

(* 4x unrolled main loop *)
Definition l1_body : list basic_instruction :=
  [BI_local_get 4%N; BI_local_get 0%N; BI_local_get 3%N; add32;
   BI_local_tee 1%N; ld8u; add64;
   BI_local_get 1%N; i32c 1; add32; ld8u; add64;
   BI_local_get 1%N; i32c 2; add32; ld8u; add64;
   BI_local_get 1%N; i32c 3; add32; ld8u; add64;
   BI_local_set 4%N; BI_local_get 5%N; BI_local_get 3%N; i32c 4; add32;
   BI_local_tee 3%N; BI_relop T_i32 (Relop_i ROI_ne); BI_br_if 0%N].

Definition l1_loop := BI_loop bt0 l1_body.

Definition b2_body : list basic_instruction :=
  [BI_block bt0 b3_body;
   BI_local_get 1%N; i32c (-4); and32; BI_local_set 5%N;
   i32c 0; BI_local_set 3%N; i64c 0; BI_local_set 4%N;
   l1_loop;
   BI_local_get 2%N; BI_testop T_i32 TO_eqz; BI_br_if 1%N].

(* remainder loop *)
Definition l2_body : list basic_instruction :=
  [BI_local_get 4%N; BI_local_get 1%N; ld8u; add64; BI_local_set 4%N;
   BI_local_get 1%N; i32c 1; add32; BI_local_set 1%N;
   BI_local_get 2%N; i32c (-1); add32; BI_local_tee 2%N; BI_br_if 0%N].

Definition l2_loop := BI_loop bt0 l2_body.

Definition b1_body : list basic_instruction :=
  [BI_block bt0 b2_body;
   BI_local_get 0%N; BI_local_get 3%N; add32; BI_local_set 1%N;
   l2_loop].

Definition byte_sum_body : list basic_instruction :=
  [BI_block bt0 b0_body;
   BI_local_get 1%N; i32c 3; and32; BI_local_set 2%N;
   BI_block bt0 b1_body;
   BI_local_get 4%N].

Definition byte_sum_locals : list value_type :=
  [T_num T_i32; T_num T_i32; T_num T_i64; T_num T_i32].

Definition byte_sum_code :=
  Build_module_func 16%N byte_sum_locals byte_sum_body.

(* The trusted extractor identifies the function offset, size and signature.
   Rocq slices the embedded module bytes and checks decoding of the exact body
   (local declarations, instructions and the final [end]). *)
Definition selected_code bs start len type_index :=
  match run (List.firstn len (List.skipn start bs)) (fun n => parse_code_func) with
  | Some (locals, body) => Some (Build_module_func type_index locals body)
  | None => None
  end.

Definition byte_sum_selected_code := selected_code byte_sum_module
  byte_sum_body_offset byte_sum_body_length byte_sum_type_index.

Example byte_sum_decodes : byte_sum_selected_code = Some byte_sum_code.
Proof. vm_compute. reflexivity. Qed.
