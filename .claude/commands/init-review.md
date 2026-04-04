# Project Folder Review for Claudine Init

Review the directory at `$ARGUMENTS` and produce a complete claudine project initialization plan.

## Available Claudine Plugins

Map detected technologies to these exact plugin names:

| Plugin | Matches |
|--------|---------|
| `node-20` | Node.js 20.x LTS |
| `node-22` | Node.js 22.x LTS |
| `node-24` | Node.js 24.x |
| `gh` | GitHub CLI (recommend when repos use GitHub) |
| `heroku` | Heroku CLI (requires a node plugin) |
| `python-venv` | Python 3 venv support |
| `rust` | Rust toolchain |
| `go` | Go toolchain |
| `lin` | Linear CLI (recommend when .linear or linear references found) |
| `glab` | GitLab CLI (recommend when repos use GitLab) |
| `aws` | AWS CLI v2 |
| `rodney` | Chrome automation |

## Analysis Steps

### 1. Identify Git Repositories

Scan the target directory for `.git` directories (top-level and one level deep). For each repo found:
- Read the git remote origin URL (`git -C <path> remote get-url origin`)
- Read the current branch (`git -C <path> branch --show-current`)
- Note whether the remote uses SSH (`git@`) or HTTPS
- Record the directory name relative to the target folder

If only a single `.git` exists at the root, treat the entire folder as one repo. If multiple `.git` directories exist at subdirectory level, treat it as a multi-repo workspace.

### 2. Detect Tech Stack

For each repo, look for these indicators:

**Languages & Runtimes:**
- `package.json` or `node_modules/` → Node.js (check `engines` field for version, check for `pnpm-lock.yaml`/`yarn.lock`/`package-lock.json`)
- `pyproject.toml`, `requirements.txt`, `Pipfile`, `setup.py`, or `*.py` files → Python
- `Cargo.toml` → Rust
- `go.mod` → Go
- `Gemfile` → Ruby (no plugin — note as unsupported)
- `Dockerfile` or `docker-compose.yml` → Docker usage (built into base image)

**Services & Infrastructure:**
- `.github/` directory → GitHub (recommend `gh` plugin)
- `.gitlab-ci.yml` → GitLab (recommend `glab` plugin)
- AWS references (`aws`, `boto3`, `terraform` with aws provider, `.aws/`, `samconfig`, `serverless.yml`) → AWS (recommend `aws` plugin)
- `heroku.yml`, `Procfile`, `app.json` → Heroku (recommend `heroku` plugin)
- `.linear/`, linear references in configs → Linear (recommend `lin` plugin)
- `playwright.config.*`, `puppeteer` references → Browser automation (recommend `rodney` plugin)

**Environment & Config:**
- `.env.example`, `.env.sample`, or `.env.template` files → document required env vars
- `CLAUDE.md` files → note existing Claude Code instructions
- `docker-compose.yml` → document service dependencies

### 3. Determine SSH Key Need

- If ANY remote URL uses `git@...` format, SSH key is required
- If all remotes use HTTPS, SSH key is optional

### 4. Detect Node.js Version

When Node.js is detected:
- Check `.nvmrc`, `.node-version`, or `engines` in `package.json`
- If version starts with 24 → `node-24`
- If version starts with 22 → `node-22`
- Default to `node-20` if unspecified or version 20.x

### 5. Check for Environment Variables

Read any `.env.example`, `.env.sample`, `.env.template` files and list variables that will need to be set. Flag any that reference API keys or secrets (these will need to be passed into the container or set up inside it).

## Output Format

Produce a structured report with these sections:

### Summary
One paragraph describing the project: what it is, its tech stack, and how many repos are involved.

### Repos Detected
Table with columns: Directory | Remote URL | Branch | Transport (SSH/HTTPS)

### Recommended Plugins
Table with columns: Plugin | Reason
Only include plugins that are actually needed. Include dependency notes (e.g., heroku requires a node plugin).

### claudine init Command
Generate the exact `claudine init` command with all flags. Use this format:

```bash
claudine init <project-name> \
  --ssh-key ~/.ssh/<key> \
  --repo <url1> \
  --repo <url2> \
  --plugin <plugin1> \
  --plugin <plugin2>
```

Use a sensible project name derived from the folder name. Plugin order matters — dependencies must come before dependents (e.g., `node-20` before `heroku`).

If SSH is not needed, omit the `--ssh-key` flag.

### Environment Variables
List env vars from `.env.example` files that will need to be configured inside the container. Group by repo if multi-repo. Flag secrets with a warning.

### CLAUDE.md Recommendations
Suggest content for a project-level CLAUDE.md that should be placed in each repo inside the container. Include:
- Build/test/lint commands discovered from package.json scripts, Makefiles, pyproject.toml, Cargo.toml, etc.
- Key architectural notes (monorepo structure, service boundaries, database setup)
- Development workflow (docker compose commands, migration commands, test commands)
- Any existing CLAUDE.md content that should be preserved or extended

### Suggested New Plugins
If the project uses technologies that would benefit from a claudine plugin but none exists in the available list (e.g., Ruby, Java, .NET, Terraform, Kubernetes, etc.), list them here with:
- A proposed plugin name
- What it would install
- Why the project needs it

This helps identify gaps in claudine's plugin catalog.

### Warnings
Flag anything that might cause issues:
- Large repos that may take time to clone
- Submodules that need special handling
- Workspace/monorepo tools (nx, turborepo, lerna) that may need additional setup
- Docker-in-Docker needs (projects that run docker compose as part of development)
