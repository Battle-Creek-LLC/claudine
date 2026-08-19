use std::process::{Command, Stdio};

use crate::{config, docker, sources};

const GO_VERSION: &str = "1.26.3";

/// A built-in layer representing a Dockerfile snippet that can be layered
/// on top of the base claudine image.
pub struct Layer {
    pub name: &'static str,
    pub description: &'static str,
    /// Layer names that satisfy a dependency. At least ONE must be present.
    pub requires: &'static [&'static str],
    /// Build toolchain needed to compile this layer from source.
    /// The Dockerfile generator installs and removes the toolchain automatically.
    pub build_tool: Option<BuildTool>,
    pub dockerfile: String,
    /// Shell commands that should exit 0 when the layer is installed correctly.
    pub validate: &'static [&'static str],
    /// Directories to prepend to PATH at runtime for this layer.
    pub path: &'static [&'static str],
    /// Git URL whose working tree should be checked out on the host into
    /// `<config>/sources/<layer-name>/` before each build. The Dockerfile can
    /// then `COPY` from that staged directory. `None` for layers that do not
    /// need host-side source preparation.
    pub source_repo: Option<&'static str>,
    /// Optional git ref (branch, tag, or commit) to check out. Defaults to
    /// tracking the remote's default branch when `None`.
    pub source_ref: Option<&'static str>,
    /// A GitHub release artifact to fetch host-side (via `gh`) instead of
    /// cloning source. Downloaded into `<config>/sources/<layer-name>/` and
    /// staged into the build context like any other source, so the Dockerfile
    /// can `COPY` it. Mutually exclusive with `source_repo` in practice.
    pub release: Option<ReleaseAsset>,
}

/// A published GitHub release artifact to download host-side and install,
/// instead of building from a checkout. `gh` uses the host's authentication,
/// so private releases work without exposing a token to the Docker build.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ReleaseAsset {
    /// `owner/name` of the GitHub repository hosting the release.
    pub repo: &'static str,
    /// Release tag to download (e.g. `v0.2.0`).
    pub tag: &'static str,
    /// Glob selecting which asset(s) to download (e.g. `*.whl`).
    pub pattern: &'static str,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum BuildTool {
    Rust,
    Go,
}

/// A single upstream version pin extracted from a layer, used by tooling
/// (e.g. the `dctr` doctor skill) to compare against the latest available
/// upstream and decide whether the catalog is behind.
#[derive(serde::Serialize)]
pub struct Pin {
    /// Layer the pin belongs to.
    pub layer: &'static str,
    /// The thing being installed (crate name, repo basename, or `go`).
    pub tool: String,
    /// Where "latest" is published: `crates.io`, `github-release`,
    /// `github-source`, or `go.dev`.
    pub kind: &'static str,
    /// The pinned version/tag/ref (`<default-branch>` for unpinned source).
    pub version: String,
    /// The upstream identifier to query: crate name, `owner/repo`, or `go.dev`.
    pub source: String,
}

/// Normalize a GitHub clone URL (SSH or HTTPS) to an `owner/repo` slug.
/// Leaves non-GitHub URLs untouched.
fn gh_slug(url: &str) -> String {
    let u = url.trim_end_matches('/').trim_end_matches(".git");
    for prefix in ["git@github.com:", "https://github.com/", "http://github.com/"] {
        if let Some(rest) = u.strip_prefix(prefix) {
            return rest.to_string();
        }
    }
    u.to_string()
}

/// Extract every upstream version pin a layer carries. Covers the four pin
/// shapes in the catalog: crates.io (`ARG X_VERSION=` + `crate@${X_VERSION}`),
/// GitHub release assets, GitHub source checkouts (the `source_repo` field and
/// in-Dockerfile `git clone` / `cargo install --git` URLs), and the go.dev
/// download URL. Layers that always fetch latest at build (flyway, doctl) or
/// track apt (node) carry no pin and yield nothing.
fn extract_pins(layer: &Layer) -> Vec<Pin> {
    let mut pins = Vec::new();
    let df = &layer.dockerfile;

    // crates.io: pair `ARG <VAR>=<ver>` with `"<crate>@${<VAR>}"`.
    let tokens: Vec<String> = df
        .split_whitespace()
        .map(|t| t.trim_matches('"').to_string())
        .collect();
    let mut args: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    for pair in tokens.windows(2) {
        if pair[0] == "ARG" {
            if let Some((var, val)) = pair[1].split_once('=') {
                args.insert(var, val);
            }
        }
    }
    for tok in &tokens {
        if let Some(at) = tok.find("@${") {
            let krate = &tok[..at];
            if let Some(end) = tok[at + 3..].find('}') {
                let var = &tok[at + 3..at + 3 + end];
                if let Some(ver) = args.get(var) {
                    pins.push(Pin {
                        layer: layer.name,
                        tool: krate.to_string(),
                        kind: "crates.io",
                        version: (*ver).to_string(),
                        source: krate.to_string(),
                    });
                }
            }
        }
    }

    // go.dev: `https://go.dev/dl/go<ver>.linux-...`.
    if let Some(idx) = df.find("go.dev/dl/go") {
        let after = &df[idx + "go.dev/dl/go".len()..];
        let ver: String = after
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect::<String>()
            .trim_end_matches('.')
            .to_string();
        if !ver.is_empty() {
            pins.push(Pin {
                layer: layer.name,
                tool: "go".to_string(),
                kind: "go.dev",
                version: ver,
                source: "go.dev".to_string(),
            });
        }
    }

    // GitHub release asset.
    if let Some(rel) = &layer.release {
        pins.push(Pin {
            layer: layer.name,
            tool: rel.repo.rsplit('/').next().unwrap_or(rel.repo).to_string(),
            kind: "github-release",
            version: rel.tag.to_string(),
            source: rel.repo.to_string(),
        });
    }

    // GitHub source checkouts: the host-side `source_repo` field plus any
    // in-Dockerfile `git clone <url>` / `cargo install --git <url>`.
    let mut source_urls: Vec<String> = layer.source_repo.map(str::to_string).into_iter().collect();
    for marker in ["git clone ", "--git "] {
        let mut from = 0;
        while let Some(pos) = df[from..].find(marker) {
            let abs = from + pos + marker.len();
            // Skip any flags that precede the URL (e.g. `--depth 1 --branch v0.8.0`),
            // stopping at the end of the command so a later clone isn't attributed here.
            let url = df[abs..]
                .split_whitespace()
                .take_while(|t| *t != "&&")
                .find(|t| t.contains("github.com"));
            if let Some(url) = url {
                source_urls.push(url.to_string());
            }
            from = abs;
        }
    }
    for url in source_urls {
        let slug = gh_slug(&url);
        pins.push(Pin {
            layer: layer.name,
            tool: slug.rsplit('/').next().unwrap_or(&slug).to_string(),
            kind: "github-source",
            version: layer.source_ref.unwrap_or("<default-branch>").to_string(),
            source: slug,
        });
    }

    pins
}

