/-
  rmath_lean_server — EXPERIMENTAL, NOT WIRED UP.

  Goal: check claims without reloading Mathlib each time (~10s per `lean`
  invocation). Three routes were tried; none is usable yet, and each blocker is
  concrete:

  1. LSP (`lean --server`) — works and is fast, but publishes *empty* diagnostic
     batches for files with genuine errors (`example : (1 : ℚ) = 2 := by
     norm_num` came back clean), so it cannot decide a verdict. It remains
     available behind RMATH_LEAN_SERVER=1 for interactive use only.
  2. Hand-driving the front end (`Elab.Frontend.processCommands` against an
     `importModules`-built environment) — compiles, but returns in microseconds
     with an empty message log: elaboration is deferred to the snapshot
     machinery, so no diagnostics are ever produced. `Elab.async.set {} false`
     did not change that.
  3. Incremental snapshots (`runFrontend` with `incrSaveFileName`) — the save
     step fails with "Could not find native implementation of external
     declaration ... you need to set supportInterpreter", so no snapshot exists
     and every later request fails with "no such file".

  This file builds (`lake build rmath_lean_server`) and reports its failures as
  errors rather than as false successes, which is why it is safe to keep as the
  starting point for the remaining work. Verdicts continue to come from the
  authoritative one-shot run in rmathpy/lean.py.

  Protocol, one request per line on stdin:
      {"code": "<lean source, newlines escaped>"}
  Replies are one JSON object per diagnostic (`severity`, `line`, `column`,
  `data`), followed by a line containing exactly `#END#`. Code must not contain
  an `import` line: the environment already has Mathlib.
-/
import Lean

open Lean

/-- A message rendered the way `lean --json` renders it. -/
def messageJson (msg : Message) (ctx : MessageDataContext) : IO Json := do
  let fmt ← msg.data.format (some ctx)
  return Json.mkObj [
    ("severity", toJson msg.severity),
    ("line", toJson msg.pos.line),
    ("column", toJson msg.pos.column),
    ("data", Json.str (toString fmt))]

/-- The incremental snapshot that carries the imported Mathlib environment, so
each request can skip the ~10s re-import. -/
def incrPath : System.FilePath := ".lake/build/rmath_incr.olean"

/-- Build the snapshot once (this is where Mathlib is loaded). -/
def prime (opts : Options) : IO Unit := do
  let _ ← Elab.runFrontend "import Mathlib\n" opts "RMathPrime.lean" `RMathPrime
            (incrSaveFileName? := some incrPath)
  pure ()

/-- Elaborate one snippet by loading the primed snapshot. -/
def check (opts : Options) (code : String) : IO Unit := do
  let _ ← Elab.runFrontend code opts "RMathSnippet.lean" `RMathSnippet
            (incrLoadFileName? := some incrPath) (jsonOutput := true)
  pure ()

partial def loop (stdin stdout : IO.FS.Stream) (opts : Options) : IO Unit := do
  let line ← stdin.getLine
  if line.isEmpty then
    return ()                        -- EOF: the client went away
  if line.trim.isEmpty then
    loop stdin stdout opts
  else
    let code :=
      match Json.parse line.trim with
      | .ok j => (j.getObjValAs? String "code").toOption.getD ""
      | .error _ => ""
    try
      check opts code
    catch e =>
      stdout.putStrLn (Json.compress (Json.mkObj [("severity", Json.str "error"),
                                                  ("data", Json.str s!"driver exception: {e}")]))
    stdout.putStrLn "#END#"
    stdout.flush
    loop stdin stdout opts

unsafe def main : IO Unit := do
  -- Both of these are normally done by the `lean` CLI (`enableInitializersExecution`
  -- is what lets Mathlib's environment extensions load).
  Lean.enableInitializersExecution
  Lean.initSearchPath (← Lean.findSysroot)
  let stdin ← IO.getStdin
  let stdout ← IO.getStdout
  -- Load Mathlib once into an incremental snapshot; each request reloads that
  -- snapshot instead of re-importing.
  prime {}
  loop stdin stdout {}
