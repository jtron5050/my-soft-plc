(* Demo conveyor — ST form of fixture.spasm (PR-15).
   Fast: interlocks / Ready / Fault
   Main: start TON + RS → RunFwd
   Slow: accumulate RunHours while running *)

PROGRAM Fast
  VAR_GLOBAL
    StartCmd AT %I0 : BOOL;
    StopCmd AT %I1 : BOOL;
    PullCordOK AT %I2 : BOOL;
    BeltSlipOK AT %I3 : BOOL;
    ChuteBlocked AT %I4 : BOOL;
    LocalMode AT %I5 : BOOL;
    RunFwd AT %Q0 : BOOL;
    Fault AT %Q1 : BOOL;
    Ready AT %Q2 : BOOL;
    RunHours AT %R0 : REAL;
    interlock AT %M0 : BOOL;
  END_VAR

  interlock := PullCordOK AND Q_GOOD(PullCordOK)
               AND BeltSlipOK AND Q_GOOD(BeltSlipOK)
               AND NOT ChuteBlocked AND Q_GOOD(ChuteBlocked);
  Fault := NOT interlock;
  Ready := interlock AND NOT LocalMode;
END_PROGRAM

PROGRAM Main
  VAR_GLOBAL
    StartCmd AT %I0 : BOOL;
    StopCmd AT %I1 : BOOL;
    RunFwd AT %Q0 : BOOL;
    Fault AT %Q1 : BOOL;
    Ready AT %Q2 : BOOL;
  END_VAR
  VAR
    start_ton : TON;
    run_rs : RS;
  END_VAR

  start_ton(IN := StartCmd AND Ready, PT := T#1s);
  run_rs(S := start_ton.Q, R := StopCmd OR Fault);
  RunFwd := run_rs.Q;
END_PROGRAM

PROGRAM Slow
  VAR_GLOBAL
    RunFwd AT %Q0 : BOOL;
    RunHours AT %R0 : REAL;
  END_VAR

  IF RunFwd THEN
    RunHours := RunHours + 0.0001388889;
  END_IF;
END_PROGRAM