/// Return the full catalog of built-in layers.
pub fn catalog() -> Vec<Layer> {
    vec![
        Layer {
            name: "node-22",
            description: "Node.js 22.x LTS",
            requires: &[],
            build_tool: None,
            dockerfile: "RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \\\n    && apt-get install -y nodejs \\\n    && rm -rf /var/lib/apt/lists/* \\\n    && corepack enable \\\n    && corepack prepare pnpm@latest --activate".to_string(),
            validate: &["node --version", "npm --version", "corepack --version", "pnpm --version"],
            path: &[],
            source_repo: None,
            source_ref: None,
            release: None,
        },
        Layer {
            name: "node-24",
            description: "Node.js 24.x",
            requires: &[],
            build_tool: None,
            dockerfile: "RUN curl -fsSL https://deb.nodesource.com/setup_24.x | bash - \\\n    && apt-get install -y nodejs \\\n    && rm -rf /var/lib/apt/lists/* \\\n    && corepack enable \\\n    && corepack prepare pnpm@latest --activate".to_string(),
            validate: &["node --version", "npm --version", "npx --version", "corepack --version", "pnpm --version"],
            path: &[],
            source_repo: None,
            source_ref: None,
            release: None,
        },
        Layer {
            name: "gh",
            description: "GitHub CLI",
            requires: &[],
            build_tool: None,
            dockerfile: "RUN curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg \\\n       | dd of=/usr/share/keyrings/githubcli-archive-keyring.gpg \\\n    && chmod go+r /usr/share/keyrings/githubcli-archive-keyring.gpg \\\n    && echo \"deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main\" \\\n       > /etc/apt/sources.list.d/github-cli.list \\\n    && apt-get update \\\n    && apt-get install -y --no-install-recommends gh \\\n    && rm -rf /var/lib/apt/lists/*".to_string(),
            validate: &["gh --version"],
            path: &[],
            source_repo: None,
            source_ref: None,
            release: None,
        },
        Layer {
            name: "heroku",
            description: "Heroku CLI",
            requires: &["node-22", "node-24"],
            build_tool: None,
            dockerfile: "RUN curl https://cli-assets.heroku.com/install.sh | sh".to_string(),
            validate: &["heroku --version"],
            path: &[],
            source_repo: None,
            source_ref: None,
            release: None,
        },
        Layer {
            name: "python-venv",
            description: "Python 3 virtual environment support",
            requires: &[],
            build_tool: None,
            dockerfile: "RUN PY_MINOR=$(python3 -c 'import sys; print(f\"{sys.version_info.major}.{sys.version_info.minor}\")') \\\n    && apt-get update && apt-get install -y python3-venv \"python${PY_MINOR}-venv\" \\\n    && rm -rf /var/lib/apt/lists/*".to_string(),
            validate: &["python3 -m venv /tmp/_venv_check && rm -rf /tmp/_venv_check"],
            path: &[],
            source_repo: None,
            source_ref: None,
            release: None,
        },
        Layer {
            name: "msodbc",
            description: "Microsoft ODBC Driver 18 for SQL Server",
            requires: &[],
            build_tool: None,
            dockerfile: "RUN apt-get update && apt-get install -y unixodbc curl gnupg2 \\\n    && { curl -fsSL https://packages.microsoft.com/keys/microsoft.asc; curl -fsSL https://packages.microsoft.com/keys/microsoft-2025.asc; } | gpg --dearmor -o /usr/share/keyrings/microsoft-prod.gpg \\\n    && echo \"deb [signed-by=/usr/share/keyrings/microsoft-prod.gpg] https://packages.microsoft.com/debian/13/prod trixie main\" > /etc/apt/sources.list.d/mssql-release.list \\\n    && apt-get update && ACCEPT_EULA=Y apt-get install -y msodbcsql18 \\\n    && rm -rf /var/lib/apt/lists/*".to_string(),
            validate: &["odbcinst -j"],
            path: &[],
            source_repo: None,
            source_ref: None,
            release: None,
        },
        Layer {
            name: "postgres",
            description: "PostgreSQL client (psql)",
            requires: &[],
            build_tool: None,
            dockerfile: "RUN apt-get update \\\n    && apt-get install -y --no-install-recommends postgresql-client \\\n    && rm -rf /var/lib/apt/lists/*".to_string(),
            validate: &["psql --version"],
            path: &[],
            source_repo: None,
            source_ref: None,
            release: None,
        },
        Layer {
            name: "go",
            description: "Go toolchain (persistent, available at runtime)",
            requires: &[],
            build_tool: None,
            dockerfile: format!(
                "RUN curl -fsSL https://go.dev/dl/go{ver}.linux-$(dpkg --print-architecture).tar.gz | tar -C /usr/local -xz \\\n    && chmod -R a+rwX /usr/local/go\nENV PATH=\"/usr/local/go/bin:${{PATH}}\"",
                ver = GO_VERSION
            ),
            validate: &["go version"],
            path: &["/usr/local/go/bin"],
            source_repo: None,
            source_ref: None,
            release: None,
        },
        Layer {
            name: "java",
            description: "OpenJDK 21 LTS runtime",
            requires: &[],
            build_tool: None,
            dockerfile: "RUN curl -fsSL https://packages.adoptium.net/artifactory/api/gpg/key/public | gpg --dearmor -o /usr/share/keyrings/adoptium.gpg \\\n    && echo \"deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/adoptium.gpg] https://packages.adoptium.net/artifactory/deb trixie main\" \\\n       > /etc/apt/sources.list.d/adoptium.list \\\n    && apt-get update \\\n    && apt-get install -y --no-install-recommends temurin-21-jre \\\n    && rm -rf /var/lib/apt/lists/*".to_string(),
            validate: &["java -version"],
            path: &[],
            source_repo: None,
            source_ref: None,
            release: None,
        },
        Layer {
            name: "flyway",
            description: "Flyway database migration CLI",
            requires: &["java"],
            build_tool: None,
            dockerfile: "RUN FLYWAY_VERSION=$(curl -fsSL https://api.github.com/repos/flyway/flyway/releases/latest | grep '\"tag_name\"' | sed 's/.*\"flyway-\\(.*\\)\".*/\\1/') \\\n    && curl -fsSL \"https://download.red-gate.com/maven/release/com/redgate/flyway/flyway-commandline/${FLYWAY_VERSION}/flyway-commandline-${FLYWAY_VERSION}.tar.gz\" | tar -C /opt -xz \\\n    && chmod +x /opt/flyway-${FLYWAY_VERSION}/flyway \\\n    && ln -s /opt/flyway-${FLYWAY_VERSION}/flyway /usr/local/bin/flyway".to_string(),
            validate: &["flyway --help"],
            path: &[],
            source_repo: None,
            source_ref: None,
            release: None,
        },
        Layer {
            name: "lin",
            description: "Fast CLI for Linear (built from source)",
            requires: &[],
            build_tool: Some(BuildTool::Rust),
            dockerfile: "RUN git clone --depth 1 --branch v0.8.0 https://github.com/sprouted-dev/lin.git /tmp/lin \\\n    && cd /tmp/lin \\\n    && cargo build --release \\\n    && cp target/release/lin /usr/local/bin/lin \\\n    && chmod 755 /usr/local/bin/lin \\\n    && rm -rf /tmp/lin /usr/local/cargo/registry /usr/local/cargo/git".to_string(),
            validate: &["lin --help"],
            path: &[],
            source_repo: None,
            source_ref: Some("v0.8.0"),
            release: None,
        },
        Layer {
            name: "exp",
            description: "Experiment tracker CLI",
            requires: &[],
            build_tool: None,
            dockerfile: "ARG EXP_VERSION=0.1.2\nRUN cargo binstall -y --root /usr/local \"exp@${EXP_VERSION}\"".to_string(),
            validate: &["exp --help"],
            path: &[],
            source_repo: None,
            source_ref: None,
            release: None,
        },
        Layer {
            name: "sumo",
            description: "Sumo Logic log query CLI",
            requires: &[],
            build_tool: None,
            dockerfile: "ARG SUMO_VERSION=0.1.4\nRUN cargo binstall -y --root /usr/local \"bcl-sumo@${SUMO_VERSION}\"".to_string(),
            validate: &["sumo --help"],
            path: &[],
            source_repo: None,
            source_ref: None,
            release: None,
        },
        Layer {
            name: "sntry",
            description: "Sentry read-side CLI",
            requires: &[],
            build_tool: None,
            dockerfile: "ARG SNTRY_VERSION=0.3.0\nRUN cargo binstall -y --root /usr/local \"bcl-sntry@${SNTRY_VERSION}\"".to_string(),
            validate: &["sntry --help"],
            path: &[],
            source_repo: None,
            source_ref: None,
            release: None,
        },
        Layer {
            name: "secops",
            description: "Security ops CLIs: secunit (WISP control registry) + repocat (GitHub/GitLab repo hardening)",
            requires: &[],
            build_tool: None,
            dockerfile: "ARG SECUNIT_VERSION=0.6.0\n\
                ARG REPOCAT_VERSION=0.5.0\n\
                RUN cargo binstall -y --root /usr/local \"bcl-secunit@${SECUNIT_VERSION}\"\n\
                RUN cargo binstall -y --disable-strategies compile --root /usr/local \"bcl-repocat@${REPOCAT_VERSION}\"".to_string(),
            validate: &["secunit --help", "repocat --help"],
            path: &[],
            source_repo: None,
            source_ref: None,
            release: None,
        },
        Layer {
            name: "ddog",
            description: "Datadog logs CLI",
            requires: &[],
            build_tool: None,
            dockerfile: "ARG DDOG_VERSION=0.4.0\nRUN cargo binstall -y --root /usr/local \"bcl-ddog@${DDOG_VERSION}\"".to_string(),
            validate: &["ddog --help"],
            path: &[],
            source_repo: None,
            source_ref: None,
            release: None,
        },
        Layer {
            name: "brdg",
            description: "brdg CLI (Battle-Creek-LLC, Python, release wheel installed into an isolated venv)",
            requires: &[],
            build_tool: None,
            // Install into a dedicated venv rather than `pip install
            // --break-system-packages` into the base image's PEP 668
            // externally-managed interpreter: isolates brdg's dependency tree
            // (typer/rich/httpx/keyring -> cryptography) from the system
            // packages and from other layers, and the symlink keeps `brdg` on
            // PATH. Needs `python3-venv` in the base image (provides ensurepip).
            dockerfile: "COPY brdg /tmp/brdg\n\
                RUN python3 -m venv /opt/brdg \\\n\
                    && /opt/brdg/bin/pip install --no-cache-dir /tmp/brdg/*.whl \\\n\
                    && ln -sf /opt/brdg/bin/brdg /usr/local/bin/brdg \\\n\
                    && rm -rf /tmp/brdg".to_string(),
            validate: &["brdg --help"],
            path: &[],
            source_repo: None,
            source_ref: None,
            release: Some(ReleaseAsset {
                repo: "Battle-Creek-LLC/brdg",
                tag: "v0.4.0",
                pattern: "*.whl",
            }),
        },
        Layer {
            name: "terra",
            description: "Terra sprout CLI (sprout), built from a host-side checkout",
            requires: &[],
            build_tool: Some(BuildTool::Rust),
            dockerfile: "COPY terra /tmp/terra\n\
                RUN apt-get update \\\n\
                    && apt-get install -y --no-install-recommends protobuf-compiler libprotobuf-dev \\\n\
                    && cd /tmp/terra \\\n\
                    && cargo install --path sprout --root /usr/local \\\n\
                    && cargo install --git https://github.com/sprouted-dev/guild.git --root /usr/local \\\n\
                    && rm -rf /var/lib/apt/lists/* /tmp/terra /usr/local/cargo/registry /usr/local/cargo/git \\\n\
                    && mkdir -p /opt/terra-defaults \\\n\
                    && printf '[endpoints]\\nsunlight = \"http://host.docker.internal:50061\"\\n' > /opt/terra-defaults/services.toml \\\n\
                    && printf 'default_agent: claude\\n\\nagents:\\n  claude:\\n    command: \"npx\"\\n    args: [\"@zed-industries/claude-agent-acp\"]\\n    protocol: acp\\n    models:\\n      default: opus\\n      available: [sonnet, opus, haiku]\\n    description: \"Claude Code via ACP adapter\"\\n\\ninstalled:\\n  - claude\\n\\ndefaults:\\n  agent: claude\\n  model: opus\\n\\nby_type:\\n  enrichment:\\n    model: haiku\\n  planning:\\n    model: opus\\n' > /opt/terra-defaults/agents.yaml\n\
                ENV TERRA_HOME=/home/claude/.terra".to_string(),
            validate: &["sprout --help", "guild --help"],
            path: &[],
            source_repo: Some("git@github.com:sprouted-dev/terra.git"),
            source_ref: None,
            release: None,
        },
        Layer {
            name: "glab",
            description: "GitLab CLI (built from source, jstockdi fork)",
            requires: &[],
            build_tool: Some(BuildTool::Go),
            dockerfile: "RUN git clone https://github.com/jstockdi/glab.git /tmp/glab \\\n    && cd /tmp/glab \\\n    && make build \\\n    && cp bin/glab /usr/local/bin/glab \\\n    && chmod 755 /usr/local/bin/glab \\\n    && rm -rf /tmp/glab /root/go /root/.cache/go-build".to_string(),
            validate: &["glab version"],
            path: &[],
            source_repo: None,
            source_ref: None,
            release: None,
        },
        Layer {
            name: "aws",
            description: "AWS CLI v2",
            requires: &[],
            build_tool: None,
            dockerfile: "RUN curl -fsSL \"https://awscli.amazonaws.com/awscli-exe-linux-$(uname -m).zip\" -o /tmp/awscliv2.zip \\\n    && unzip -q /tmp/awscliv2.zip -d /tmp \\\n    && /tmp/aws/install \\\n    && rm -rf /tmp/awscliv2.zip /tmp/aws".to_string(),
            validate: &["aws --version"],
            path: &[],
            source_repo: None,
            source_ref: None,
            release: None,
        },
        Layer {
            name: "gcloud",
            description: "Google Cloud SDK (gcloud)",
            requires: &[],
            build_tool: None,
            dockerfile: "RUN curl -fsSL https://packages.cloud.google.com/apt/doc/apt-key.gpg | gpg --dearmor -o /usr/share/keyrings/cloud.google.gpg \\\n    && echo \"deb [signed-by=/usr/share/keyrings/cloud.google.gpg] https://packages.cloud.google.com/apt cloud-sdk main\" \\\n       > /etc/apt/sources.list.d/google-cloud-sdk.list \\\n    && apt-get update \\\n    && apt-get install -y --no-install-recommends google-cloud-cli \\\n    && rm -rf /var/lib/apt/lists/*".to_string(),
            validate: &["gcloud --version"],
            path: &[],
            source_repo: None,
            source_ref: None,
            release: None,
        },
        Layer {
            name: "terraform",
            description: "Terraform CLI for infrastructure provisioning",
            requires: &[],
            build_tool: None,
            dockerfile: "RUN curl -fsSL https://apt.releases.hashicorp.com/gpg | gpg --dearmor -o /usr/share/keyrings/hashicorp-archive-keyring.gpg \\\n    && echo \"deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/hashicorp-archive-keyring.gpg] https://apt.releases.hashicorp.com trixie main\" \\\n       > /etc/apt/sources.list.d/hashicorp.list \\\n    && apt-get update \\\n    && apt-get install -y --no-install-recommends terraform \\\n    && rm -rf /var/lib/apt/lists/*".to_string(),
            validate: &["terraform version"],
            path: &[],
            source_repo: None,
            source_ref: None,
            release: None,
        },
        Layer {
            name: "doctl",
            description: "DigitalOcean CLI",
            requires: &[],
            build_tool: None,
            dockerfile: "RUN DOCTL_VERSION=$(curl -fsSL https://api.github.com/repos/digitalocean/doctl/releases/latest | grep '\"tag_name\"' | sed 's/.*\"v\\(.*\\)\".*/\\1/') \\\n    && curl -fsSL \"https://github.com/digitalocean/doctl/releases/download/v${DOCTL_VERSION}/doctl-${DOCTL_VERSION}-linux-$(dpkg --print-architecture).tar.gz\" | tar -C /usr/local/bin -xz \\\n    && chmod 755 /usr/local/bin/doctl".to_string(),
            validate: &["doctl version"],
            path: &[],
            source_repo: None,
            source_ref: None,
            release: None,
        },
        Layer {
            name: "rodney",
            description: "Chrome automation CLI (built from source, jstockdi fork)",
            requires: &[],
            build_tool: Some(BuildTool::Go),
            dockerfile: "RUN apt-get update && apt-get install -y --no-install-recommends chromium \\\n    && rm -rf /var/lib/apt/lists/* \\\n    && git clone https://github.com/jstockdi/rodney.git /tmp/rodney \\\n    && cd /tmp/rodney \\\n    && go build -o /usr/local/bin/rodney . \\\n    && chmod 755 /usr/local/bin/rodney \\\n    && rm -rf /tmp/rodney /root/go /root/.cache/go-build".to_string(),
            validate: &["chromium --version", "rodney --help"],
            path: &[],
            source_repo: None,
            source_ref: None,
            release: None,
        },
    ]
}

