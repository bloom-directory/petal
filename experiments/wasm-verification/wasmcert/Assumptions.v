Require Import Artifact Arithmetic Execution Negative.

(* These commands run after the sections close: host and memory interfaces
   are universally quantified parameters, not unbound section assumptions. *)
Print Assumptions Artifact.correct_decodes.
Print Assumptions Artifact.faulty_decodes.
Print Assumptions Arithmetic.split_arithmetic.
Print Assumptions Execution.packaged_correct_function.
Print Assumptions Negative.faulty_violates_SPLIT_1.
