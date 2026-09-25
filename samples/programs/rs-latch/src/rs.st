(* User FB RS latch — Appendix A.8 shape *)
FUNCTION_BLOCK RsLatch
  VAR_INPUT
    S : BOOL;
    R : BOOL;
  END_VAR
  VAR_OUTPUT
    Q : BOOL;
  END_VAR

  Q := (S OR Q) AND NOT R;
END_FUNCTION_BLOCK

PROGRAM Main
  VAR
    latch : RsLatch;
    s_in : BOOL;
    r_in : BOOL;
    q_out : BOOL;
  END_VAR

  latch(S := s_in, R := r_in, Q => q_out);
END_PROGRAM