const BASE_PATH: &[&str] = &[
    "/usr/local/cargo/bin",
    "/usr/local/sbin",
    "/usr/local/bin",
    "/usr/sbin",
    "/usr/bin",
    "/sbin",
    "/bin",
];

/// Compute the full PATH for a project based on its installed layers.
///
/// Prepends `/home/claude/.local/bin` and any layer-specific paths (in catalog
/// order) before the standard system PATH.
pub fn compute_path(layers: &[String]) -> String {
    let cat = catalog();
    let mut entries: Vec<&str> = vec!["/home/claude/.local/bin"];

    for layer in &cat {
        if layers.iter().any(|n| n == layer.name) {
            entries.extend_from_slice(layer.path);
        }
    }

    entries.extend_from_slice(BASE_PATH);
    entries.join(":")
}

/// Look up a layer by name in the catalog.
pub fn find(name: &str) -> Option<Layer> {
    catalog().into_iter().find(|p| p.name == name)
}

/// Check that the dependency requirements for a layer are satisfied.
///
/// For layers with a non-empty `requires` list, at least one of the listed
/// layers must already be present in `installed`.
pub fn check_requires(name: &str, installed: &[String]) -> anyhow::Result<()> {
    let layer = find(name)
        .ok_or_else(|| anyhow::anyhow!("Unknown layer '{}'.", name))?;

    if layer.requires.is_empty() {
        return Ok(());
    }

    let satisfied = layer
        .requires
        .iter()
        .any(|req| installed.iter().any(|i| i == req));

    if !satisfied {
        let options = layer.requires.join(", ");
        anyhow::bail!(
            "Layer '{}' requires one of: {}. Install one first: claudine layer add <project> {}",
            name,
            options,
            layer.requires[0],
        );
    }

    Ok(())
}

