# Mirador shell integration for zsh (regenerated on every launch — edits are lost).
#
# Mirador points ZDOTDIR here so that zsh reads this file first, then hands
# every other startup file back to the person: ZDOTDIR is restored before
# zsh looks for .zprofile, .zshrc and .zlogin, and before any child process
# can see it.

if [[ -n ${MIRADOR_ZDOTDIR+x} ]]; then
  export ZDOTDIR=$MIRADOR_ZDOTDIR
  unset MIRADOR_ZDOTDIR
else
  unset ZDOTDIR
fi

[[ -f ${ZDOTDIR:-$HOME}/.zshenv ]] && source "${ZDOTDIR:-$HOME}/.zshenv"

# Tab completion for `mira`, loaded once, at the first prompt: by then the
# person's .zshrc has run, and with it compinit. A person who never runs
# compinit gets nothing: Mirador does not turn on a completion system they
# did not ask for.
if [[ -o interactive ]]; then
  _mirador_completions() {
    # The person's options and aliases are on by now: `ksh_arrays`, or an
    # alias named `mira`, would otherwise change what this runs.
    emulate -L zsh
    add-zsh-hook -d precmd _mirador_completions
    if (( $+functions[compdef] && $+commands[mira] )); then
      source <(command mira completions zsh 2>/dev/null)
    fi
    unfunction _mirador_completions
  }
  autoload -Uz add-zsh-hook && add-zsh-hook precmd _mirador_completions
fi
