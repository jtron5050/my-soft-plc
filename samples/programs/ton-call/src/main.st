(* TON primitive call — behavioral match for samples/programs/ton-call/fixture.spasm *)
PROGRAM Main
  VAR
    ton : TON;
    q_slot : BOOL;
    et_slot : TIME;
  END_VAR

  ton(IN := TRUE, PT := T#1s);
  q_slot := ton.Q;
  et_slot := ton.ET;
END_PROGRAM