/// Generate a Dockerfile from a list of layer names.
///
/// Layers are ordered according to their position in the catalog, regardless
/// of the order they were installed. This ensures deterministic builds.
pub fn generate_dockerfile(layers: &[String]) -> anyhow::Result<String> {
    let cat = catalog();

    // Collect layers in catalog order
    let ordered: Vec<&Layer> = cat
        .iter()
        .filter(|p| layers.iter().any(|name| name == p.name))
        .collect();

    // Verify all requested layers exist
    for name in layers {
        if !cat.iter().any(|p| p.name == name) {
            anyhow::bail!("Unknown layer '{}'.", name);
        }
    }

    // Rust toolchain ships in the base image; Go is still installed on demand.
    let needs_go = ordered.iter().any(|p| p.build_tool == Some(BuildTool::Go))
        && !layers.iter().any(|n| n == "go");

    let mut lines = vec!["FROM claudine:latest".to_string()];

    // Non-compiled layers first
    for layer in ordered.iter().filter(|p| p.build_tool.is_none()) {
        lines.push(String::new());
        lines.push(format!("# Layer: {}", layer.name));
        lines.push(layer.dockerfile.to_string());
    }

    // Install Go toolchain temporarily if needed
    if needs_go {
        lines.push(String::new());
        lines.push("# Build phase: install Go toolchain".to_string());
        lines.push(format!("RUN curl -fsSL https://go.dev/dl/go{GO_VERSION}.linux-$(dpkg --print-architecture).tar.gz | tar -C /usr/local -xz"));
    }

    // Compiled layers (Rust first, then Go — catalog order)
    let compiled: Vec<_> = ordered.iter().filter(|p| p.build_tool.is_some()).collect();
    for layer in &compiled {
        lines.push(String::new());
        lines.push(format!("# Layer: {}", layer.name));
        // Compiled layers need PATH set for their build toolchain
        if layer.build_tool == Some(BuildTool::Go) {
            let dockerfile = layer.dockerfile.replacen("RUN ", "RUN export PATH=$PATH:/usr/local/go/bin && ", 1);
            lines.push(dockerfile);
        } else if layer.build_tool == Some(BuildTool::Rust) {
            let dockerfile = layer.dockerfile.replacen("RUN ", "RUN export PATH=$PATH:/usr/local/cargo/bin && ", 1);
            lines.push(dockerfile);
        } else {
            lines.push(layer.dockerfile.to_string());
        }
    }

    // Clean up Go toolchain (rust stays — it's in the base)
    if needs_go {
        lines.push(String::new());
        lines.push("# Cleanup: remove temporary Go toolchain".to_string());
        lines.push("RUN rm -rf /usr/local/go".to_string());
    }

    // Trailing newline
    lines.push(String::new());

    Ok(lines.join("\n"))
}

