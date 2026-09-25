import Protocol.Code.Funs
import Protocol.Spec.Folder

/-!
# The action classifier

The classifier answers `deny`, `desktop`, `ask`, or `allow` for each tool call
(SPEC 6.6.3). Paths are compared by their parts, as in S5. The `deny` and
`desktop` patterns compare without ASCII case, on every OS.
-/

open Aeneas Aeneas.Std protocol

namespace Protocol.Spec

/-- From strict to open: `deny < desktop < ask < allow`. -/
def rankV : action.Verdict → Nat
  | .Deny => 0
  | .Desktop => 1
  | .Ask => 2
  | .Allow => 3

/-- The byte strings in a Rust list of byte vectors. -/
def strs (l : List (alloc.vec.Vec Std.U8)) : List (List Byte) := l.map (fun v => bytes v.val)

/-! ## Paths -/

def lowerByte (b : Byte) : Byte := if 65 ≤ b.toNat ∧ b.toNat ≤ 90 then b + 32 else b

def lower (l : List Byte) : List Byte := l.map lowerByte

/-- `p` is a clean path (the form of S5) inside `root`. -/
def within (root p : List Byte) : Prop := cleanPath p ∧ insideRoot root p

/-- Inside, with no regard to ASCII case. -/
def insideCI (folder p : List Byte) : Prop := insideRoot (lower folder) (lower p)

/-- A path inside a `deny` folder of the policy. -/
def denied (policy : action.Policy) (p : List Byte) : Prop :=
  ∃ folder ∈ strs policy.deny_folders.val, insideCI folder p

/-- A pattern part that ends with `*` matches every part that starts with the rest. -/
def partMatch (pat part : List Byte) : Prop :=
  if pat.getLast? = some (ch '*') then pat.dropLast <+: part else pat = part

