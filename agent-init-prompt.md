Analyze the current directory to determine what's needed to initialize it as a claudine project.

IMPORTANT: Do NOT use TodoWrite, planning tools, or any task management. Work through the analysis directly and output the result. Be efficient — only inspect files that help determine the tech stack, not every file in every directory. Skip directories that are clearly not git repos (no .git).

## Tasks

1. **Find git repos**: Run a single command to find `.git` directories at the root and one level deep. For each, run `git remote get-url origin` and `git branch --show-current`. Skip worktrees and non-repo directories.

2. **Detect tech stack**: For each repo, check ONLY for these indicator files at the repo root (do not recurse deeply):
   - `package.json` → Node.js (check `engines` field for version)
   - `pyproject.toml` or `requirements.txt` → Python
   - `Cargo.toml` → Rust
   - `go.mod` → Go
   - `.github/` → GitHub
   - `.gitlab-ci.yml` → GitLab
   - `Procfile` → Heroku
   - `playwright.config.*` → Browser automation

3. **Map to claudine plugins** (use ONLY these exact names):
   - `node-20` (Node 20.x or unspecified), `node-22` (Node 22.x), `node-24` (Node 24.x)
   - `python-venv` (Python 3 venv support)
   - `rust` (Rust toolchain)
   - `go` (Go toolchain)
   - `gh` (GitHub CLI)
   - `glab` (GitLab CLI)
   - `aws` (AWS CLI v2)
   - `heroku` (Heroku CLI — requires one of: node-20, node-22, node-24)
   - `lin` (Linear CLI)
   - `rodney` (Chrome automation)

4. **Check SSH transport**: If any remote URL uses `git@` format, SSH is required.

5. **Flag missing plugins**: If the project uses a technology that would benefit from a claudine plugin but none exists in the list above (e.g. Ruby, Java, .NET, Terraform, Kubernetes, etc.), note it as a suggested plugin. Include what the plugin would need to provide and why the project needs it.

## Output

Keep it brief. Write a short summary (a few lines) of what the project is, repos found, and plugin recommendations. Then output the JSON block as the LAST fenced code block:

```json
{
  "repos": [
    {"url": "the-git-remote-origin-url", "dir": "directory-name", "branch": "current-branch-or-null"}
  ],
  "plugins": ["plugin-name"],
  "suggested_plugins": [
    {"name": "proposed-plugin-name", "reason": "why this plugin should be added to claudine"}
  ],
  "ssh_key_needed": true
}
```

JSON field rules:
- `repos[].url` — the git remote origin URL (not the local filesystem path)
- `repos[].dir` — the directory name relative to the analyzed root
- `repos[].branch` — current branch name, or `null` to use the repo default
- `plugins` — only names from the list above, ordered so dependencies come first (e.g. `node-20` before `heroku`)
- `suggested_plugins` — technologies detected that have no matching claudine plugin yet. Use an empty array if none.
- `ssh_key_needed` — `true` if any repo remote uses SSH (`git@`) transport
- Only include plugins that are clearly needed based on what you found