/// Rebuild all project images that have layers installed.
pub fn cmd_build_all(no_cache: bool) -> anyhow::Result<()> {
    let projects = config::list_projects()?;
    let mut failures: Vec<String> = Vec::new();

    for name in &projects {
        let project_config = match config::load_project(name) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let has_layers = project_config
            .layers
            .as_ref()
            .map(|l| !l.is_empty())
            .unwrap_or(false);

        if !has_layers {
            continue;
        }

        println!("=== {} ===", name);
        match cmd_build_project(name, no_cache) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("Error: {}", e);
                failures.push(name.clone());
            }
        }
        println!();
    }

    if failures.is_empty() {
        println!("All project images rebuilt.");
        Ok(())
    } else {
        anyhow::bail!("{} project(s) failed: {}", failures.len(), failures.join(", "))
    }
}

/// Rebuild a project's layer image from its current config.
pub fn cmd_build_project(project: &str, no_cache: bool) -> anyhow::Result<()> {
    let project_config = config::load_project(project)?;

    let layers = project_config
        .layers
        .as_ref()
        .filter(|p| !p.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Project '{}' has no layers installed.", project))?;

    ensure_sources_for(layers)?;

    let dockerfile = generate_dockerfile(layers)?;
    docker::cmd_build_project(project, &dockerfile, no_cache)?;

    let image = format!("claudine:{}", project);
    validate_image(&image, layers)?;

    println!("Project '{}' image rebuilt.", project);
    Ok(())
}

/// Refresh the host-side source checkout for every layer in `layers` that
/// declares a `source_repo`.
fn ensure_sources_for(layers: &[String]) -> anyhow::Result<()> {
    for name in layers {
        if let Some(layer) = find(name) {
            sources::ensure_source(&layer)?;
        }
    }
    Ok(())
}

/// Add a layer to a project.
///
/// Validates the layer exists, checks dependency requirements, updates the
/// project config, generates a new Dockerfile, and builds the project image.
pub fn cmd_layer_add(project: &str, layer: &str) -> anyhow::Result<()> {
    // Validate layer exists
    if find(layer).is_none() {
        anyhow::bail!(
            "Unknown layer '{}'. Run 'claudine layer available' to see options.",
            layer,
        );
    }

    let mut project_config = config::load_project(project)?;
    let layers = project_config.layers.get_or_insert_with(Vec::new);

    // Check if already installed
    if layers.iter().any(|p| p == layer) {
        println!("Layer '{}' is already installed in project '{}'.", layer, project);
        return Ok(());
    }

    // Check dependency requirements
    check_requires(layer, layers)?;

    // Add the layer
    layers.push(layer.to_string());

    // Set the project-specific image
    project_config.image = Some(config::ImageConfig {
        name: format!("claudine:{}", project),
    });

    config::save_project(project, &project_config)?;

    // Generate Dockerfile and build
    let layers = project_config.layers.as_ref().unwrap();
    ensure_sources_for(layers)?;
    let dockerfile = generate_dockerfile(layers)?;
    docker::cmd_build_project(project, &dockerfile, false)?;

    let image = format!("claudine:{}", project);
    validate_image(&image, layers)?;

    println!("Layer '{}' added to project '{}'.", layer, project);
    Ok(())
}

/// Remove a layer from a project.
///
/// Updates the project config and rebuilds the project image. If no layers
/// remain, reverts the image to `claudine:latest`.
pub fn cmd_layer_remove(project: &str, layer: &str) -> anyhow::Result<()> {
    let mut project_config = config::load_project(project)?;

    {
        let layers = project_config.layers.get_or_insert_with(Vec::new);

        let index = layers
            .iter()
            .position(|p| p == layer)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Layer '{}' is not installed in project '{}'.",
                    layer,
                    project,
                )
            })?;

        layers.remove(index);
    }

    let remaining = project_config
        .layers
        .as_ref()
        .map(|p| p.is_empty())
        .unwrap_or(true);

    if remaining {
        // Revert to base image
        project_config.layers = None;
        project_config.image = None;
        config::save_project(project, &project_config)?;
        println!("No layers remaining. Image reverted to claudine:latest.");
    } else {
        project_config.image = Some(config::ImageConfig {
            name: format!("claudine:{}", project),
        });
        config::save_project(project, &project_config)?;

        let layers = project_config.layers.as_ref().unwrap();
        ensure_sources_for(layers)?;
        let dockerfile = generate_dockerfile(layers)?;
        docker::cmd_build_project(project, &dockerfile, false)?;

        let image = format!("claudine:{}", project);
        validate_image(&image, layers)?;
    }

    println!("Layer '{}' removed from project '{}'.", layer, project);
    Ok(())
}