/-- The parts of the pattern appear in a row somewhere in `parts`. -/
def patternIn (pat parts : List (List Byte)) : Prop :=
  ∃ i, ∃ _ : i + pat.length ≤ parts.length,
    ∀ k (hk : k < pat.length), partMatch pat[k] (parts[i + k]'(by omega))

/-- A path that matches one of the patterns, with no regard to ASCII case. -/
def matchesPattern (patterns : List (List Byte)) (p : List Byte) : Prop :=
  ∃ pat ∈ patterns, patternIn (pathParts (lower pat)) (pathParts (lower p))

/-! ## Commands

A list of names is one string with a space before and after each name. -/

def listed (names : String) (n : List Byte) : Prop := (ch ' ' :: n ++ [ch ' ']) <:+: ascii names

/-- A `/` or `\` starts the name over. -/
def nameStep (acc : List Byte) (b : Byte) : List Byte :=
  if b = ch '/' ∨ b = ch '\\' then [] else acc ++ [lowerByte b]

def withoutExe (n : List Byte) : List Byte :=
  if ascii ".exe" <:+ n then n.take (n.length - 4) else n

/-- The name of a program: no folder, lower case, and no last `.exe`. -/
def progName (w : List Byte) : List Byte := withoutExe (w.foldl nameStep [])

/-- The part of a word before any `=`. -/
def flag (w : List Byte) : List Byte := w.takeWhile (· ≠ ch '=')

def desktopNames : String :=
  " eval sudo sudoedit doas su pkexec run0 gsudo runas cmd command.com powershell pwsh "
def shellNames : String := " sh bash zsh dash ksh mksh fish csh tcsh ash busybox "
def networkNames : String :=
  " curl wget nc ncat netcat socat ssh scp sftp rsync ftp tftp telnet aria2c lftp mosh rclone "
def runnerNames : String :=
  " xargs env exec nohup time timeout nice ionice setsid stdbuf watch parallel flock script strace ltrace valgrind gdb unshare nsenter chroot taskset numactl firejail bwrap runuser sg tmux screen caffeinate wsl crontab systemd-run launchctl schtasks sh bash zsh dash ksh mksh fish csh tcsh ash busybox python python2 python3 node nodejs deno bun perl ruby php lua luajit awk gawk mawk nawk osascript "
def headRunnerNames : String :=
  " . source command builtin enable trap export declare typeset local readonly set unset shopt alias hash "
def neverAlwaysNames : String := " chmod chown chgrp "
def gitRunFlags : String := " -c --config-env --exec-path --upload-pack --receive-pack --exec -x config "
def gitForceFlags : String := " --force --force-with-lease --force-if-includes --hard --mirror "
def findRunFlags : String := " -exec -execdir -ok -okdir -delete "

/-- The words of a simple command, after quote removal. -/
def words (s : shell.Simple) : List (List Byte) := strs s.words.val

def anyName (names : String) (ws : List (List Byte)) : Prop := ∃ w ∈ ws, listed names (progName w)

def headIs (n : String) (ws : List (List Byte)) : Prop := ∃ h, ws.head? = some h ∧ progName h = ascii n

def headListed (names : String) (ws : List (List Byte)) : Prop :=
  ∃ h, ws.head? = some h ∧ listed names (progName h)

/-- `A=1 cmd` sets a variable, such as `LD_PRELOAD`, for `cmd`. -/
def headAssigns (ws : List (List Byte)) : Prop := ∃ h, ws.head? = some h ∧ ch '=' ∈ h

/-- Commands that run other commands or code from their arguments. -/
def runner (ws : List (List Byte)) : Prop :=
  anyName runnerNames ws ∨ headListed headRunnerNames ws ∨ headAssigns ws ∨
    (headIs "find" ws ∧ ∃ w ∈ ws, listed findRunFlags (flag w)) ∨
    (headIs "git" ws ∧ ∃ w ∈ ws, listed gitRunFlags (flag w))

def network (ws : List (List Byte)) : Prop := anyName networkNames ws

/-- A flag with `r` or `R`, as in `rm -rf`. -/
def recursiveFlag (w : List Byte) : Prop := w.head? = some (ch '-') ∧ (ch 'r' ∈ w ∨ ch 'R' ∈ w)

/-- Short flags, such as `-fu`, not a long flag such as `--force`. -/
def shortFlags (w : List Byte) : Prop := w.head? = some (ch '-') ∧ (w.drop 1).head? ≠ some (ch '-')

/-- A word that forces a `git` push or reset. `+main` forces a push. -/
def gitForce (w : List Byte) : Prop :=
  listed gitForceFlags (flag w) ∨ w.head? = some (ch '+') ∨ (shortFlags w ∧ ch 'f' ∈ w)

/-- The "never always" commands of SPEC 6.6.3, apart from the `desktop` ones. -/
def neverAlways (ws : List (List Byte)) : Prop :=
  runner ws ∨ network ws ∨ anyName neverAlwaysNames ws ∨
    (headIs "rm" ws ∧ ∃ w ∈ ws, recursiveFlag w) ∨ (headIs "git" ws ∧ ∃ w ∈ ws, gitForce w)

/-- The quote state of the splitter after one byte: `'` starts and ends single quotes,
`"` double quotes, and `\` escapes the next byte. The lexer of `shell.rs` changes its
mode the same way. -/
def quoteStep (mode : shell.Mode) (b : Byte) : shell.Mode :=
  match mode with
  | .Plain => if b = ch '\'' then .Single else if b = ch '"' then .Double
      else if b = ch '\\' then .Escape else .Plain
  | .Single => if b = ch '\'' then .Plain else .Single
  | .Double => if b = ch '"' then .Plain else if b = ch '\\' then .DoubleEscape else .Double
  | .DoubleEscape => .Double
  | .Escape => .Plain

/-- The quote state before byte `i`. -/
def modeAt (raw : List Byte) (i : Nat) : shell.Mode := (raw.take i).foldl quoteStep .Plain

/-- `$(` or a backtick starts at byte `i`. -/
def opensAt (raw : List Byte) (i : Nat) : Prop :=
  raw[i]? = some (ch '`') ∨ (raw[i]? = some (ch '$') ∧ raw[i + 1]? = some (ch '('))

/-- Command substitution: `$(` or a backtick outside single quotes. Inside single quotes
both are plain text. An escaped one still counts, which is stricter than the shell. -/
def substitution (raw : List Byte) : Prop :=
  ∃ i < raw.length, modeAt raw i ≠ .Single ∧ opensAt raw i

/-- `eval`, `sudo`, `cmd.exe`, PowerShell, or a shell after a `|`. -/
def desktopSimple (s : shell.Simple) : Prop :=
  anyName desktopNames (words s) ∨ (s.link = .Pipe ∧ anyName shellNames (words s))

end Protocol.Spec
