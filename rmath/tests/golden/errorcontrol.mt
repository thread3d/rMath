# Error control: Quiet, Check, Off/On, Abort and TimeConstrained.
# (Message suppression itself is covered by a unit test; these pin the values.)

Quiet[1/0]
=> ComplexInfinity
Check[1/0, "boom"]
=> "boom"
Check[1 + 1, "boom"]
=> 2
Check[Abort[], "caught"]
=> "caught"
Off[Power]; 1/0
=> ComplexInfinity
On[Power]; 1/0
=> ComplexInfinity
TimeConstrained[1 + 1, 5]
=> 2
TimeConstrained[While[True, Null], 0.25]
=> $Aborted
TimeConstrained[While[True, Null], 0.25, "timeout"]
=> "timeout"
