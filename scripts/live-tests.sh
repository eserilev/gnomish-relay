#!/usr/bin/env bash
# Runs the live tests that work on a CI runner (SPEC.md 14.8), one at a time, and writes
# the name of each failed test to FAILED_FILE. With --claude, the tests of the real Claude
# Code also run: they need `claude`, `claude-agent-acp`, a Claude login, and bwrap.
# Usage: scripts/live-tests.sh FAILED_FILE [--claude]
set -uo pipefail
cd "$(dirname "$0")/.."
failed=$1
with_claude=${2:-}
: > "$failed"

# Each entry is the test target of the bridge crate and the name of the test.
always=(
  "lib command_sandbox::tests::a_chat_folder_with_more_than_a_million_entries_gets_a_run"
)
claude=(
  "claude live_claude_answers_lists_forks_and_resumes"
  "claude_gate live_the_hook_of_claude_fires_for_a_read"
  "notices_e2e the_real_claude_fires_the_hooks_and_a_finished_notice_comes"
  "model live_claude_with_no_tools_cannot_read_a_file"
  "model live_claude_answers_a_model_call_inside_the_strict_wall"
  "agent_wall live_claude_answers_in_its_wall_and_runs_a_command_in_the_command_sandbox"
  # Last: it lists the sessions that the tests above made.
  "acp live_claude_lists_and_replays_sessions"
)

# A renamed test matches no name and "passes" with 0 tests, so a pass must say "1 passed".
run_test() {
  local target=$1 name=$2
  local args=(--test "$target")
  if [ "$target" = lib ]; then args=(--lib); fi
  echo "== $name"
  local log
  log=$(mktemp)
  cargo test -q -p bridge --locked "${args[@]}" -- --ignored --exact --nocapture "$name" 2>&1 |
    tee "$log"
  if ! grep -q "test result: ok. 1 passed" "$log"; then
    echo "$name" >> "$failed"
  fi
  rm -f "$log"
}

tests=("${always[@]}")
if [ "$with_claude" = --claude ]; then tests+=("${claude[@]}"); fi
for entry in "${tests[@]}"; do
  read -r target name <<< "$entry"
  run_test "$target" "$name"
done

if [ -s "$failed" ]; then
  echo "Failed:" >&2
  cat "$failed" >&2
  exit 1
fi
