#!/bin/sh
# Put this project in a new GitHub repository, using the git and gh (GitHub CLI) already
# installed and signed in on this computer.
#
#   ./publish-to-github.sh                       -> private repo "wireant"
#   ./publish-to-github.sh my-name --public      -> public repo "my-name"
#
# Running it again later just commits and pushes your newest changes.
set -e
cd "$(dirname "$0")"
NAME="${1:-wireant}"
VISIBILITY="${2:---private}"

command -v git >/dev/null 2>&1 || { echo "git isn't installed (sudo apt install git)" >&2; exit 1; }
command -v gh >/dev/null 2>&1 || { echo "GitHub CLI isn't installed (sudo apt install gh), then run: gh auth login" >&2; exit 1; }
gh auth status >/dev/null 2>&1 || { echo "Sign in to GitHub first: gh auth login" >&2; exit 1; }

if [ ! -d .git ]; then
    git init -q -b main
fi
git add -A
if ! git diff --cached --quiet; then
    if git rev-parse --verify -q HEAD >/dev/null; then
        MSG="Update wireant"
    else
        MSG="wireant: first version"
    fi
    git commit -q -m "$MSG" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
fi

if git remote get-url origin >/dev/null 2>&1; then
    git push -u origin main
else
    gh repo create "$NAME" "$VISIBILITY" --source . --remote origin --push \
        --description "Wire antenna analyser for Linux: multi-threaded MININEC solver, NEC-2, .maa/.nec/.ez files"
fi
echo "Done: $(gh repo view --json url -q .url 2>/dev/null || git remote get-url origin)"
