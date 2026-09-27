import Protocol.Spec.Action

/-!
# "Always allow"

The rule that one click in the game adds (SPEC 6.6.5). The bridge makes it from the
words of one simple command.
-/

open Aeneas Aeneas.Std protocol

namespace Protocol.Spec

def subcommandNames : String :=
  " git cargo npm pnpm yarn go uv pip poetry gradle mvn dotnet rustup just "
/-- Tools that run any program or download code, and tools that publish. -/
def noRuleNames : String := " npx bunx uvx pipx docker twine gh "
/-- The same, as `name:word` for a tool with subcommands. -/
def noRulePairs : String :=
  " npm:exec pnpm:exec pnpm:dlx yarn:dlx yarn:exec uv:run poetry:run git:push cargo:publish npm:publish "
/-- The shell syntax that the allow table of the config refuses (SPEC 12). -/
def specialChars : String := "*?[]$`'\"\\;&|<>(){}~#="

/-- Printable ASCII with no space and no shell syntax. -/
def ruleByte (b : Byte) : Prop := 33 ≤ b.toNat ∧ b.toNat ≤ 126 ∧ b ∉ ascii specialChars

/-- A word of a rule: 1 to 64 plain bytes, not a flag and not a toolchain such as `+nightly`. -/
def plainWord (w : List Byte) : Prop :=
  1 ≤ w.length ∧ w.length ≤ 64 ∧ w.head? ≠ some (ch '-') ∧ w.head? ≠ some (ch '+') ∧
    ∀ b ∈ w, ruleByte b

def hasSlash (w : List Byte) : Prop := ch '/' ∈ w

/-- A tool that runs any program, downloads code, or publishes: `npx`, `uv run`, `git push`. -/
def noRuleTool (ws : List (List Byte)) : Prop :=
  ∃ h, ws.head? = some h ∧ (listed noRuleNames (progName h) ∨
    ∃ w, ws[1]? = some w ∧ listed noRulePairs (progName h ++ [ch ':'] ++ progName w))

/-- A rule covers a command that starts with its words. An empty rule covers nothing. -/
def ruleMatches (rule ws : List (List Byte)) : Prop := rule ≠ [] ∧ rule <+: ws

/-- `s` is a simple command of the shell command `call`. -/
def inCall (call : action.ToolCall) (s : shell.Simple) : Prop :=
  ∃ raw cwd script, call = .Command raw cwd ∧
    shell.split (alloc.vec.Vec.deref raw) = .ok (some script) ∧ s ∈ script.simples.val

end Protocol.Spec
