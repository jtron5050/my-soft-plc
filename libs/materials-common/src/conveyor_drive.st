(* Plant-level conveyor drive FB (architecture example).
   Interlocks + start delay TON + RS run latch. *)
FUNCTION_BLOCK ConveyorDrive
  VAR_INPUT
    i_StartCmd : BOOL;
    i_StopCmd : BOOL;
    i_PullCordOK : BOOL;
    i_BeltSlipOK : BOOL;
    i_ChuteBlocked : BOOL;
    i_LocalMode : BOOL;
    i_QPullCord : BOOL;
    i_QBeltSlip : BOOL;
    i_QChute : BOOL;
  END_VAR
  VAR_OUTPUT
    o_RunFwd : BOOL;
    o_Fault : BOOL;
    o_Ready : BOOL;
  END_VAR
  VAR
    start_ton : TON;
    run_rs : RS;
    interlock : BOOL;
  END_VAR

  interlock := i_PullCordOK AND i_QPullCord AND i_BeltSlipOK AND i_QBeltSlip
               AND NOT i_ChuteBlocked AND i_QChute;
  o_Fault := NOT interlock;
  o_Ready := interlock AND NOT i_LocalMode;
  start_ton(IN := i_StartCmd AND o_Ready, PT := T#1s);
  run_rs(S := start_ton.Q, R := i_StopCmd OR o_Fault);
  o_RunFwd := run_rs.Q;
END_FUNCTION_BLOCK
