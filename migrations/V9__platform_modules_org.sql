-- Platform modules + org tenancy schema for Enclagent stable v1.
--
-- These tables provide durable control-plane records for:
-- - org workspace ownership and enclave mapping
-- - membership and roles
-- - module catalog and per-workspace state
-- - frontdoor provisioning lifecycle
-- - skill fork source/release tracking
-- - verification artifact linkage

CREATE TABLE IF NOT EXISTS org_workspaces (
    id         TEXT        PRIMARY KEY,
    user_id    TEXT        NOT NULL,
    org_slug   TEXT        NOT NULL,
    name       TEXT        NOT NULL,
    enclave_id TEXT        NOT NULL,
    plan       TEXT        NOT NULL DEFAULT 'closed_beta',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (user_id, org_slug)
);

CREATE INDEX IF NOT EXISTS idx_org_workspaces_user ON org_workspaces (user_id);
CREATE INDEX IF NOT EXISTS idx_org_workspaces_org_slug ON org_workspaces (org_slug);

CREATE TABLE IF NOT EXISTS org_memberships (
    id           TEXT        PRIMARY KEY,
    workspace_id TEXT        NOT NULL REFERENCES org_workspaces(id) ON DELETE CASCADE,
    member_id    TEXT        NOT NULL,
    role         TEXT        NOT NULL,
    status       TEXT        NOT NULL DEFAULT 'active',
    invited_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (workspace_id, member_id)
);

CREATE INDEX IF NOT EXISTS idx_org_memberships_workspace ON org_memberships (workspace_id);
CREATE INDEX IF NOT EXISTS idx_org_memberships_member ON org_memberships (member_id);

CREATE TABLE IF NOT EXISTS module_catalog (
    id               TEXT        PRIMARY KEY,
    name             TEXT        NOT NULL,
    category         TEXT        NOT NULL,
    description      TEXT        NOT NULL,
    enabled_by_default BOOLEAN   NOT NULL DEFAULT FALSE,
    optional_addon   BOOLEAN     NOT NULL DEFAULT FALSE,
    capabilities     JSONB       NOT NULL DEFAULT '[]',
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS module_state (
    id           TEXT        PRIMARY KEY,
    workspace_id TEXT        NOT NULL REFERENCES org_workspaces(id) ON DELETE CASCADE,
    module_id    TEXT        NOT NULL REFERENCES module_catalog(id) ON DELETE RESTRICT,
    enabled      BOOLEAN     NOT NULL DEFAULT FALSE,
    status       TEXT        NOT NULL DEFAULT 'disabled',
    config       JSONB       NOT NULL DEFAULT '{}',
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (workspace_id, module_id)
);

CREATE INDEX IF NOT EXISTS idx_module_state_workspace ON module_state (workspace_id);
CREATE INDEX IF NOT EXISTS idx_module_state_module ON module_state (module_id);

CREATE TABLE IF NOT EXISTS frontdoor_sessions (
    id           TEXT        PRIMARY KEY,
    user_id      TEXT        NOT NULL,
    wallet_address TEXT      NOT NULL,
    version      BIGINT      NOT NULL,
    status       TEXT        NOT NULL,
    detail       TEXT        NOT NULL,
    privy_user_id TEXT,
    profile_name TEXT,
    instance_url TEXT,
    verify_url   TEXT,
    eigen_app_id TEXT,
    error        TEXT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at   TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_frontdoor_sessions_user_wallet ON frontdoor_sessions (user_id, wallet_address);
CREATE INDEX IF NOT EXISTS idx_frontdoor_sessions_status ON frontdoor_sessions (status);

CREATE TABLE IF NOT EXISTS provisioning_runs (
    id             TEXT        PRIMARY KEY,
    session_id     TEXT        NOT NULL REFERENCES frontdoor_sessions(id) ON DELETE CASCADE,
    workspace_id   TEXT        REFERENCES org_workspaces(id) ON DELETE SET NULL,
    attempt_number INTEGER     NOT NULL DEFAULT 1,
    status         TEXT        NOT NULL,
    error_code     TEXT,
    error_message  TEXT,
    instance_url   TEXT,
    verify_url     TEXT,
    eigen_app_id   TEXT,
    started_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at   TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_provisioning_runs_session ON provisioning_runs (session_id);
CREATE INDEX IF NOT EXISTS idx_provisioning_runs_status ON provisioning_runs (status);

CREATE TABLE IF NOT EXISTS skill_fork_sources (
    id               TEXT        PRIMARY KEY,
    source_name      TEXT        NOT NULL,
    source_type      TEXT        NOT NULL,
    source_url       TEXT        NOT NULL,
    release_pin      TEXT        NOT NULL,
    trust_policy     TEXT        NOT NULL,
    compatibility_policy TEXT    NOT NULL,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS skill_fork_releases (
    id               TEXT        PRIMARY KEY,
    source_id        TEXT        NOT NULL REFERENCES skill_fork_sources(id) ON DELETE CASCADE,
    release_tag      TEXT        NOT NULL,
    release_digest   TEXT,
    imported_skills  INTEGER     NOT NULL DEFAULT 0,
    rejected_skills  INTEGER     NOT NULL DEFAULT 0,
    compatibility_status TEXT    NOT NULL,
    security_status  TEXT        NOT NULL,
    imported_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (source_id, release_tag)
);

CREATE INDEX IF NOT EXISTS idx_skill_fork_releases_source ON skill_fork_releases (source_id);

CREATE TABLE IF NOT EXISTS verification_artifact_links (
    id                 TEXT        PRIMARY KEY,
    user_id            TEXT        NOT NULL,
    workspace_id       TEXT        REFERENCES org_workspaces(id) ON DELETE SET NULL,
    module_id          TEXT        REFERENCES module_catalog(id) ON DELETE SET NULL,
    intent_id          TEXT,
    execution_receipt_id TEXT,
    verification_record_id TEXT,
    chain_hash         TEXT,
    status             TEXT        NOT NULL,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_verification_artifacts_user ON verification_artifact_links (user_id);
CREATE INDEX IF NOT EXISTS idx_verification_artifacts_workspace ON verification_artifact_links (workspace_id);
CREATE INDEX IF NOT EXISTS idx_verification_artifacts_module ON verification_artifact_links (module_id);