/// List layers installed in a project.
pub fn cmd_layer_list(project: &str) -> anyhow::Result<()> {
    let project_config = config::load_project(project)?;

    match &project_config.layers {
        Some(layers) if !layers.is_empty() => {
            println!("Layers for project '{}':", project);
            for name in layers {
                if let Some(p) = find(name) {
                    println!("  {} - {}", p.name, p.description);
                } else {
                    println!("  {} (unknown)", name);
                }
            }
        }
        _ => {
            println!("No layers installed for project '{}'.", project);
        }
    }

    Ok(())
}

/// List all available layers in the catalog.
pub fn cmd_layer_available() -> anyhow::Result<()> {
    let cat = catalog();

    println!("Available layers:");
    for layer in &cat {
        let deps = if layer.requires.is_empty() {
            String::new()
        } else {
            format!(" (requires one of: {})", layer.requires.join(", "))
        };
        println!("  {:<15} {}{}", layer.name, layer.description, deps);
    }

    Ok(())
}

/// Print every layer's pinned upstream version, for `dctr`-style "is the
/// catalog behind upstream?" checks. Text table by default; `--json` emits an
/// array of `Pin` objects.
pub fn cmd_layer_pins(json: bool) -> anyhow::Result<()> {
    let mut pins: Vec<Pin> = catalog().iter().flat_map(extract_pins).collect();
    pins.sort_by(|a, b| (a.layer, &a.source).cmp(&(b.layer, &b.source)));
    pins.dedup_by(|a, b| a.layer == b.layer && a.source == b.source);

    if json {
        println!("{}", serde_json::to_string_pretty(&pins)?);
    } else {
        println!(
            "{:<10} {:<14} {:<16} {:<18} {}",
            "LAYER", "TOOL", "KIND", "PINNED", "SOURCE"
        );
        for p in &pins {
            println!(
                "{:<10} {:<14} {:<16} {:<18} {}",
                p.layer, p.tool, p.kind, p.version, p.source
            );
        }
    }

    Ok(())
}

/// Collect the minimal set of layers needed to validate a given layer.
///
/// Includes the target layer plus any required dependencies (picking the
/// first option from `requires` recursively).
fn collect_validation_layers(name: &str) -> anyhow::Result<Vec<String>> {
    let layer = find(name)
        .ok_or_else(|| anyhow::anyhow!("Unknown layer '{}'.", name))?;

    let mut layers = Vec::new();

    if !layer.requires.is_empty() {
        let dep = layer.requires[0];
        let dep_layers = collect_validation_layers(dep)?;
        layers.extend(dep_layers);
    }

    layers.push(name.to_string());
    Ok(layers)
}

/// Build a temporary Docker image and return its tag.
fn build_validation_image(tag: &str, dockerfile: &str) -> anyhow::Result<()> {
    docker::check_docker()?;

    let tmp = tempfile::tempdir()?;
    std::fs::write(tmp.path().join("Dockerfile"), dockerfile)?;
    sources::stage_sources(tmp.path())?;

    let output = Command::new("docker")
        .args(["build", "-t", tag])
        .arg(tmp.path())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run 'docker build': {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to build validation image:\n{}", stderr);
    }

    Ok(())
}

