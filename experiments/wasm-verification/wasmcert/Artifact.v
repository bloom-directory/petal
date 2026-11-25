From Stdlib Require Import List ZArith Strings.Byte.
From Wasm Require Import datatypes binary_format_parser.
Require Export ArtifactBytes.
Import ListNotations.

Definition correct_body : list basic_instruction :=
  [BI_local_get 0%N; BI_local_get 1%N;
   BI_const_num (VAL_int64 Wasm_int.Int64.one);
   BI_binop T_i64 (Binop_i (BOI_shr SX_U)); BI_local_tee 2%N;
   BI_store T_i64 None (Build_memarg 0%N 3%N);
   BI_local_get 0%N; BI_local_get 1%N; BI_local_get 2%N;
   BI_binop T_i64 (Binop_i BOI_sub);
   BI_store T_i64 None (Build_memarg 8%N 3%N)].

Definition faulty_body : list basic_instruction :=
  [BI_local_get 0%N; BI_local_get 1%N;
   BI_const_num (VAL_int64 Wasm_int.Int64.one);
   BI_binop T_i64 (Binop_i (BOI_shr SX_U)); BI_local_tee 1%N;
   BI_store T_i64 None (Build_memarg 8%N 3%N);
   BI_local_get 0%N; BI_local_get 1%N;
   BI_store T_i64 None (Build_memarg 0%N 3%N)].

Definition correct_code := Build_module_func 16%N [T_num T_i64] correct_body.
Definition faulty_code := Build_module_func 16%N [] faulty_body.

(* The trusted extractor identifies the function offset, size and signature.
   Rocq slices the embedded module bytes and checks decoding of the exact body.
   No harness, re-encoding, or separately compiled function is used. *)
Definition selected_code bs start len type_index :=
  match run (List.firstn len (List.skipn start bs)) (fun n => parse_code_func) with
  | Some (locals, body) => Some (Build_module_func type_index locals body)
  | None => None
  end.

Definition correct_selected_code := selected_code correct_bytes
  correct_body_offset correct_body_length correct_type_index.
Definition faulty_selected_code := selected_code faulty_bytes
  faulty_body_offset faulty_body_length faulty_type_index.

Example correct_decodes : correct_selected_code = Some correct_code.
Proof. vm_compute. reflexivity. Qed.

Example faulty_decodes : faulty_selected_code = Some faulty_code.
Proof. vm_compute. reflexivity. Qed.

Print Assumptions correct_decodes.
Print Assumptions faulty_decodes.
