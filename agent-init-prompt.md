Based on the pre-scan data below, determine what's needed to initialize this as a claudine project.

Do NOT use any tools. All the information you need is in the prescan data.

## Prescan format
- `=== REPOS ===` lines: `dir|remote-url|current-branch|recent-branches` (recent branches are comma-separated, up to 10, most recent first)
- `=== STACK ===` lines: `dir: indicator1 indicator2 ...`
- `=== SSH ===` (optional): detected SSH key path from ~/.ssh/config — if present, `ssh_key_needed` should be `true`
- `=== PLUGINS ===`: available claudine plugins with descriptions and dependencies — use ONLY these exact names

## Rules
- `git@` remotes mean SSH is required
- Dependencies must come before dependents in the plugins list (e.g. `node-20` before `heroku`)
- If tech is detected that has no matching plugin, add it to `suggested_plugins`
- Only include plugins clearly needed by the project
- Tech stack detection applies to ALL repos, including local-only ones without remotes
- Docker is already available in the base image — do NOT suggest a docker plugin
- `repos[].dir` should be the repo name from the URL (e.g. `git@host:Org/my-repo.git` → `my-repo`), not the local directory name
- `repos[].branch` should always be `null` — clone the default branch, not the currently checked out branch
- `frontend` or `playwright` in stack indicators → recommend `rodney` (Chrome automation)
- Branch names like `APP-123` or `PROJ-456-description` indicate Linear issue tracking → recommend `lin`
- GitHub remotes → recommend `gh`; GitLab remotes → recommend `glab`

## Output

Write 2-3 lines summarizing the project, then output this JSON as the LAST fenced code block:

```json
{
  "repos": [
    {"url": "remote-url", "dir": "repo-name-from-url", "branch": null}
  ],
  "plugins": ["plugin-name"],
  "suggested_plugins": [
    {"name": "proposed-name", "reason": "why"}
  ],
  "ssh_key_needed": true
}
```