/// Remove a Docker image, ignoring errors.
fn remove_image(tag: &str) {
    let _ = Command::new("docker")
        .args(["rmi", tag])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Run validation commands for the given layers against an existing Docker image.
///
/// Returns Ok if all checks pass, Err listing the failures otherwise.
fn validate_image(image: &str, layer_names: &[String]) -> anyhow::Result<()> {
    println!("Validating layers...");

    let mut total_passed = 0;
    let mut total_failed = 0;
    let mut failed_layers: Vec<String> = Vec::new();

    for name in layer_names {
        let layer = match find(name) {
            Some(l) => l,
            None => continue,
        };

        if layer.validate.is_empty() {
            continue;
        }

        let mut layer_ok = true;
        for cmd in layer.validate {
            let status = Command::new("docker")
                .args([
                    "run", "--rm",
                    "--entrypoint", "bash",
                    "-e", "HOME=/tmp",
                    image, "-c", cmd,
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map_err(|e| anyhow::anyhow!("Failed to run 'docker run': {e}"))?;

            if status.success() {
                println!("  PASS  {} — {}", name, cmd);
                total_passed += 1;
            } else {
                println!("  FAIL  {} — {}", name, cmd);
                total_failed += 1;
                layer_ok = false;
            }
        }

        if !layer_ok {
            failed_layers.push(name.clone());
        }
    }

    if total_failed > 0 {
        anyhow::bail!(
            "{} check(s) failed across layer(s): {}",
            total_failed,
            failed_layers.join(", "),
        );
    }

    println!("All {} checks passed.", total_passed);
    Ok(())
}

/// Validate a single layer by building a temporary image and running its checks.
pub fn cmd_layer_validate(name: &str) -> anyhow::Result<()> {
    let _layer = find(name)
        .ok_or_else(|| anyhow::anyhow!("Unknown layer '{}'.", name))?;

    let layers = collect_validation_layers(name)?;
    ensure_sources_for(&layers)?;
    let dockerfile = generate_dockerfile(&layers)?;
    let tag = format!("claudine:validate-{}", name);

    println!("Building validation image ({})...", layers.join(", "));
    build_validation_image(&tag, &dockerfile)?;

    let result = validate_image(&tag, &layers);
    remove_image(&tag);
    result
}

/// Validate all layers in the catalog (standalone builds).
pub fn cmd_layer_validate_all() -> anyhow::Result<()> {
    let cat = catalog();
    let mut failures: Vec<String> = Vec::new();

    for layer in &cat {
        match cmd_layer_validate(layer.name) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("  {}", e);
                failures.push(layer.name.to_string());
            }
        }
        println!();
    }

    if failures.is_empty() {
        println!("All {} layers validated.", cat.len());
        Ok(())
    } else {
        anyhow::bail!("{} layer(s) failed validation: {}", failures.len(), failures.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_existing_layer() {
        assert!(find("node-22").is_some());
        assert!(find("heroku").is_some());
        assert!(find("go").is_some());
    }

    #[test]
    fn find_unknown_layer() {
        assert!(find("does-not-exist").is_none());
    }

    /// Helper: collect every pin in the catalog keyed for assertions.
    fn all_pins() -> Vec<Pin> {
        catalog().iter().flat_map(extract_pins).collect()
    }

    #[test]
    fn pins_cratesio_arg_pairing() {
        // secops pairs two ARG vars with two crate refs.
        let pins = all_pins();
        let secunit = pins
            .iter()
            .find(|p| p.source == "bcl-secunit")
            .expect("bcl-secunit pin present");
        assert_eq!(secunit.kind, "crates.io");
        assert_eq!(secunit.version, "0.6.0");
        assert_eq!(secunit.layer, "secops");
        let repocat = pins.iter().find(|p| p.source == "bcl-repocat").unwrap();
        assert_eq!(repocat.version, "0.5.0");
    }

    #[test]
    fn pins_go_version_has_no_trailing_dot() {
        let pins = all_pins();
        let go = pins.iter().find(|p| p.layer == "go").unwrap();
        assert_eq!(go.kind, "go.dev");
        assert_eq!(go.version, GO_VERSION);
        assert!(!go.version.ends_with('.'));
    }

    #[test]
    fn pins_github_release_and_source() {
        let pins = all_pins();
        let brdg = pins.iter().find(|p| p.layer == "brdg").unwrap();
        assert_eq!(brdg.kind, "github-release");
        assert_eq!(brdg.source, "Battle-Creek-LLC/brdg");
        assert_eq!(brdg.version, "v0.4.0");

        // lin clones github in-Dockerfile (no source_repo field) — still caught.
        let lin = pins.iter().find(|p| p.layer == "lin").unwrap();
        assert_eq!(lin.kind, "github-source");
        assert_eq!(lin.source, "sprouted-dev/lin");
        // Tag pin survives the `--depth 1 --branch <tag>` flags before the URL.
        assert_eq!(lin.version, "v0.8.0");

        // terra uses the source_repo field (SSH URL) — normalized to a slug.
        let terra = pins.iter().find(|p| p.source == "sprouted-dev/terra").unwrap();
        assert_eq!(terra.kind, "github-source");
    }

    #[test]
    fn pins_skip_dynamic_and_apt_layers() {
        let pins = all_pins();
        // flyway/doctl fetch latest at build; node uses apt — none are pinned.
        for layer in ["flyway", "doctl", "node-22", "node-24"] {
            assert!(
                !pins.iter().any(|p| p.layer == layer),
                "{layer} should carry no pin"
            );
        }
    }

    #[test]
    fn check_requires_no_deps() {
        let installed = vec![];
        assert!(check_requires("node-22", &installed).is_ok());
        assert!(check_requires("python-venv", &installed).is_ok());
    }

    #[test]
    fn check_requires_satisfied() {
        let installed = vec!["node-22".to_string()];
        assert!(check_requires("heroku", &installed).is_ok());
    }

    #[test]
    fn check_requires_satisfied_alt() {
        let installed = vec!["node-24".to_string()];
        assert!(check_requires("heroku", &installed).is_ok());
    }

    #[test]
    fn check_requires_not_satisfied() {
        let installed = vec!["python-venv".to_string()];
        let result = check_requires("heroku", &installed);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("requires one of"));
        assert!(msg.contains("node-22"));
    }

    #[test]
    fn generate_dockerfile_single() {
        let layers = vec!["node-22".to_string()];
        let result = generate_dockerfile(&layers).unwrap();
        assert!(result.starts_with("FROM claudine:latest"));
        assert!(result.contains("# Layer: node-22"));
        assert!(result.contains("setup_22.x"));
    }

    #[test]
    fn generate_dockerfile_multiple_ordered() {
        // Install heroku first, node-22 second — output should be node-22 first (catalog order)
        let layers = vec!["heroku".to_string(), "node-22".to_string()];
        let result = generate_dockerfile(&layers).unwrap();
        let node_pos = result.find("# Layer: node-22").unwrap();
        let heroku_pos = result.find("# Layer: heroku").unwrap();
        assert!(
            node_pos < heroku_pos,
            "node-22 should appear before heroku in the Dockerfile"
        );
    }

    #[test]
    fn generate_dockerfile_unknown() {
        let layers = vec!["nonexistent".to_string()];
        assert!(generate_dockerfile(&layers).is_err());
    }

    #[test]
    fn generate_dockerfile_empty() {
        let layers: Vec<String> = vec![];
        let result = generate_dockerfile(&layers).unwrap();
        assert!(result.starts_with("FROM claudine:latest"));
        // Should just be the FROM line and a trailing newline
        assert!(!result.contains("# Layer:"));
    }

    #[test]
    fn catalog_has_expected_layers() {
        let cat = catalog();
        let names: Vec<&str> = cat.iter().map(|p| p.name).collect();
        assert!(names.contains(&"node-22"));
        assert!(names.contains(&"node-24"));
        assert!(names.contains(&"heroku"));
        assert!(names.contains(&"python-venv"));
        assert!(names.contains(&"go"));
        assert!(names.contains(&"postgres"));
        assert!(names.contains(&"aws"));
        assert!(names.contains(&"gcloud"));
        assert!(names.contains(&"java"));
        assert!(names.contains(&"flyway"));
        assert!(names.contains(&"exp"));
        assert!(names.contains(&"sumo"));
        assert!(names.contains(&"sntry"));
        assert!(names.contains(&"secops"));
        assert!(!names.contains(&"secunit"));
        assert!(!names.contains(&"node-20"));
        assert!(names.contains(&"ddog"));
        assert!(names.contains(&"brdg"));
        assert!(names.contains(&"terraform"));
        assert!(names.contains(&"doctl"));
    }

    #[test]
    fn rust_layer_is_no_longer_a_layer() {
        // rust ships in the base image now — it must not be selectable as a layer
        assert!(find("rust").is_none());
        assert!(generate_dockerfile(&vec!["rust".to_string()]).is_err());
    }

    #[test]
    fn compiled_rust_layer_skips_build_toolchain_install() {
        // A compiled-from-Rust layer (e.g. `exp`) must not trigger rustup install
        // since the base already has cargo on PATH.
        let layers = vec!["exp".to_string()];
        let result = generate_dockerfile(&layers).unwrap();
        assert!(!result.contains("sh.rustup.rs"));
        assert!(!result.contains("Build phase: install build toolchains"));
        assert!(result.contains("# Layer: exp"));
    }

    #[test]
    fn terra_layer_preserves_copy_and_rewrites_run() {
        let layers = vec!["terra".to_string()];
        let result = generate_dockerfile(&layers).unwrap();
        assert!(result.contains("COPY terra /tmp/terra"));
        // The cargo bin path should be injected into the first RUN (apt-get).
        assert!(
            result.contains("RUN export PATH=$PATH:/usr/local/cargo/bin && apt-get update"),
            "expected cargo PATH to be injected into terra's first RUN, got:\n{}",
            result,
        );
        // The RUN must still include the cargo install step later on.
        assert!(result.contains("cargo install --path sprout --root /usr/local"));
        // Guild CLI must be installed alongside sp from the sprouted-dev repo.
        assert!(result.contains("cargo install --git https://github.com/sprouted-dev/guild.git --root /usr/local"));
        // services.toml default must be baked with the host.docker.internal endpoint
        // into a build-time location that setup-home.sh seeds into the user's home.
        assert!(result.contains("host.docker.internal:50061"));
        assert!(result.contains("/opt/terra-defaults/services.toml"));
        assert!(result.contains("/opt/terra-defaults/agents.yaml"));
        assert!(result.contains("default_agent: claude"));
        assert!(result.contains("ENV TERRA_HOME=/home/claude/.terra"));
        assert!(
            !result.contains("/etc/terra"),
            "terra config must live under the user's home, not /etc/terra"
        );
        // protobuf-compiler must be installed and kept available at runtime so
        // terra can be rebuilt inside the container from a live checkout.
        assert!(result.contains("apt-get install -y --no-install-recommends protobuf-compiler"));
        assert!(
            !result.contains("apt-get purge -y --auto-remove protobuf-compiler"),
            "terra layer must NOT purge protobuf-compiler — it is needed at runtime for rebuilding sp"
        );

        let copy_pos = result.find("COPY terra /tmp/terra").unwrap();
        let run_pos = result
            .find("RUN export PATH=$PATH:/usr/local/cargo/bin && apt-get update")
            .unwrap();
        assert!(copy_pos < run_pos, "COPY must precede the RUN");
    }

    #[test]
    fn brdg_layer_installs_from_release_wheel() {
        let brdg = find("brdg").unwrap();
        assert!(brdg.source_repo.is_none(), "brdg installs from a release, not a checkout");
        let release = brdg.release.expect("brdg should declare a release asset");
        assert_eq!(release.repo, "Battle-Creek-LLC/brdg");
        assert_eq!(release.pattern, "*.whl");

        let df = generate_dockerfile(&vec!["brdg".to_string()]).unwrap();
        assert!(df.contains("COPY brdg /tmp/brdg"));
        assert!(
            df.contains("python3 -m venv /opt/brdg")
                && df.contains("/opt/brdg/bin/pip install --no-cache-dir /tmp/brdg/*.whl")
                && df.contains("ln -sf /opt/brdg/bin/brdg /usr/local/bin/brdg"),
            "brdg must install the staged wheel into an isolated venv and symlink it onto PATH, got:\n{}",
            df,
        );
        assert!(
            !df.contains("--break-system-packages"),
            "brdg must not pollute the system interpreter with --break-system-packages, got:\n{}",
            df,
        );
    }

    #[test]
    fn terra_layer_declares_source_repo() {
        let terra = find("terra").unwrap();
        assert_eq!(
            terra.source_repo,
            Some("git@github.com:sprouted-dev/terra.git")
        );
        assert_eq!(terra.build_tool, Some(BuildTool::Rust));
    }

    #[test]
    fn go_layer_skips_build_toolchain() {
        let layers = vec!["go".to_string(), "rodney".to_string()];
        let result = generate_dockerfile(&layers).unwrap();
        assert!(!result.contains("Build phase: install build toolchains"));
        assert!(!result.contains("Cleanup: remove build toolchains"));
        assert!(result.contains("# Layer: go"));
        assert!(result.contains("# Layer: rodney"));
    }

    #[test]
    fn heroku_requires_node() {
        let heroku = find("heroku").unwrap();
        assert!(!heroku.requires.is_empty());
        assert!(heroku.requires.contains(&"node-22"));
        assert!(heroku.requires.contains(&"node-24"));
    }

    #[test]
    fn flyway_requires_java() {
        let flyway = find("flyway").unwrap();
        assert!(flyway.requires.contains(&"java"));

        let installed = vec![];
        assert!(check_requires("flyway", &installed).is_err());

        let installed = vec!["java".to_string()];
        assert!(check_requires("flyway", &installed).is_ok());
    }

    #[test]
    fn all_layers_have_validate_commands() {
        for layer in catalog() {
            assert!(
                !layer.validate.is_empty(),
                "Layer '{}' has no validation commands",
                layer.name,
            );
        }
    }

    #[test]
    fn collect_validation_layers_no_deps() {
        let layers = collect_validation_layers("go").unwrap();
        assert_eq!(layers, vec!["go"]);
    }

    #[test]
    fn collect_validation_layers_with_deps() {
        let layers = collect_validation_layers("heroku").unwrap();
        assert_eq!(layers, vec!["node-22", "heroku"]);
    }

    #[test]
    fn collect_validation_layers_transitive_deps() {
        let layers = collect_validation_layers("flyway").unwrap();
        assert_eq!(layers, vec!["java", "flyway"]);
    }

    #[test]
    fn collect_validation_layers_unknown() {
        assert!(collect_validation_layers("nope").is_err());
    }

    #[test]
    fn compute_path_no_layers() {
        let layers: Vec<String> = vec![];
        let path = compute_path(&layers);
        assert!(path.starts_with("/home/claude/.local/bin:"));
        // Rust toolchain ships in the base image, so cargo/bin is always on PATH.
        assert!(path.contains("/usr/local/cargo/bin"));
        assert!(!path.contains("/usr/local/go/bin"));
    }

    #[test]
    fn compute_path_with_go() {
        let layers = vec!["go".to_string()];
        let path = compute_path(&layers);
        assert!(path.contains("/usr/local/go/bin"));
        assert!(path.contains("/usr/local/cargo/bin"));
    }
}
