(* Arithmetic + quality gate demo — matches samples/programs/arith-demo/fixture.spasm *)
PROGRAM Main
  VAR_GLOBAL
    I0 AT %I0 : BOOL;
    I1 AT %I1 : BOOL;
    Q0 AT %Q0 : BOOL;
  END_VAR

  Q0 := I0 AND Q_GOOD(I0) AND I1;
END_PROGRAM
